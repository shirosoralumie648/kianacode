use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    pub threshold_tokens: u32,
    pub target_tokens: u32,
    pub enabled: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            threshold_tokens: 100_000,
            target_tokens: 50_000,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionReport {
    pub messages: Vec<String>,
    pub compacted: bool,
    pub original_message_count: usize,
    pub compacted_message_count: usize,
    pub omitted_message_count: usize,
    pub kept_prefix_count: usize,
    pub kept_suffix_count: usize,
    pub original_estimated_tokens: u32,
    pub compacted_estimated_tokens: u32,
    pub target_tokens: u32,
    pub summary: Option<String>,
}

pub async fn compact_context(
    messages: Vec<String>,
    config: CompactionConfig,
) -> crate::errors::ServiceResult<Vec<String>> {
    Ok(compact_context_report(messages, config).await?.messages)
}

pub async fn compact_context_report(
    messages: Vec<String>,
    config: CompactionConfig,
) -> crate::errors::ServiceResult<CompactionReport> {
    let original_estimated_tokens = estimate_messages_tokens(&messages);
    let original_message_count = messages.len();

    if !config.enabled
        || messages.len() <= 2
        || original_estimated_tokens <= config.threshold_tokens
        || config.target_tokens >= original_estimated_tokens
    {
        return Ok(CompactionReport {
            compacted_estimated_tokens: original_estimated_tokens,
            messages,
            compacted: false,
            original_message_count,
            compacted_message_count: original_message_count,
            omitted_message_count: 0,
            kept_prefix_count: original_message_count,
            kept_suffix_count: 0,
            original_estimated_tokens,
            target_tokens: config.target_tokens,
            summary: None,
        });
    }

    let kept_prefix_count = 1;
    let summary_reserve = (config.target_tokens / 5).clamp(128, 1_024);
    let available_for_kept = config.target_tokens.saturating_sub(summary_reserve).max(1);
    let mut kept_suffix_count = 0usize;
    let mut kept_tokens = estimate_messages_tokens(&messages[..kept_prefix_count]);

    for index in (kept_prefix_count..messages.len()).rev() {
        let message_tokens = estimate_message_tokens(&messages[index]);
        if kept_suffix_count > 0 && kept_tokens.saturating_add(message_tokens) > available_for_kept
        {
            break;
        }
        kept_suffix_count += 1;
        kept_tokens = kept_tokens.saturating_add(message_tokens);
    }

    if kept_prefix_count + kept_suffix_count >= messages.len() {
        return Ok(CompactionReport {
            compacted_estimated_tokens: original_estimated_tokens,
            messages,
            compacted: false,
            original_message_count,
            compacted_message_count: original_message_count,
            omitted_message_count: 0,
            kept_prefix_count: original_message_count,
            kept_suffix_count: 0,
            original_estimated_tokens,
            target_tokens: config.target_tokens,
            summary: None,
        });
    }

    let omitted_start = kept_prefix_count;
    let omitted_end = messages.len() - kept_suffix_count;
    let omitted = &messages[omitted_start..omitted_end];
    let omitted_message_count = omitted.len();
    let omitted_estimated_tokens = estimate_messages_tokens(omitted);
    let summary = build_summary(
        omitted,
        omitted_message_count,
        omitted_estimated_tokens,
        original_estimated_tokens,
        config.target_tokens,
    );

    let mut compacted_messages = Vec::with_capacity(kept_prefix_count + 1 + kept_suffix_count);
    compacted_messages.extend(messages.iter().take(kept_prefix_count).cloned());
    compacted_messages.push(summary.clone());
    compacted_messages.extend(messages.iter().skip(omitted_end).cloned());
    let compacted_estimated_tokens = estimate_messages_tokens(&compacted_messages);
    let compacted_message_count = compacted_messages.len();

    Ok(CompactionReport {
        messages: compacted_messages,
        compacted: true,
        original_message_count,
        compacted_message_count,
        omitted_message_count,
        kept_prefix_count,
        kept_suffix_count,
        original_estimated_tokens,
        compacted_estimated_tokens,
        target_tokens: config.target_tokens,
        summary: Some(summary),
    })
}

