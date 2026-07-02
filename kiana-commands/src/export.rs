use crate::local_state::sdk_sessions_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct ExportCommand;

#[async_trait]
impl Command for ExportCommand {
    fn name(&self) -> &str {
        "export"
    }

    fn description(&self) -> &str {
        "Export conversation"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = parse_export_args(&context.args)?;
        if args.show_help {
            return Ok(CommandResult::text(usage()));
        }
        let session_id = args
            .session_id
            .clone()
            .or_else(|| current_session_id(&context.app_state));
        let cwd = context_cwd(&context.app_state)?;
        let format = args.format.unwrap_or_else(|| {
            if args
                .output_path
                .as_ref()
                .and_then(|path| path.extension())
                .and_then(|ext| ext.to_str())
                == Some("json")
            {
                ExportFormat::Json
            } else if session_id.is_some() {
                ExportFormat::Text
            } else {
                ExportFormat::Json
            }
        });

        let output = match session_id {
            Some(session_id) => {
                let session = read_session_json(&session_id)?;
                match format {
                    ExportFormat::Text => render_session_text(&session_id, &session)?,
                    ExportFormat::Json => serde_json::to_string_pretty(&session)?,
                }
            }
            None => serde_json::to_string_pretty(&context.app_state)?,
        };

        if let Some(path) = args.output_path {
            let path = resolve_output_path(&cwd, &path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::write(&path, format!("{output}\n"))
                .with_context(|| format!("failed to write {}", path.display()))?;
            return Ok(CommandResult::text(format!(
                "Conversation exported\nformat: {}\nfile: {}",
                format.name(),
                path.display()
            )));
        }

        Ok(CommandResult::text(output))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Text,
    Json,
}

impl ExportFormat {
    fn name(self) -> &'static str {
        match self {
            ExportFormat::Text => "text",
            ExportFormat::Json => "json",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ExportArgs {
    format: Option<ExportFormat>,
    output_path: Option<PathBuf>,
    session_id: Option<String>,
    show_help: bool,
}

fn parse_export_args(raw: &str) -> Result<ExportArgs> {
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let mut format = None;
    let mut output_path = None;
    let mut session_id = None;
    let mut show_help = false;
    let mut index = 0;

    while index < tokens.len() {
        match tokens[index] {
            "" => {}
            "-" => {}
            "help" | "--help" | "-h" => show_help = true,
            "json" | "--json" => format = Some(ExportFormat::Json),
            "text" | "--text" => format = Some(ExportFormat::Text),
            "--format" | "-f" => {
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| anyhow!("--format requires text or json"))?;
                format = Some(parse_export_format(value)?);
            }
            arg if arg.starts_with("--format=") => {
                let value = arg
                    .strip_prefix("--format=")
                    .ok_or_else(|| anyhow!("--format requires text or json"))?;
                format = Some(parse_export_format(value)?);
            }
            "--session" => {
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| anyhow!("--session requires a session id"))?;
                session_id = Some((*value).to_string());
            }
            arg if arg.starts_with("--session=") => {
                let value = arg
                    .strip_prefix("--session=")
                    .ok_or_else(|| anyhow!("--session requires a session id"))?;
                session_id = Some(value.to_string());
            }
            other if other.starts_with('-') => {
                return Err(anyhow!("unknown export option '{}'", other));
            }
            _ => {
                let path = tokens[index..].join(" ");
                output_path = Some(PathBuf::from(path));
                break;
            }
        }
        index += 1;
    }

    Ok(ExportArgs {
        format,
        output_path,
        session_id,
        show_help,
    })
}

fn usage() -> &'static str {
    "Usage:\n\
       kiana export [--session <id>] [--format text|json] [output_path]\n\
       kiana export --session <id> --text\n\
       kiana export --session <id> --json [output_path]\n\
     \n\
     Exports a local SDK session. Without --session, exports the current command context when one is available."
}

fn parse_export_format(value: &str) -> Result<ExportFormat> {
    match value {
        "text" | "txt" | "plain" => Ok(ExportFormat::Text),
        "json" => Ok(ExportFormat::Json),
        _ => Err(anyhow!("export format must be text or json")),
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

fn context_cwd(app_state: &std::collections::HashMap<String, Value>) -> Result<PathBuf> {
    if let Some(cwd) = app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(cwd));
    }
    std::env::current_dir().context("failed to read current directory")
}

fn read_session_json(session_id: &str) -> Result<Value> {
    validate_session_id(session_id)?;
    let path = sdk_sessions_dir().join(format!("{session_id}.json"));
    if !path.exists() {
        return read_session_from_event_tree(session_id);
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("session '{}' was not found", session_id))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse session file {}", path.display()))
}

fn read_session_from_event_tree(session_id: &str) -> Result<Value> {
    validate_session_id(session_id)?;
    let path = sdk_sessions_dir().join(session_id).join("events.jsonl");
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("session '{}' was not found", session_id))?;
    let mut messages = Vec::new();
    let mut created_at = None;
    let mut updated_at = None;

