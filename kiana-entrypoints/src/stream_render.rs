//! Human-readable CLI rendering for additive run-stream events.
//!
//! The stream is a display projection only. The renderer writes model text deltas as
//! they arrive, but it never treats the stream closing or the text itself as a
//! terminal fact: only a `Terminal` envelope ends the command.

use anyhow::{anyhow, Context, Result};
use kiana_daemon::{RunStreamSubscription, StreamingRedactor};
use kiana_protocol::{ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent, UiCursor};
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
                if let Some(response) = renderer.handle_envelope(run_id, event)? {
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
        if let Some(response) = renderer.handle_envelope(run_id, event)? {
            return Ok(response);
        }
    }
}

    /// 把 broadcast 通道的错误翻译成一条可读的原因。
    ///
    /// 【两种错误含义完全不同】
    /// - `Lagged(n)`：订阅者处理太慢，**丢了** n 条事件。这不是「结束」，
    ///   而是一段事实的缺失——必须让调用方知道中间有空洞。
    /// - `Closed`：通道的发送端全没了，通常意味着这次 run 真的结束了。
    ///
    /// 【⚠ 为什么 `Lagged` 不能当成正常收尾】
    /// 把它当成收尾的话，界面会停在一个「看起来跑完了」的位置，而实际上中间漏了
    /// 若干步。丢事件和事件流结束，在屏幕上可能长得一模一样。
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
    cursor: UiCursor,
}

impl<W: Write> StreamRenderer<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            redactor: StreamingRedactor::new(),
            streamed_text: false,
            cursor: UiCursor::default(),
        }
    }

    fn handle_envelope(
        &mut self,
        run_id: RunId,
        envelope: RunStreamEnvelope,
    ) -> Result<Option<ResponseEnvelope>> {
        if !envelope
            .advance_cursor(&mut self.cursor)
            .map_err(anyhow::Error::msg)?
        {
            return Ok(None);
        }
        self.handle_event(run_id, envelope.event)
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
            RunStreamEvent::Usage { .. }
            | RunStreamEvent::ToolCall { .. }
            | RunStreamEvent::ApprovalRequested { .. }
            | RunStreamEvent::Error { .. }
            | RunStreamEvent::Unknown => Ok(None),
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

    /// 确认事件里的 run 就是我们等的那个 run。
    ///
    /// 【为什么一个「显示层」要核对 run_id】
    /// 因为渲染器订阅的是一条**共享的** run 流。切��会话、取消后重开、或者两个 run
    /// 短暂重叠时，订阅里可能混进别的 run 的事件。
    ///
    /// 而一旦渲染器把 A 的事件画到了 B 的界面上，读者看到的就是
    /// 「B 做到���一步」——**一个凭空出现的进度**。这不是显示瑕疵，是会误导人的假事实。
    ///
    /// 【为什么不「忽略掉不匹配」而是报错】
    /// 忽略会让渲染器继续跑，而界面继续显示一条混合的时间线。报错让这一次渲染整体作废，
    /// 界面停在最后一个确定的状态上。后者更难看，但**不会骗人**。
    ///
    /// 【⚠ 它保证的是「没串台」，不是「事件是事实」】
    /// run_id 对得上，只说明这条事件属于这次运行。事件本身仍然是一个**流式投影**，
    /// 不是 EventLog 里的事实——文件头那句话在这里同样成立。
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