pub fn estimate_message_tokens(message: &str) -> u32 {
    let chars = message.chars().count() as u32;
    let words = message.split_whitespace().count() as u32;
    ((chars + 3) / 4).max(words).max(1)
}

pub fn estimate_messages_tokens(messages: &[String]) -> u32 {
    messages
        .iter()
        .map(|message| estimate_message_tokens(message).saturating_add(1))
        .fold(0u32, u32::saturating_add)
}

fn build_summary(
    omitted: &[String],
    omitted_message_count: usize,
    omitted_estimated_tokens: u32,
    original_estimated_tokens: u32,
    target_tokens: u32,
) -> String {
    let mut lines = vec![
        "[Compacted conversation summary]".to_string(),
        format!("omitted_messages: {omitted_message_count}"),
        format!("omitted_estimated_tokens: {omitted_estimated_tokens}"),
        format!("original_estimated_tokens: {original_estimated_tokens}"),
        format!("target_estimated_tokens: {target_tokens}"),
        "omitted_preview:".to_string(),
    ];

    let preview_count = omitted.len().min(6);
    for (index, message) in omitted.iter().take(preview_count).enumerate() {
        lines.push(format!("{}. {}", index + 1, excerpt(message, 280)));
    }
    if omitted.len() > preview_count {
        lines.push(format!(
            "... {} more omitted message(s)",
            omitted.len() - preview_count
        ));
    }
    lines.join("\n")
}

fn excerpt(message: &str, max_chars: usize) -> String {
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let excerpt = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{excerpt}...")
    } else if excerpt.is_empty() {
        "(empty message)".to_string()
    } else {
        excerpt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(label: &str, repeat: usize) -> String {
        format!("{} {}", label, "detail ".repeat(repeat))
    }

    #[tokio::test]
    async fn compact_context_returns_original_when_disabled() {
        let messages = vec![message("first", 10), message("second", 10)];

        let report = compact_context_report(
            messages.clone(),
            CompactionConfig {
                enabled: false,
                threshold_tokens: 1,
                target_tokens: 1,
            },
        )
        .await
        .unwrap();

        assert!(!report.compacted);
        assert_eq!(report.messages, messages);
    }

    #[tokio::test]
    async fn compact_context_returns_original_below_threshold() {
        let messages = vec![message("small", 1), message("tail", 1)];

        let report = compact_context_report(
            messages.clone(),
            CompactionConfig {
                enabled: true,
                threshold_tokens: 10_000,
                target_tokens: 10,
            },
        )
        .await
        .unwrap();

        assert!(!report.compacted);
        assert_eq!(report.messages, messages);
    }

    #[tokio::test]
    async fn compact_context_replaces_middle_messages_with_summary() {
        let messages = vec![
            message("first", 80),
            message("middle-one", 80),
            message("middle-two", 80),
            message("middle-three", 80),
            message("last", 8),
        ];

        let report = compact_context_report(
            messages.clone(),
            CompactionConfig {
                enabled: true,
                threshold_tokens: 10,
                target_tokens: 80,
            },
        )
        .await
        .unwrap();

        assert!(report.compacted);
        assert_eq!(report.kept_prefix_count, 1);
        assert!(report.kept_suffix_count >= 1);
        assert!(report.omitted_message_count >= 1);
        assert_eq!(report.messages.first(), messages.first());
        assert_eq!(report.messages.last(), messages.last());
        assert!(report.messages[1].contains("[Compacted conversation summary]"));
        assert!(report.messages[1].contains("middle-one"));
        assert!(report.compacted_estimated_tokens < report.original_estimated_tokens);
    }

    #[tokio::test]
    async fn compact_context_vec_api_returns_compacted_messages() {
        let messages = vec![
            message("first", 60),
            message("middle", 60),
            message("last", 5),
        ];

        let compacted = compact_context(
            messages,
            CompactionConfig {
                enabled: true,
                threshold_tokens: 10,
                target_tokens: 60,
            },
        )
        .await
        .unwrap();

        assert!(compacted
            .iter()
            .any(|message| message.contains("omitted_messages")));
    }
}