    for (line_index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: kiana_types::RuntimeEvent = serde_json::from_str(line).with_context(|| {
            format!(
                "failed to parse runtime event {} in {}",
                line_index + 1,
                path.display()
            )
        })?;
        if event.session_id != session_id {
            return Err(anyhow!(
                "runtime event {} in {} belongs to session '{}'",
                line_index + 1,
                path.display(),
                event.session_id
            ));
        }
        let event_time = parse_runtime_event_timestamp(&event.timestamp);
        created_at = Some(created_at.unwrap_or(event_time));
        updated_at = Some(event_time);
        match event.payload {
            kiana_types::RuntimeEventPayload::UserMessage(message)
            | kiana_types::RuntimeEventPayload::AssistantMessage(message) => {
                messages.push(message.message);
            }
            _ => {}
        }
    }

    let title = infer_session_title_from_messages(&messages).unwrap_or_else(|| session_id.into());
    Ok(serde_json::json!({
        "session_id": session_id,
        "title": title,
        "tag": null,
        "parent_session_id": null,
        "created_at": created_at.unwrap_or_default(),
        "updated_at": updated_at.unwrap_or_else(|| created_at.unwrap_or_default()),
        "messages": messages
    }))
}

fn parse_runtime_event_timestamp(timestamp: &str) -> u64 {
    timestamp.trim().parse::<u64>().unwrap_or_default()
}

fn infer_session_title_from_messages(messages: &[Value]) -> Option<String> {
    messages
        .iter()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .map(message_text)
        .map(|text| infer_title(&text))
        .filter(|title| !title.trim().is_empty())
}

fn infer_title(prompt: &str) -> String {
    let title = prompt.lines().next().unwrap_or(prompt).trim();
    let mut chars = title.chars();
    let shortened = chars.by_ref().take(80).collect::<String>();
    if chars.next().is_some() {
        format!("{}...", shortened)
    } else {
        shortened
    }
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

fn resolve_output_path(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

struct ExportTextEntry {
    role: String,
    text: String,
}

fn render_session_text(session_id: &str, session: &Value) -> Result<String> {
    let session_id = session
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or(session_id);
    let title = session.get("title").and_then(Value::as_str);
    let entries = runtime_events_for_text_export(session_id, session)?
        .into_iter()
        .flat_map(runtime_event_text_entries)
        .collect::<Vec<_>>();

    let mut lines = vec!["# Kiana Conversation Export".to_string(), String::new()];
    lines.push(format!("session_id: {session_id}"));
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        lines.push(format!("title: {title}"));
    }
    lines.push(format!("messages: {}", entries.len()));

    if entries.is_empty() {
        lines.push(String::new());
        lines.push("(no messages)".to_string());
        return Ok(lines.join("\n"));
    }

    for entry in entries {
        lines.push(String::new());
        lines.push(format!("## {}", title_case(&entry.role)));
        lines.push(String::new());
        lines.push(if entry.text.trim().is_empty() {
            "(empty message)".to_string()
        } else {
            entry.text
        });
    }

    Ok(lines.join("\n"))
}

fn runtime_events_for_text_export(
    session_id: &str,
    session: &Value,
) -> Result<Vec<kiana_types::RuntimeEvent>> {
    if let Some(events) = read_runtime_events_from_event_tree(session_id)? {
        return Ok(events);
    }
    Ok(runtime_events_from_session_messages(session_id, session))
}

fn read_runtime_events_from_event_tree(
    session_id: &str,
) -> Result<Option<Vec<kiana_types::RuntimeEvent>>> {
    validate_session_id(session_id)?;
    let path = sdk_sessions_dir().join(session_id).join("events.jsonl");
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut events = Vec::new();
    for (line_index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: kiana_types::RuntimeEvent = serde_json::from_str(line).with_context(|| {
            format!(
                "failed to parse runtime event {} in {}",
                line_index + 1,
                path.display()
            )
        })?;
        if event.session_id != session_id {
            return Err(anyhow!(
                "runtime event {} in {} belongs to session '{}'",
                line_index + 1,
                path.display(),
                event.session_id
            ));
        }
        events.push(event);
    }
    Ok(Some(events))
}

fn runtime_events_from_session_messages(
    session_id: &str,
    session: &Value,
) -> Vec<kiana_types::RuntimeEvent> {
    session
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            let timestamp = runtime_timestamp_from_message(&message);
            kiana_types::sdk_message_to_runtime_event(
                session_id,
                &format!("turn-{index}"),
                index.checked_sub(1).map(|parent| format!("turn-{parent}")),
                index as u64,
                &timestamp,
                message,
            )
        })
        .collect()
}

