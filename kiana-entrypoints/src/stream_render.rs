//! Human-readable CLI rendering for additive run-stream events.
//!
//! The stream is a display projection only. The renderer writes model text deltas as
//! they arrive, but it never treats the stream closing or the text itself as a
//! terminal fact: only a `Terminal` envelope ends the command.

use anyhow::{anyhow, Context, Result};
use kiana_daemon::{RunStreamSubscription, StreamingRedactor};
use kiana_protocol::{ResponseEnvelope, RunId, RunStreamEvent};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, Write};
use std::time::Duration;

const TERMINAL_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn run_envelope(
    session_id: String,
    prompt: String,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let run_id = RunId::parse_str(&session_id).ok_or_else(|| anyhow!("stream_run_id_required"))?;
    let host = crate::harness_run::new_local_host_with_options(options)?;
    let mut subscription = host.subscribe_run(run_id);
    let options = options.clone();
    let mut run_task = tokio::spawn(async move {
        crate::harness_run::run_envelope_on_host(host, session_id, prompt, &options).await
    });

    let mut renderer = StreamRenderer::new(io::stdout());
    loop {
        tokio::select! {
            response = &mut run_task => {
                let response = response
                    .map_err(|error| anyhow!("stream_run_task_failed:{error}"))??;
                if !response.status.is_terminal() {
                    return Err(anyhow!(
                        "stream_terminal_missing:{}",
                        response.status.as_str()
                    ));
                }
                return tokio::time::timeout(
                    TERMINAL_WAIT_TIMEOUT,
                    wait_for_terminal(&mut subscription, run_id, &mut renderer),
                )
                .await
                .map_err(|_| anyhow!("stream_terminal_missing:timeout"))?;
            }
            event = subscription.recv() => {
                let event = event.map_err(stream_recv_error)?;
                if let Some(response) = renderer.handle_event(run_id, event.event)? {
                    return Ok(response);
                }
            }
        }
    }
}

async fn wait_for_terminal<W: Write>(
    subscription: &mut RunStreamSubscription,
    run_id: RunId,
    renderer: &mut StreamRenderer<W>,
) -> Result<ResponseEnvelope> {
    loop {
        let event = subscription.recv().await.map_err(stream_recv_error)?;
        if let Some(response) = renderer.handle_event(run_id, event.event)? {
            return Ok(response);
        }
    }
}

fn stream_recv_error(error: tokio::sync::broadcast::error::RecvError) -> anyhow::Error {
    match error {
        tokio::sync::broadcast::error::RecvError::Lagged(skipped) => {
            anyhow!("stream_subscription_lagged:{skipped}")
        }
        tokio::sync::broadcast::error::RecvError::Closed => {
            anyhow!("stream_terminal_missing:closed")
        }
    }
}

struct StreamRenderer<W: Write> {
    writer: W,
    redactor: StreamingRedactor,
    streamed_text: bool,
}

impl<W: Write> StreamRenderer<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            redactor: StreamingRedactor::new(),
            streamed_text: false,
        }
    }

    fn handle_event(
        &mut self,
        expected_run_id: RunId,
        event: RunStreamEvent,
    ) -> Result<Option<ResponseEnvelope>> {
        match event {
            RunStreamEvent::Delta { run_id, text } => {
                ensure_run_id(expected_run_id, run_id)?;
                self.write_delta(&text)?;
                Ok(None)
            }
            RunStreamEvent::Terminal { run_id, response } => {
                ensure_run_id(expected_run_id, run_id)?;
                self.flush_redactor()?;
                self.finish_line()?;
                Ok(Some(response))
            }
            RunStreamEvent::Unknown => Ok(None),
        }
    }

    fn write_delta(&mut self, text: &str) -> Result<()> {
        let text = self.redactor.push(text);
        if text.is_empty() {
            return Ok(());
        }
        self.write_redacted(&text)
    }

    fn flush_redactor(&mut self) -> Result<()> {
        let tail = self.redactor.finish();
        if tail.is_empty() {
            return Ok(());
        }
        self.write_redacted(&tail)
    }

    fn write_redacted(&mut self, text: &str) -> Result<()> {
        self.writer
            .write_all(text.as_bytes())
            .context("stream_stdout_write_failed")?;
        self.writer.flush().context("stream_stdout_flush_failed")?;
        self.streamed_text = true;
        Ok(())
    }

    fn finish_line(&mut self) -> Result<()> {
        if !self.streamed_text {
            return Ok(());
        }
        self.writer
            .write_all(b"\n")
            .context("stream_stdout_write_failed")?;
        self.writer.flush().context("stream_stdout_flush_failed")?;
        self.streamed_text = false;
        Ok(())
    }
}

