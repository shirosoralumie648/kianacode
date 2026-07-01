use crate::local_state::sdk_sessions_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use kiana_services::compact::{compact_context_report, CompactionConfig, CompactionReport};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CompactCommand;

#[async_trait]
impl Command for CompactCommand {
    fn name(&self) -> &str {
        "compact"
    }

    fn description(&self) -> &str {
        "Compact a local SDK session"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = parse_args(&context.args)?;
        if args.show_help {
            return Ok(CommandResult::text(usage()));
        }
        let session_id = resolve_session_id(&args, &context.app_state)?;
        let mut session = read_session_json(&session_id)?;
        let messages = session_messages(&session)?;
        let rendered_messages = messages.iter().map(render_message).collect::<Vec<_>>();
        let report = compact_context_report(rendered_messages, args.config.clone()).await?;

        if !report.compacted {
            return Ok(CommandResult::text(format!(
                "Session compact skipped\nid: {}\nmessages: {}\nestimated_tokens: {}\nthreshold_tokens: {}",
                session_id,
                report.original_message_count,
                report.original_estimated_tokens,
                args.config.threshold_tokens
            )));
        }

        if args.dry_run {
            return Ok(CommandResult::text(format_compact_report(
                "Session compact dry run",
                &session_id,
                &report,
            )));
        }

        let compacted_messages = compact_session_messages(messages, &report)?;
        session["messages"] = Value::Array(compacted_messages);
        session["updated_at"] = json!(now_unix_seconds());
        write_session_json(&session_id, &session)?;
        rewrite_runtime_events_from_session(&session_id, &session)?;

        let mut result = CommandResult::text(format_compact_report(
            "Session compacted",
            &session_id,
            &report,
        ));
        result.metadata = Some(HashMap::from([
            ("session_id".to_string(), session_id),
            ("session_transcript_mutated".to_string(), "true".to_string()),
        ]));
        Ok(result)
    }
}

#[derive(Debug, Clone)]
struct CompactArgs {
    session_id: Option<String>,
    latest: bool,
    dry_run: bool,
    show_help: bool,
    config: CompactionConfig,
}

fn parse_args(raw: &str) -> Result<CompactArgs> {
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let mut args = CompactArgs {
        session_id: None,
        latest: false,
        dry_run: false,
        show_help: false,
        config: CompactionConfig::default(),
    };
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index] {
            "" => {}
            "help" | "--help" | "-h" => args.show_help = true,
            "current" => args.session_id = Some("current".to_string()),
            "--latest" | "latest" => args.latest = true,
            "--dry-run" | "--check" => args.dry_run = true,
            "--threshold-tokens" => {
                index += 1;
                args.config.threshold_tokens =
                    parse_u32_arg("--threshold-tokens", tokens.get(index).copied())?;
            }
            value if value.starts_with("--threshold-tokens=") => {
                args.config.threshold_tokens = parse_u32_arg(
                    "--threshold-tokens",
                    value.strip_prefix("--threshold-tokens="),
                )?;
            }
            "--target-tokens" => {
                index += 1;
                args.config.target_tokens =
                    parse_u32_arg("--target-tokens", tokens.get(index).copied())?;
            }
            value if value.starts_with("--target-tokens=") => {
                args.config.target_tokens =
                    parse_u32_arg("--target-tokens", value.strip_prefix("--target-tokens="))?;
            }
            "--disabled" | "--no-compact" => args.config.enabled = false,
            other if other.starts_with('-') => {
                return Err(anyhow!("unknown compact option '{}'\n\n{}", other, usage()));
            }
            session_id => {
                if args.session_id.is_some() {
                    return Err(anyhow!(
                        "compact accepts at most one session id\n\n{}",
                        usage()
                    ));
                }
                args.session_id = Some(session_id.to_string());
            }
        }
        index += 1;
    }

    if args.config.target_tokens == 0 || args.config.threshold_tokens == 0 {
        return Err(anyhow!("compact token limits must be positive"));
    }
    Ok(args)
}

