//! Owned harness 使用的 Codex 风格 token 压缩器。
//!
//! 当对话历史超过本地触发阈值时，本模块只保留最近的用户消息，并在末尾插入固定摘要
//! 标记，建立一个较小的新上下文窗口。它不调用模型生成摘要，也不修改 EventLog 或真实
//! 会话事实；摘要只是可丢弃的 Runner 上下文视图。token 估算按字节近似，不能当作供应商
//! tokenizer、计费或硬预算证明。
//!
//! 实现形状参考 OpenAI Codex 的公开压缩逻辑；`reference/` 仍然只是审计输入，当前代码
//! 不依赖其中的源码。

use crate::model::{ModelMessage, ModelRole};

/// 压缩摘要使用的固定前缀，用于后续识别并避免把摘要当作真实用户轮次。
pub const SUMMARY_PREFIX: &str = r#"Another language model started to solve this problem and produced a summary of its thinking process. You also have access to the state of the tools that were used by that language model. Use this to build on the work that has already been done and avoid duplicating work. Here is the summary produced by the other language model, use the information in this summary to assist with your own analysis:"#;

const APPROX_BYTES_PER_TOKEN: usize = 4;
/// 压缩后保留的用户消息 token 上限（当前为 20,000）。
pub const COMPACT_USER_MESSAGE_MAX_TOKENS: usize = 20_000;
/// 未配置模型上下文窗口时采用的自动压缩触发值（当前为 32,000）。
pub const DEFAULT_COMPACT_TRIGGER_TOKENS: usize = 32_000;

/// 以约 4 字节/token 的固定比例估算文本 token 数。
///
/// 该函数只服务于压缩的确定性上限；非 ASCII 文本、代码和真实模型 tokenizer 的结果可能
/// 差异很大，因此不能用它判断供应商请求是否一定不会超限。
pub fn approx_token_count(text: &str) -> usize {
    let len = text.len();
    len.saturating_add(APPROX_BYTES_PER_TOKEN.saturating_sub(1)) / APPROX_BYTES_PER_TOKEN
}

/// 汇总消息文本的近似 token 数。
pub fn history_tokens(messages: &[ModelMessage]) -> usize {
    messages
        .iter()
        .map(|message| approx_token_count(&message.text))
        .sum()
}

/// 判断一段文本是否已经带有压缩摘要前缀。
pub fn is_summary_message(message: &str) -> bool {
    message.starts_with(&format!("{SUMMARY_PREFIX}\n"))
}

#[derive(Clone, Debug, PartialEq)]
/// 一次压缩检查的结果和前后 token 估算。
pub struct CompactOutcome {
    /// 压缩后要交给模型的消息列表。
    pub messages: Vec<ModelMessage>,
    /// 是否实际替换了历史。
    pub applied: bool,
    /// 压缩前的近似 token 数。
    pub tokens_before: usize,
    /// 压缩后的近似 token 数。
    pub tokens_after: usize,
    /// 结果中是否包含摘要消息。
    pub summary_present: bool,
}

/// 在历史超过触发阈值时构建压缩上下文。
///
/// `trigger_tokens == 0` 或历史未超限时原样返回消息；超限后调用
/// [`build_compacted_history`]，不会尝试部分修改原向量。调用方应把 `applied` 和 token
/// 统计写入展示/收据，但不要把摘要当成新的业务事实。
pub fn compact_if_needed(
    messages: Vec<ModelMessage>,
    trigger_tokens: usize,
    retain_tokens: usize,
) -> CompactOutcome {
    let tokens_before = history_tokens(&messages);
    if trigger_tokens == 0 || tokens_before <= trigger_tokens {
        return CompactOutcome {
            summary_present: messages
                .iter()
                .any(|message| is_summary_message(&message.text)),
            tokens_after: tokens_before,
            messages,
            applied: false,
            tokens_before,
        };
    }
    let compacted = build_compacted_history(&messages, retain_tokens);
    let tokens_after = history_tokens(&compacted);
    CompactOutcome {
        applied: true,
        tokens_before,
        tokens_after,
        summary_present: compacted
            .iter()
            .any(|message| is_summary_message(&message.text)),
        messages: compacted,
    }
}

/// 只保留最近真实用户消息并在末尾追加固定摘要消息。
///
/// Assistant、Tool 和已有摘要消息不会被保留；从后往前选择可以优先保住最新上下文，
/// 最后再恢复时间顺序。最后一条过长用户消息会做中间截断，保留开头和结尾以减少关键
/// 目标/结果同时丢失的概率。`max_user_tokens == 0` 时只生成摘要消息。
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
        // 逆序选择最新消息；预算不足时仅截断当前最靠近末尾的一条。
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
    // 使用 ASCII "..."，避免再次引入一个可能改变字节预算的 Unicode 字符。
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
        assert!(!out.applied);
        assert_eq!(out.messages, messages);
        assert_eq!(out.tokens_after, out.tokens_before);
    }

    #[test]
    fn compact_if_needed_replaces_over_budget_history() {
        let messages = vec![
            ModelMessage::user("a".repeat(80)),
            ModelMessage::assistant("tooling"),
            ModelMessage::user("b".repeat(80)),
        ];
        let out = compact_if_needed(messages, 8, 4);
        assert!(out.applied);
        assert!(out.summary_present);
        assert!(is_summary_message(&out.messages.last().unwrap().text));
        assert!(!out
            .messages
            .iter()
            .any(|message| message.role == ModelRole::Assistant));
    }

    #[test]
    fn compact_outcome_shrinks_large_discarded_history() {
        let messages = vec![ModelMessage::user("a".repeat(2000))];
        let out = compact_if_needed(messages, 200, 40);
        assert!(out.applied);
        assert!(out.summary_present);
        assert!(
            out.tokens_after < out.tokens_before,
            "before={} after={}",
            out.tokens_before,
            out.tokens_after
        );
    }
}
