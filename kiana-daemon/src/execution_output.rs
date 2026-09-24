//! Shared bounded output capture, redaction and preview projection.

use kiana_domain::ExecutionOutputBudget;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::task::JoinHandle;

const READ_CHUNK_SIZE: usize = 8192;
const IO_DRAIN_TIMEOUT: Duration = Duration::from_millis(2000);

#[derive(Debug)]
pub(crate) struct CappedOutput {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
    pub(crate) observed_bytes: u64,
    pub(crate) lines: u64,
    pub(crate) read_error: Option<String>,
}

pub(crate) async fn read_capped<R>(
    mut reader: R,
    budget: ExecutionOutputBudget,
) -> std::io::Result<CappedOutput>
where
    R: AsyncRead + Unpin,
{
    budget.validate().map_err(std::io::Error::other)?;
    let mut bytes = Vec::with_capacity(READ_CHUNK_SIZE.min(budget.collect_max_bytes));
    let mut tmp = [0u8; READ_CHUNK_SIZE];
    let mut truncated = false;
    let mut observed_bytes = 0u64;
    let mut lines = 0u64;
    let mut read_error = None;
    loop {
        let n = match reader.read(&mut tmp).await {
            Ok(count) => count,
            Err(error) => {
                read_error = Some(error.kind().to_string());
                break;
            }
        };
        if n == 0 {
            break;
        }
        observed_bytes = observed_bytes.saturating_add(n as u64);
        lines = lines.saturating_add(tmp[..n].iter().filter(|byte| **byte == b'\n').count() as u64);
        if observed_bytes > budget.observed_max_bytes as u64 {
            truncated = true;
            read_error = Some("output_total_limit".to_owned());
            break;
        }
        if lines > budget.max_lines as u64 {
            truncated = true;
            read_error = Some("output_line_limit".to_owned());
            break;
        }
        if bytes.len() >= budget.collect_max_bytes {
            truncated = true;
            continue;
        }
        let take = (budget.collect_max_bytes - bytes.len()).min(n);
        bytes.extend_from_slice(&tmp[..take]);
        if take < n {
            truncated = true;
        }
    }
    Ok(CappedOutput {
        bytes,
        truncated,
        observed_bytes,
        lines,
        read_error,
    })
}

pub(crate) async fn drain_capped(
    handle: &mut JoinHandle<std::io::Result<CappedOutput>>,
) -> CappedOutput {
    match tokio::time::timeout(IO_DRAIN_TIMEOUT, &mut *handle).await {
        Ok(Ok(Ok(output))) => output,
        Ok(Ok(Err(_))) | Ok(Err(_)) => CappedOutput {
            bytes: Vec::new(),
            truncated: false,
            observed_bytes: 0,
            lines: 0,
            read_error: Some("output_reader_failed".to_owned()),
        },
        Err(_) => {
            handle.abort();
            CappedOutput {
                bytes: Vec::new(),
                truncated: false,
                observed_bytes: 0,
                lines: 0,
                read_error: Some("output_drain_timeout".to_owned()),
            }
        }
    }
}

/// Redact before terminal-control filtering so secrets split across input chunks are never
/// reintroduced by the display projection.
pub(crate) fn safe_output_text(text: &str) -> String {
    safe_output_text_for(kiana_domain::SecretScanChannel::Stdout, text)
}

pub(crate) fn safe_output_text_for(channel: kiana_domain::SecretScanChannel, text: &str) -> String {
    let text = kiana_domain::redact_text(text);
    let mut output = String::new();
    let mut state = 0u8;
    for ch in text.chars() {
        match state {
            0 if ch == '\u{1b}' => state = 1,
            0 if ch == '\n' || ch == '\t' || !ch.is_control() => output.push(ch),
            0 => {}
            1 if ch == '[' => state = 2,
            1 if ch == ']' => state = 3,
            1 => state = 0,
            2 if ('@'..='~').contains(&ch) => state = 0,
            3 if ch == '\u{7}' => state = 0,
            3 if ch == '\u{1b}' => state = 4,
            4 if ch == '\\' => state = 0,
            4 => state = 3,
            _ => {}
        }
    }
    if kiana_domain::scan_secret_sentinels(channel, &output).is_ok() {
        output
    } else {
        "[REDACTED]".to_owned()
    }
}

pub(crate) fn render_capped(bytes: &[u8], truncated: bool, preview_max_bytes: usize) -> String {
    render_capped_for(
        kiana_domain::SecretScanChannel::Stdout,
        bytes,
        truncated,
        preview_max_bytes,
    )
}

pub(crate) fn render_capped_for(
    channel: kiana_domain::SecretScanChannel,
    bytes: &[u8],
    truncated: bool,
    preview_max_bytes: usize,
) -> String {
    let mut text = safe_output_text_for(channel, &String::from_utf8_lossy(bytes));
    if text.len() > preview_max_bytes {
        let mut end = preview_max_bytes.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n...preview-truncated...");
    } else if truncated {
        text.push_str("\n...truncated...");
    }
    text
}

pub(crate) fn bounded_preview(text: &str, preview_max_bytes: usize) -> String {
    render_capped(text.as_bytes(), false, preview_max_bytes)
}

pub(crate) fn metadata(output: &CappedOutput, budget: &ExecutionOutputBudget) -> Value {
    json!({
        "captured_bytes": output.bytes.len(),
        "observed_bytes": output.observed_bytes,
        "lines": output.lines,
        "truncated": output.truncated,
        "read_error": output.read_error,
        "budget": {
            "collect_max_bytes": budget.collect_max_bytes,
            "preview_max_bytes": budget.preview_max_bytes,
            "persist_max_bytes": budget.persist_max_bytes,
            "observed_max_bytes": budget.observed_max_bytes,
            "max_lines": budget.max_lines,
        }
    })
}