fn parse_u32_arg(flag: &str, value: Option<&str>) -> Result<u32> {
    let value = value.ok_or_else(|| anyhow!("{flag} requires a value"))?;
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow!("{flag} requires a positive integer"))
}

fn resolve_session_id(
    args: &CompactArgs,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<String> {
    if args.latest {
        return latest_session_id();
    }
    match args.session_id.as_deref() {
        Some("current") | None => current_session_id(app_state).ok_or_else(|| {
            anyhow!("no current session in command context; pass a session id or --latest")
        }),
        Some(session_id) => {
            validate_session_id(session_id)?;
            Ok(session_id.to_string())
        }
    }
}

fn current_session_id(app_state: &std::collections::HashMap<String, Value>) -> Option<String> {
    app_state
        .get("session_id")
        .or_else(|| app_state.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn latest_session_id() -> Result<String> {
    let dir = sdk_sessions_dir();
    let mut latest: Option<(u64, String)> = None;
    if !dir.is_dir() {
        return Err(anyhow!(
            "no SDK sessions found; sessions_dir: {}",
            dir.display()
        ));
    }
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        let Some(session_id) = value.get("session_id").and_then(Value::as_str) else {
            continue;
        };
        if path.file_stem().and_then(|stem| stem.to_str()) != Some(session_id) {
            continue;
        }
        let updated_at = value.get("updated_at").and_then(Value::as_u64).unwrap_or(0);
        if latest
            .as_ref()
            .is_none_or(|(latest_updated_at, _)| updated_at > *latest_updated_at)
        {
            latest = Some((updated_at, session_id.to_string()));
        }
    }
    latest
        .map(|(_, session_id)| session_id)
        .ok_or_else(|| anyhow!("no SDK sessions found; sessions_dir: {}", dir.display()))
}

fn read_session_json(session_id: &str) -> Result<Value> {
    let path = session_file(session_id)?;
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("session '{}' was not found", session_id))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse session file {}", path.display()))
}

fn write_session_json(session_id: &str, session: &Value) -> Result<()> {
    let dir = sdk_sessions_dir();
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = session_file(session_id)?;
    let tmp = dir.join(format!(".{}.{}.tmp", session_id, std::process::id()));
    fs::write(&tmp, serde_json::to_string_pretty(session)?)
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("failed to replace {}", path.display()))
}

fn rewrite_runtime_events_from_session(session_id: &str, session: &Value) -> Result<()> {
    validate_session_id(session_id)?;
    let messages = session_messages(session)?;
    let event_dir = sdk_sessions_dir().join(session_id);
    fs::create_dir_all(&event_dir)
        .with_context(|| format!("failed to create {}", event_dir.display()))?;
    let path = event_dir.join("events.jsonl");
    let mut contents = String::new();
    for (index, message) in messages.iter().enumerate() {
        let event = kiana_types::sdk_message_to_runtime_event(
            session_id,
            &format!("turn-{index}"),
            index.checked_sub(1).map(|parent| format!("turn-{parent}")),
            index as u64,
            &runtime_timestamp_from_message(message),
            message.clone(),
        );
        contents.push_str(&serde_json::to_string(&event)?);
        contents.push('\n');
    }
    fs::write(&path, contents).with_context(|| format!("failed to write {}", path.display()))
}

fn runtime_timestamp_from_message(message: &Value) -> String {
    match message.get("created_at") {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(value) => {
            serde_json::to_string(value).unwrap_or_else(|_| now_unix_seconds().to_string())
        }
        None => now_unix_seconds().to_string(),
    }
}

fn session_file(session_id: &str) -> Result<PathBuf> {
    validate_session_id(session_id)?;
    Ok(sdk_sessions_dir().join(format!("{session_id}.json")))
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.trim().is_empty()
        || session_id.chars().any(char::is_whitespace)
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
    {
        return Err(anyhow!("session id contains invalid path characters"));
    }
    Ok(())
}