fn runtime_event_text_entries(event: kiana_types::RuntimeEvent) -> Vec<ExportTextEntry> {
    match event.payload {
        kiana_types::RuntimeEventPayload::UserMessage(message)
        | kiana_types::RuntimeEventPayload::AssistantMessage(message) => {
            message_text_entries(message.message)
        }
        kiana_types::RuntimeEventPayload::ToolCall(tool_call) => vec![ExportTextEntry {
            role: "tool".to_string(),
            text: format_runtime_tool_call_text(&tool_call),
        }],
        kiana_types::RuntimeEventPayload::ToolResult(tool_result) => vec![ExportTextEntry {
            role: "tool".to_string(),
            text: format_runtime_tool_result_text(&tool_result),
        }],
        kiana_types::RuntimeEventPayload::StreamDelta(delta) => {
            let text = delta
                .delta
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| message_text(&delta.delta));
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![ExportTextEntry {
                    role: "assistant".to_string(),
                    text,
                }]
            }
        }
        kiana_types::RuntimeEventPayload::PermissionRequest(permission) => {
            vec![ExportTextEntry {
                role: "system".to_string(),
                text: format!(
                    "Permission requested for {}.\nrequest_id: {}\naction: {}\ninput: {}",
                    permission.tool_name,
                    permission.request_id,
                    permission.action,
                    serde_json::to_string_pretty(&permission.input)
                        .unwrap_or_else(|_| permission.input.to_string())
                ),
            }]
        }
        kiana_types::RuntimeEventPayload::SessionEvent(session_event) => {
            if session_event.subtype == "sdk_message" {
                if let Some(message) = session_event.metadata.get("message").cloned() {
                    return message_text_entries(message);
                }
            }
            vec![ExportTextEntry {
                role: "system".to_string(),
                text: session_event
                    .message
                    .unwrap_or_else(|| format!("Session event: {}", session_event.subtype)),
            }]
        }
        kiana_types::RuntimeEventPayload::Error(error) => vec![ExportTextEntry {
            role: "system".to_string(),
            text: format!("Error: {}", error.message),
        }],
        kiana_types::RuntimeEventPayload::Result(result) => result
            .assistant_text
            .filter(|text| !text.trim().is_empty())
            .map(|text| ExportTextEntry {
                role: "assistant".to_string(),
                text,
            })
            .into_iter()
            .collect(),
    }
}

fn message_text_entries(message: Value) -> Vec<ExportTextEntry> {
    vec![ExportTextEntry {
        role: message_role(&message).to_string(),
        text: message_text(&message),
    }]
}

fn format_runtime_tool_call_text(tool_call: &kiana_types::RuntimeToolCallEvent) -> String {
    let mut lines = vec![
        format!("Tool requested: {}", tool_call.name),
        format!("tool_use_id: {}", tool_call.tool_call_id),
    ];
    if !tool_call.input.is_null()
        && !tool_call
            .input
            .as_object()
            .is_some_and(|object| object.is_empty())
    {
        lines.push(format!(
            "input:\n{}",
            serde_json::to_string_pretty(&tool_call.input)
                .unwrap_or_else(|_| tool_call.input.to_string())
        ));
    }
    lines.join("\n")
}