fn ensure_run_id(expected: RunId, actual: RunId) -> Result<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(anyhow!("stream_run_id_mismatch"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_protocol::{ExecutionStatus, RequestId, PROTOCOL_SCHEMA};

    #[derive(Default)]
    struct RecordingWriter {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl Write for RecordingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    struct BackpressureWriter;

    impl Write for BackpressureWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stdout backpressure",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn completed_response() -> ResponseEnvelope {
        ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: RequestId::new(),
            status: ExecutionStatus::Completed,
            output: serde_json::json!({"schema": "kiana.run-result.v1"}),
            error: None,
        }
    }

    #[test]
    fn renders_and_flushes_each_delta_before_terminal() {
        let run_id = RunId::new();
        let mut renderer = StreamRenderer::new(RecordingWriter::default());

        assert!(renderer
            .handle_event(
                run_id,
                RunStreamEvent::Delta {
                    run_id,
                    text: "alpha".to_owned(),
                },
            )
            .unwrap()
            .is_none());
        assert_eq!(renderer.writer.bytes, b"alpha");
        assert_eq!(renderer.writer.flushes, 1);

        assert!(renderer
            .handle_event(
                run_id,
                RunStreamEvent::Delta {
                    run_id,
                    text: " beta".to_owned(),
                },
            )
            .unwrap()
            .is_none());
        assert_eq!(renderer.writer.bytes, b"alpha beta");
        assert_eq!(renderer.writer.flushes, 2);

        let response = completed_response();
        assert_eq!(
            renderer
                .handle_event(
                    run_id,
                    RunStreamEvent::Terminal {
                        run_id,
                        response: response.clone(),
                    },
                )
                .unwrap(),
            Some(response)
        );
        assert_eq!(renderer.writer.bytes, b"alpha beta\n");
        assert_eq!(renderer.writer.flushes, 3);
    }

    #[test]
    fn rejects_a_delta_for_another_run_without_writing() {
        let run_id = RunId::new();
        let mut renderer = StreamRenderer::new(RecordingWriter::default());
        let error = renderer
            .handle_event(
                run_id,
                RunStreamEvent::Delta {
                    run_id: RunId::new(),
                    text: "wrong".to_owned(),
                },
            )
            .unwrap_err();

        assert_eq!(error.to_string(), "stream_run_id_mismatch");
        assert!(renderer.writer.bytes.is_empty());
        assert_eq!(renderer.writer.flushes, 0);
    }

    #[test]
    fn split_secret_across_deltas_never_reaches_stdout() {
        let run_id = RunId::new();
        let mut renderer = StreamRenderer::new(RecordingWriter::default());

        for text in ["Authorization: Bear", "er stdout-split-sentinel"] {
            renderer
                .handle_event(
                    run_id,
                    RunStreamEvent::Delta {
                        run_id,
                        text: text.to_owned(),
                    },
                )
                .unwrap();
        }

        let stdout = String::from_utf8(renderer.writer.bytes).expect("stdout is UTF-8");
        assert!(
            !stdout.contains("stdout-split-sentinel"),
            "split secret reached stdout: {stdout}"
        );
    }

    #[test]
    fn stdout_backpressure_fails_closed_without_completing() {
        let run_id = RunId::new();
        let mut renderer = StreamRenderer::new(BackpressureWriter);

        let error = renderer
            .handle_event(
                run_id,
                RunStreamEvent::Delta {
                    run_id,
                    text: "alpha".to_owned(),
                },
            )
            .expect_err("stdout backpressure must fail closed");

        assert_eq!(error.to_string(), "stream_stdout_write_failed");
        assert!(!renderer.streamed_text);
    }

    #[test]
    fn lagged_subscription_never_returns_a_completed_response() {
        let error = stream_recv_error(tokio::sync::broadcast::error::RecvError::Lagged(3));
        assert_eq!(error.to_string(), "stream_subscription_lagged:3");
    }
}