fn session_messages(session: &Value) -> Result<Vec<Value>> {
    session
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("session file is missing a messages array"))
}

fn compact_session_messages(messages: Vec<Value>, report: &CompactionReport) -> Result<Vec<Value>> {
    let suffix_start = messages
        .len()
        .checked_sub(report.kept_suffix_count)
        .ok_or_else(|| anyhow!("invalid compaction suffix count"))?;
    let mut compacted = Vec::with_capacity(report.compacted_message_count);
    compacted.extend(messages.iter().take(report.kept_prefix_count).cloned());
    compacted.push(summary_message(report)?);
    compacted.extend(messages.iter().skip(suffix_start).cloned());
    Ok(compacted)
}

fn summary_message(report: &CompactionReport) -> Result<Value> {
    let summary = report
        .summary
        .as_deref()
        .ok_or_else(|| anyhow!("compaction report did not contain a summary"))?;
    Ok(json!({
        "role": "user",
        "content": [{
            "type": "text",
            "text": summary
        }],
        "is_compact_summary": true,
        "compact_metadata": {
            "omitted_message_count": report.omitted_message_count,
            "original_message_count": report.original_message_count,
            "compacted_message_count": report.compacted_message_count,
            "original_estimated_tokens": report.original_estimated_tokens,
            "compacted_estimated_tokens": report.compacted_estimated_tokens,
            "target_tokens": report.target_tokens,
            "kept_prefix_count": report.kept_prefix_count,
            "kept_suffix_count": report.kept_suffix_count
        }
    }))
}