fn format_runtime_tool_result_text(tool_result: &kiana_types::RuntimeToolResultEvent) -> String {
    let status = if tool_result.is_error {
        "error"
    } else {
        "success"
    };
    let mut lines = vec![
        format!("tool_use_id: {}", tool_result.tool_call_id),
        format!("result: {status}"),
    ];
    let content = content_text(&tool_result.content);
    if !content.trim().is_empty() {
        lines.push(content);
    }
    lines.join("\n")
}

fn runtime_timestamp_from_message(message: &Value) -> String {
    match message.get("created_at") {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(value) => serde_json::to_string(value).unwrap_or_default(),
        None => String::new(),
    }
}

fn message_role(message: &Value) -> &str {
    message
        .get("role")
        .or_else(|| message.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("message")
}

fn message_text(message: &Value) -> String {
    if let Some(content) = message.get("content") {
        return content_text(content);
    }
    if let Some(content) = message
        .get("message")
        .and_then(|value| value.get("content"))
    {
        return content_text(content);
    }
    serde_json::to_string_pretty(message).unwrap_or_default()
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
            .unwrap_or_else(|| serde_json::to_string_pretty(content).unwrap_or_default()),
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
        return Some(serde_json::to_string_pretty(input).ok()?);
    }
    None
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return "Message".to_string();
    };
    format!("{}{}", first.to_uppercase(), chars.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_state::env_lock;
    use crate::Command;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-export-command-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, app_state: HashMap<String, Value>) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state,
        }
    }

    fn write_session(root: &Path, session_id: &str) {
        let sessions = root.join("sdk-sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join(format!("{session_id}.json")),
            serde_json::to_string_pretty(&json!({
                "session_id": session_id,
                "title": "Export test",
                "tag": "repl",
                "parent_session_id": null,
                "created_at": 1,
                "updated_at": 2,
                "messages": [
                    {"role": "user", "content": "Hello"},
                    {"role": "assistant", "content": [{"type": "text", "text": "Hi there"}]}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn write_jsonl_only_session(root: &Path, session_id: &str) {
        let sessions = root.join("sdk-sessions").join(session_id);
        std::fs::create_dir_all(&sessions).unwrap();
        let messages = [
            json!({"role": "user", "content": "jsonl export prompt"}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "jsonl export reply"}]}),
        ];
        let contents = messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                let event = kiana_types::sdk_message_to_runtime_event(
                    session_id,
                    &format!("turn-{index}"),
                    index.checked_sub(1).map(|parent| format!("turn-{parent}")),
                    index as u64,
                    &format!("{}", index + 1),
                    message.clone(),
                );
                serde_json::to_string(&event).unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(sessions.join("events.jsonl"), format!("{contents}\n")).unwrap();
    }

    fn write_jsonl_only_session_with_tool_events(root: &Path, session_id: &str) {
        let sessions = root.join("sdk-sessions").join(session_id);
        std::fs::create_dir_all(&sessions).unwrap();
        let events = vec![
            kiana_types::sdk_message_to_runtime_event(
                session_id,
                "turn-0",
                None,
                0,
                "1",
                json!({"role": "user", "content": "inspect file"}),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-tool-call",
                session_id,
                "turn-1",
                Some("turn-0".to_string()),
                1,
                "2",
                kiana_types::RuntimeEventPayload::ToolCall(kiana_types::RuntimeToolCallEvent {
                    tool_call_id: "toolu_read".to_string(),
                    name: "Read".to_string(),
                    workbench: None,
                    input: json!({"file_path": "src/lib.rs"}),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-tool-result",
                session_id,
                "turn-1",
                Some("turn-0".to_string()),
                2,
                "3",
                kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                    tool_call_id: "toolu_read".to_string(),
                    name: Some("Read".to_string()),
                    workbench: None,
                    is_error: false,
                    content: json!("pub fn main() {}"),
                }),
            ),
            kiana_types::sdk_message_to_runtime_event(
                session_id,
                "turn-2",
                Some("turn-1".to_string()),
                3,
                "4",
                json!({"role": "assistant", "content": [{"type": "text", "text": "done"}]}),
            ),
        ];
        let contents = events
            .into_iter()
            .map(|event| serde_json::to_string(&event).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(sessions.join("events.jsonl"), format!("{contents}\n")).unwrap();
    }

    fn write_jsonl_only_session_with_full_runtime_events(root: &Path, session_id: &str) {
        let sessions = root.join("sdk-sessions").join(session_id);
        std::fs::create_dir_all(&sessions).unwrap();
        let events = vec![
            kiana_types::RuntimeEvent::new(
                "evt-1",
                session_id,
                "turn-0",
                None,
                0,
                "100",
                kiana_types::RuntimeEventPayload::UserMessage(kiana_types::MessageRuntimeEvent {
                    message: json!({
                        "role": "user",
                        "content": "start"
                    }),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-2",
                session_id,
                "turn-0",
                None,
                1,
                "101",
                kiana_types::RuntimeEventPayload::AssistantMessage(
                    kiana_types::MessageRuntimeEvent {
                        message: json!({
                            "role": "assistant",
                            "content": [{"type": "text", "text": "answer"}]
                        }),
                    },
                ),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-3",
                session_id,
                "turn-0",
                None,
                2,
                "102",
                kiana_types::RuntimeEventPayload::StreamDelta(
                    kiana_types::RuntimeStreamDeltaEvent {
                        delta: json!({"text": "streaming chunk"}),
                    },
                ),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-4",
                session_id,
                "turn-0",
                None,
                3,
                "103",
                kiana_types::RuntimeEventPayload::ToolCall(kiana_types::RuntimeToolCallEvent {
                    tool_call_id: "toolu_read".to_string(),
                    name: "Read".to_string(),
                    workbench: Some("local".to_string()),
                    input: json!({"file_path": "src/lib.rs"}),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-5",
                session_id,
                "turn-0",
                None,
                4,
                "104",
                kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                    tool_call_id: "toolu_read".to_string(),
                    name: Some("Read".to_string()),
                    workbench: Some("local".to_string()),
                    is_error: true,
                    content: json!("permission denied"),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-6",
                session_id,
                "turn-0",
                None,
                5,
                "105",
                kiana_types::RuntimeEventPayload::PermissionRequest(
                    kiana_types::RuntimePermissionRequestEvent {
                        request_id: "req-1".to_string(),
                        tool_name: "Write".to_string(),
                        action: "ask".to_string(),
                        input: json!({"file_path": "src/main.rs"}),
                        reason: Some("workspace policy".to_string()),
                    },
                ),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-7",
                session_id,
                "turn-0",
                None,
                6,
                "106",
                kiana_types::RuntimeEventPayload::SessionEvent(kiana_types::RuntimeSessionEvent {
                    subtype: "started".to_string(),
                    message: Some("session started".to_string()),
                    metadata: json!({}),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-8",
                session_id,
                "turn-0",
                None,
                7,
                "107",
                kiana_types::RuntimeEventPayload::Error(kiana_types::RuntimeErrorEvent {
                    code: Some("provider_error".to_string()),
                    message: "provider failed".to_string(),
                    details: json!({"retryable": false}),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-9",
                session_id,
                "turn-0",
                None,
                8,
                "108",
                kiana_types::RuntimeEventPayload::Result(kiana_types::RuntimeResultEvent {
                    status: "completed".to_string(),
                    assistant_text: Some("final answer".to_string()),
                    metadata: json!({"duration_ms": 12}),
                }),
            ),
        ];
        let contents = events
            .into_iter()
            .map(|event| serde_json::to_string(&event).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(sessions.join("events.jsonl"), format!("{contents}\n")).unwrap();
    }

    #[tokio::test]
    async fn export_uses_current_session_from_context() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_session(&root, "session-1");
        let app_state = HashMap::from([
            ("session_id".to_string(), json!("session-1")),
            ("cwd".to_string(), json!(root.to_string_lossy().to_string())),
        ]);

        let result = ExportCommand.execute(context("", app_state)).await.unwrap();

        assert!(result.value.contains("# Kiana Conversation Export"));
        assert!(result.value.contains("session_id: session-1"));
        assert!(result.value.contains("## User"));
        assert!(result.value.contains("Hello"));
        assert!(result.value.contains("Hi there"));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn export_can_write_session_json_to_file() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_session(&root, "session-2");
        let export_path = root.join("exports").join("conversation.json");

        let result = ExportCommand
            .execute(context(
                &format!(
                    "--session session-2 --format json {}",
                    export_path.to_string_lossy()
                ),
                HashMap::new(),
            ))
            .await
            .unwrap();

        assert!(result.value.contains("format: json"));
        let exported = std::fs::read_to_string(export_path).unwrap();
        assert!(exported.contains("\"session_id\": \"session-2\""));
        assert!(exported.contains("\"messages\""));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn export_reads_jsonl_only_session_tree() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_jsonl_only_session(&root, "jsonl-export");

        let result = ExportCommand
            .execute(context("--session jsonl-export --text", HashMap::new()))
            .await
            .unwrap();

        assert!(result.value.contains("# Kiana Conversation Export"));
        assert!(result.value.contains("session_id: jsonl-export"));
        assert!(result.value.contains("jsonl export prompt"));
        assert!(result.value.contains("jsonl export reply"));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn export_text_renders_runtime_tool_events_from_jsonl_tree() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_jsonl_only_session_with_tool_events(&root, "jsonl-tools");

        let result = ExportCommand
            .execute(context("--session jsonl-tools --text", HashMap::new()))
            .await
            .unwrap();

        assert!(result.value.contains("# Kiana Conversation Export"));
        assert!(result.value.contains("session_id: jsonl-tools"));
        assert!(result.value.contains("## Tool"));
        assert!(result.value.contains("Tool requested: Read"));
        assert!(result.value.contains("tool_use_id: toolu_read"));
        assert!(result.value.contains("src/lib.rs"));
        assert!(result.value.contains("result: success"));
        assert!(result.value.contains("pub fn main()"));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn export_text_renders_full_runtime_event_fixture_from_jsonl_tree() {
        let _guard = env_lock().lock().unwrap();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_jsonl_only_session_with_full_runtime_events(&root, "jsonl-full-events");

        let result = ExportCommand
            .execute(context(
                "--session jsonl-full-events --text",
                HashMap::new(),
            ))
            .await
            .unwrap();

        assert_eq!(
            result.value,
            concat!(
                "# Kiana Conversation Export\n",
                "\n",
                "session_id: jsonl-full-events\n",
                "title: start\n",
                "messages: 9\n",
                "\n",
                "## User\n",
                "\n",
                "start\n",
                "\n",
                "## Assistant\n",
                "\n",
                "answer\n",
                "\n",
                "## Assistant\n",
                "\n",
                "streaming chunk\n",
                "\n",
                "## Tool\n",
                "\n",
                "Tool requested: Read\n",
                "tool_use_id: toolu_read\n",
                "input:\n",
                "{\n",
                "  \"file_path\": \"src/lib.rs\"\n",
                "}\n",
                "\n",
                "## Tool\n",
                "\n",
                "tool_use_id: toolu_read\n",
                "result: error\n",
                "permission denied\n",
                "\n",
                "## System\n",
                "\n",
                "Permission requested for Write.\n",
                "request_id: req-1\n",
                "action: ask\n",
                "input: {\n",
                "  \"file_path\": \"src/main.rs\"\n",
                "}\n",
                "\n",
                "## System\n",
                "\n",
                "session started\n",
                "\n",
                "## System\n",
                "\n",
                "Error: provider failed\n",
                "\n",
                "## Assistant\n",
                "\n",
                "final answer"
            )
        );

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn export_without_session_preserves_context_json_fallback() {
        let app_state = HashMap::from([("tasks".to_string(), json!([{ "id": "t1" }]))]);

        let result = ExportCommand
            .execute(context("json", app_state))
            .await
            .unwrap();

        assert!(result.value.contains("\"tasks\""));
        assert!(result.value.contains("\"id\": \"t1\""));
    }

    #[test]
    fn parse_export_args_accepts_format_and_session_flags() {
        assert_eq!(
            parse_export_args("--session abc --format json out.json").unwrap(),
            ExportArgs {
                format: Some(ExportFormat::Json),
                output_path: Some(PathBuf::from("out.json")),
                session_id: Some("abc".to_string()),
                show_help: false,
            }
        );
    }

    #[test]
    fn parse_export_args_accepts_help_flag() {
        assert_eq!(
            parse_export_args("--help").unwrap(),
            ExportArgs {
                format: None,
                output_path: None,
                session_id: None,
                show_help: true,
            }
        );
    }

    #[test]
    fn export_session_id_validation_rejects_whitespace() {
        assert!(validate_session_id("missing session").is_err());
        assert!(validate_session_id("missing\tsession").is_err());
    }
}
