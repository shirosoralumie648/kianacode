//! Codex-style token-budget compaction for the owned harness.
//!
//! Derived from OpenAI Codex (Apache-2.0) `codex-rs/core/src/compact.rs`:
//! `build_compacted_history`, `is_summary_message`, and the token-budget path
//! that skips model/server summarization and installs a fresh context window.
//! Copied into `kiana-runner`; `reference/` is audit-only.

use crate::model::{ModelMessage, ModelRole};

/// Marker Codex uses so compacted summaries can be recognized later.
pub const SUMMARY_PREFIX: &str = r#"Another language model started to solve this problem and produced a summary of its thinking process. You also have access to the state of the tools that were used by that language model. Use this to build on the work that has already been done and avoid duplicating work. Here is the summary produced by the other language model, use the information in this summary to assist with your own analysis:"#;

const APPROX_BYTES_PER_TOKEN: usize = 4;
/// Codex `COMPACT_USER_MESSAGE_MAX_TOKENS` for retained user turns after compact.
pub const COMPACT_USER_MESSAGE_MAX_TOKENS: usize = 20_000;
/// Auto-compact trigger when no model context window is configured.
pub const DEFAULT_COMPACT_TRIGGER_TOKENS: usize = 32_000;

pub fn approx_token_count(text: &str) -> usize {
    let len = text.len();
    len.saturating_add(APPROX_BYTES_PER_TOKEN.saturating_sub(1)) / APPROX_BYTES_PER_TOKEN
}

pub fn history_tokens(messages: &[ModelMessage]) -> usize {
    messages
        .iter()
        .map(|message| approx_token_count(&message.text))
        .sum()
}

pub fn is_summary_message(message: &str) -> bool {
    message.starts_with(&format!("{SUMMARY_PREFIX}\n"))
}

pub fn compact_if_needed(
    messages: Vec<ModelMessage>,
    trigger_tokens: usize,
    retain_tokens: usize,
) -> Vec<ModelMessage> {
    if trigger_tokens == 0 || history_tokens(&messages) <= trigger_tokens {
        return messages;
    }
    build_compacted_history(&messages, retain_tokens)
}

/// Codex `build_compacted_history`: keep recent real user messages under a
/// token budget, then append a summary user message last.
pub fn build_compacted_history(
    messages: &[ModelMessage],
    max_user_tokens: usize,
) -> Vec<ModelMessage> {
    let user_messages: Vec<&str> = messages
        .iter()
        .filter(|message| message.role == ModelRole::User && !is_summary_message(&message.text))
        .map(|message| message.text.as_str())
        .collect();

    let mut selected = Vec::new();
    if max_user_tokens > 0 {
        let mut remaining = max_user_tokens;
        for message in user_messages.iter().rev() {
            if remaining == 0 {
                break;
            }
            let tokens = approx_token_count(message);
            if tokens <= remaining {
                selected.push((*message).to_owned());
                remaining -= tokens;
            } else {
                selected.push(truncate_middle_tokens(message, remaining));
                break;
            }
        }
        selected.reverse();
    }

    let mut history: Vec<ModelMessage> = selected.into_iter().map(ModelMessage::user).collect();
    history.push(ModelMessage::user(format!(
        "{SUMMARY_PREFIX}\n(no summary available)"
    )));
    history
}

fn truncate_middle_tokens(text: &str, max_tokens: usize) -> String {
    let max_bytes = max_tokens.saturating_mul(APPROX_BYTES_PER_TOKEN);
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    if max_bytes <= 3 {
        return truncate_utf8(text, max_bytes);
    }
    let keep = (max_bytes - 3) / 2;
    let prefix = truncate_utf8(text, keep);
    let mut start = text.len().saturating_sub(keep);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    format!("{}...{}", prefix, &text[start..])
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacted_history_keeps_recent_user_messages_and_appends_summary() {
        let messages = vec![
            ModelMessage::user("old-1"),
            ModelMessage::assistant("ignored assistant"),
            ModelMessage::user("keep-me-recent"),
        ];
        let compacted = build_compacted_history(&messages, 4);
        assert!(compacted
            .iter()
            .all(|message| message.role == ModelRole::User));
        assert!(!compacted
            .iter()
            .any(|message| message.text == "ignored assistant"));
        assert_eq!(
            compacted
                .last()
                .map(|message| is_summary_message(&message.text)),
            Some(true)
        );
        assert!(compacted
            .iter()
            .any(|message| message.text == "keep-me-recent"));
    }

    #[test]
    fn compact_if_needed_is_noop_under_trigger() {
        let messages = vec![ModelMessage::user("short")];
        let out = compact_if_needed(messages.clone(), 1_000, 20);
        assert_eq!(out, messages);
    }

    #[test]
    fn compact_if_needed_replaces_over_budget_history() {
        let messages = vec![
            ModelMessage::user("a".repeat(80)),
            ModelMessage::assistant("tooling"),
            ModelMessage::user("b".repeat(80)),
        ];
        let out = compact_if_needed(messages, 8, 4);
        assert!(is_summary_message(&out.last().unwrap().text));
        assert!(
            history_tokens(&out) <= history_tokens(&[out[0].clone(), out[1].clone()]) + 1
                || out.len() <= 3
        );
        assert!(!out
            .iter()
            .any(|message| message.role == ModelRole::Assistant));
    }
}