fn render_message(message: &Value) -> String {
    let role = message
        .get("role")
        .or_else(|| message.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("message");
    let text = message
        .get("content")
        .map(content_text)
        .unwrap_or_else(|| serde_json::to_string(message).unwrap_or_default());
    format!("{role}: {text}")
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.trim().to_string(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(block_text)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        Value::Object(_) => block_text(content)
            .unwrap_or_else(|| serde_json::to_string(content).unwrap_or_default()),
        _ => content.to_string(),
    }
}

fn block_text(block: &Value) -> Option<String> {
    if let Some(text) = block.get("text").and_then(Value::as_str) {
        return Some(text.trim().to_string());
    }
    if let Some(text) = block.get("content").and_then(Value::as_str) {
        return Some(text.trim().to_string());
    }
    if let Some(input) = block.get("input") {
        return serde_json::to_string(input).ok();
    }
    None
}

fn format_compact_report(label: &str, session_id: &str, report: &CompactionReport) -> String {
    format!(
        "{label}\nid: {session_id}\nmessages: {} -> {}\nomitted_messages: {}\nestimated_tokens: {} -> {}\ntarget_tokens: {}",
        report.original_message_count,
        report.compacted_message_count,
        report.omitted_message_count,
        report.original_estimated_tokens,
        report.compacted_estimated_tokens,
        report.target_tokens
    )
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn usage() -> &'static str {
    "Usage:\n  kiana compact <session_id|current> [--threshold-tokens N] [--target-tokens N]\n  kiana compact --latest [--dry-run]\n\nCompacts a local SDK session by preserving the first and recent messages while summarizing the middle."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_state::env_lock;
    use crate::Command;
    use std::collections::HashMap;
    use std::path::Path;

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-compact-command-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, app_state: HashMap<String, Value>) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state,
        }
    }

    fn write_test_session(root: &Path, session_id: &str) {
        let dir = root.join("sdk-sessions");
        fs::create_dir_all(&dir).unwrap();
        let messages = vec![
            json!({"role": "user", "content": "first message ".repeat(80)}),
            json!({"role": "assistant", "content": "middle one ".repeat(80)}),
            json!({"role": "user", "content": "middle two ".repeat(80)}),
            json!({"role": "assistant", "content": "middle three ".repeat(80)}),
            json!({"role": "user", "content": "recent tail"}),
        ];
        fs::write(
            dir.join(format!("{session_id}.json")),
            serde_json::to_string_pretty(&json!({
                "session_id": session_id,
                "title": "Compact test",
                "tag": null,
                "parent_session_id": null,
                "created_at": 1,
                "updated_at": 2,
                "messages": messages
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn write_runtime_event_tree(root: &Path, session_id: &str) {
        let session = read_session_json(session_id).unwrap();
        let messages = session["messages"].as_array().unwrap();
        let event_dir = root.join("sdk-sessions").join(session_id);
        fs::create_dir_all(&event_dir).unwrap();
        let contents = messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                let event = kiana_types::sdk_message_to_runtime_event(
                    session_id,
                    &format!("turn-{index}"),
                    index.checked_sub(1).map(|parent| format!("turn-{parent}")),
                    index as u64,
                    "2026-06-23T00:00:00Z",
                    message.clone(),
                );
                serde_json::to_string(&event).unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(event_dir.join("events.jsonl"), format!("{contents}\n")).unwrap();
    }

    #[tokio::test]
    async fn compact_dry_run_does_not_rewrite_session() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-compact-dry");
        let before =
            fs::read_to_string(root.join("sdk-sessions").join("session-compact-dry.json")).unwrap();

        let result = CompactCommand
            .execute(context(
                "session-compact-dry --dry-run --threshold-tokens 10 --target-tokens 80",
                HashMap::new(),
            ))
            .await
            .unwrap();
        let after =
            fs::read_to_string(root.join("sdk-sessions").join("session-compact-dry.json")).unwrap();

        assert!(result.value.contains("Session compact dry run"));
        assert_eq!(before, after);

        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn compact_rewrites_session_with_summary_message() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-compact-write");

        let result = CompactCommand
            .execute(context(
                "session-compact-write --threshold-tokens 10 --target-tokens 80",
                HashMap::new(),
            ))
            .await
            .unwrap();
        let session = read_session_json("session-compact-write").unwrap();
        let messages = session["messages"].as_array().unwrap();

        assert!(result.value.contains("Session compacted"));
        assert!(messages.len() < 5);
        assert_eq!(messages[0]["role"], "user");
        assert!(messages
            .iter()
            .any(|message| message["is_compact_summary"] == json!(true)));
        assert_eq!(messages.last().unwrap()["content"], "recent tail");

        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn compact_rebuilds_runtime_event_tree_after_rewriting_messages() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-compact-events");
        write_runtime_event_tree(&root, "session-compact-events");

        CompactCommand
            .execute(context(
                "session-compact-events --threshold-tokens 10 --target-tokens 80",
                HashMap::new(),
            ))
            .await
            .unwrap();

        let session = read_session_json("session-compact-events").unwrap();
        let messages = session["messages"].as_array().unwrap();
        let event_file = root
            .join("sdk-sessions")
            .join("session-compact-events")
            .join("events.jsonl");
        let events = fs::read_to_string(&event_file)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", event_file.display()))
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(events.len(), messages.len());
        assert!(events
            .iter()
            .any(|event| event["message"]["is_compact_summary"] == json!(true)));
        assert_eq!(events[0]["sequence"], 0);
        assert_eq!(events[0]["turn_id"], "turn-0");
        assert_eq!(
            events.last().unwrap()["message"]["content"],
            messages.last().unwrap()["content"]
        );

        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn compact_can_target_latest_session_explicitly() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-compact-latest");

        let result = CompactCommand
            .execute(context(
                "--latest --dry-run --threshold-tokens 10 --target-tokens 80",
                HashMap::new(),
            ))
            .await
            .unwrap();

        assert!(result.value.contains("session-compact-latest"));

        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn compact_help_returns_usage_text() {
        let result = CompactCommand
            .execute(context("--help", HashMap::new()))
            .await
            .unwrap();

        assert!(result.value.contains("kiana compact"));
        assert!(result.value.contains("--latest"));
    }

    #[test]
    fn compact_session_id_validation_rejects_whitespace() {
        assert!(validate_session_id("missing session").is_err());
        assert!(validate_session_id("missing\tsession").is_err());
    }
}
