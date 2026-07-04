use crate::types::{ContentBlock, ControlRequestType, SDKMessage};
use kiana_types::{
    MessageRuntimeEvent, RuntimeErrorEvent, RuntimeEvent, RuntimeEventPayload,
    RuntimePermissionRequestEvent, RuntimeResultEvent, RuntimeSessionEvent,
    RuntimeStreamDeltaEvent, RuntimeToolCallEvent, RuntimeToolResultEvent,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

pub fn runtime_events_from_bridge_sdk_message(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    start_sequence: u64,
    timestamp: &str,
    msg: SDKMessage,
) -> Vec<RuntimeEvent> {
    match msg {
        SDKMessage::Assistant { message, .. } => {
            let message_value = content_to_message_value("assistant", message.content);
            let mut events = vec![runtime_event(
                session_id,
                turn_id,
                parent_turn_id.clone(),
                start_sequence,
                timestamp,
                RuntimeEventPayload::AssistantMessage(MessageRuntimeEvent {
                    message: message_value.clone(),
                }),
            )];
            append_tool_events_from_message(
                &mut events,
                session_id,
                turn_id,
                parent_turn_id,
                timestamp,
                start_sequence + 1,
                &message_value,
            );
            events
        }
        SDKMessage::User { message, .. } => {
            let message_value = content_to_message_value("user", message.content);
            let mut events = vec![runtime_event(
                session_id,
                turn_id,
                parent_turn_id.clone(),
                start_sequence,
                timestamp,
                RuntimeEventPayload::UserMessage(MessageRuntimeEvent {
                    message: message_value.clone(),
                }),
            )];
            append_tool_events_from_message(
                &mut events,
                session_id,
                turn_id,
                parent_turn_id,
                timestamp,
                start_sequence + 1,
                &message_value,
            );
            events
        }
        SDKMessage::StreamEvent { data } => vec![runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            RuntimeEventPayload::StreamDelta(RuntimeStreamDeltaEvent { delta: json!(data) }),
        )],
        SDKMessage::Result { data } => {
            let status = data
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed")
                .to_string();
            let assistant_text = data
                .get("assistant_text")
                .and_then(Value::as_str)
                .map(str::to_string);
            if status == "completed" || status == "success" {
                vec![runtime_event(
                    session_id,
                    turn_id,
                    parent_turn_id,
                    start_sequence,
                    timestamp,
                    RuntimeEventPayload::Result(RuntimeResultEvent {
                        status,
                        stop_reason: data
                            .get("stop_reason")
                            .and_then(Value::as_str)
                            .filter(|reason| !reason.trim().is_empty())
                            .unwrap_or("model_stop")
                            .to_string(),
                        assistant_text,
                        metadata: json!(data),
                    }),
                )]
            } else {
                vec![runtime_event(
                    session_id,
                    turn_id,
                    parent_turn_id,
                    start_sequence,
                    timestamp,
                    RuntimeEventPayload::Error(RuntimeErrorEvent {
                        code: Some(status),
                        message: data
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("Unknown bridge result error")
                            .to_string(),
                        details: json!(data),
                    }),
                )]
            }
        }
        SDKMessage::System { data } => vec![runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            RuntimeEventPayload::SessionEvent(RuntimeSessionEvent {
                subtype: data
                    .get("subtype")
                    .and_then(Value::as_str)
                    .unwrap_or("system")
                    .to_string(),
                message: data
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                metadata: json!(data),
            }),
        )],
        SDKMessage::ToolProgress { data } => vec![runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            RuntimeEventPayload::SessionEvent(RuntimeSessionEvent {
                subtype: "tool_progress".to_string(),
                message: data.get("tool_name").and_then(Value::as_str).map(|name| {
                    format!(
                        "{} running for {}s",
                        name,
                        data.get("elapsed_time_seconds")
                            .and_then(Value::as_f64)
                            .unwrap_or_default()
                    )
                }),
                metadata: json!(data),
            }),
        )],
        SDKMessage::ControlRequest {
            request_id,
            request,
        } => runtime_event_from_bridge_control_request(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            &request_id,
            request,
        )
        .into_iter()
        .collect(),
        SDKMessage::ControlResponse { .. }
        | SDKMessage::ControlCancelRequest { .. }
        | SDKMessage::UpdateEnvironmentVariables { .. }
        | SDKMessage::Unknown => Vec::new(),
    }
}

pub fn runtime_event_from_bridge_control_request(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    request_id: &str,
    request: ControlRequestType,
) -> Option<RuntimeEvent> {
    match request {
        ControlRequestType::CanUseTool {
            tool_name,
            input,
            tool_use_id: _,
        } => Some(runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            sequence,
            timestamp,
            RuntimeEventPayload::PermissionRequest(RuntimePermissionRequestEvent {
                request_id: request_id.to_string(),
                tool_name,
                action: "can_use_tool".to_string(),
                input: json!(input),
                reason: None,
            }),
        )),
        _ => None,
    }
}

pub fn bridge_session_changes_report(
    session_id: &str,
    events: &[RuntimeEvent],
) -> BridgeSessionChangesReport {
    let mut files = BTreeMap::<String, BridgeSessionChangedFile>::new();
    let mut change_count = 0usize;

    for event in events {
        if event.session_id != session_id {
            continue;
        }
        let RuntimeEventPayload::ToolResult(tool_result) = &event.payload else {
            continue;
        };
        let Some(changed_files) = tool_result.changed_files.as_ref() else {
            continue;
        };
        collect_bridge_changed_files(&mut files, &mut change_count, changed_files);
    }

    BridgeSessionChangesReport {
        schema: "kiana.diff.session_changes.v1",
        session_id: session_id.to_string(),
        changed: !files.is_empty(),
        file_count: files.len(),
        change_count,
        files: files.into_values().collect(),
    }
}

fn collect_bridge_changed_files(
    files: &mut BTreeMap<String, BridgeSessionChangedFile>,
    change_count: &mut usize,
    changed_files: &Value,
) {
    let Some(items) = changed_files.as_array() else {
        return;
    };

    for item in items {
        let Some(path) = item.get("path").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if path.is_empty() || safe_relative_path(path).is_none() {
            continue;
        }

        *change_count += 1;
        let entry = files
            .entry(path.to_string())
            .or_insert_with(|| BridgeSessionChangedFile {
                path: path.to_string(),
                operations: BTreeSet::new(),
                sources: BTreeSet::new(),
            });

        if let Some(operation) = item
            .get("operation")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            entry.operations.insert(operation.to_string());
        }
        if let Some(source) = item
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            entry.sources.insert(source.to_string());
        }
    }
}

pub fn bridge_events_view_report(
    session_id: &str,
    events: &[RuntimeEvent],
) -> BridgeEventsViewReport {
    let mut messages = Vec::new();

    for event in events {
        if event.session_id != session_id {
            continue;
        }
        match &event.payload {
            RuntimeEventPayload::UserMessage(message) => {
                messages.extend(bridge_message_view_messages(
                    event,
                    &message.message,
                    "user",
                ));
            }
            RuntimeEventPayload::AssistantMessage(message) => {
                messages.extend(bridge_message_view_messages(
                    event,
                    &message.message,
                    "assistant",
                ));
            }
            RuntimeEventPayload::StreamDelta(delta) => {
                let content = bridge_runtime_text(&delta.delta);
                if !content.trim().is_empty() {
                    messages.push(bridge_view_message(
                        event,
                        "assistant",
                        content,
                        json!({
                            "kind": "stream_delta",
                        }),
                        None,
                    ));
                }
            }
            RuntimeEventPayload::ToolCall(tool_call) => {
                messages.push(bridge_view_message(
                    event,
                    "tool",
                    format!("Tool requested: {}", tool_call.name),
                    json!({
                        "kind": "tool_call",
                        "tool_call_id": tool_call.tool_call_id,
                        "name": tool_call.name,
                        "workbench": tool_call.workbench,
                        "input": tool_call.input,
                    }),
                    None,
                ));
            }
            RuntimeEventPayload::ToolResult(tool_result) => {
                let mut metadata = json!({
                    "kind": "tool_result",
                    "tool_call_id": tool_result.tool_call_id,
                    "name": tool_result.name,
                    "workbench": tool_result.workbench,
                    "is_error": tool_result.is_error,
                });
                if let Some(error) = &tool_result.error {
                    metadata["error"] = error.clone();
                }
                if let Some(changed_files) = &tool_result.changed_files {
                    metadata["changed_files"] = changed_files.clone();
                }
                messages.push(bridge_view_message(
                    event,
                    "tool",
                    bridge_runtime_text(&tool_result.content),
                    metadata,
                    tool_result.changed_files.clone(),
                ));
            }
            RuntimeEventPayload::PermissionRequest(permission) => {
                messages.push(bridge_view_message(
                    event,
                    "system",
                    format!("Permission requested for {}.", permission.tool_name),
                    json!({
                        "kind": "permission_request",
                        "request_id": permission.request_id,
                        "tool_name": permission.tool_name,
                        "action": permission.action,
                        "reason": permission.reason,
                        "input": permission.input,
                    }),
                    None,
                ));
            }
            RuntimeEventPayload::SessionEvent(session_event) => {
                messages.push(bridge_view_message(
                    event,
                    "system",
                    session_event
                        .message
                        .clone()
                        .unwrap_or_else(|| format!("Session event: {}", session_event.subtype)),
                    json!({
                        "kind": "session_event",
                        "subtype": session_event.subtype,
                        "metadata": session_event.metadata,
                    }),
                    None,
                ));
            }
            RuntimeEventPayload::Error(error) => {
                messages.push(bridge_view_message(
                    event,
                    "system",
                    format!("Error: {}", error.message),
                    json!({
                        "kind": "error",
                        "code": error.code,
                        "details": error.details,
                    }),
                    None,
                ));
            }
            RuntimeEventPayload::Result(result) => {
                if let Some(text) = result
                    .assistant_text
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                    .filter(|text| {
                        !messages
                            .iter()
                            .rev()
                            .any(|message: &BridgeEventsViewMessage| {
                                message.role == "assistant" && message.content == *text
                            })
                    })
                {
                    messages.push(bridge_view_message(
                        event,
                        "assistant",
                        text.to_string(),
                        json!({
                            "kind": "result",
                            "status": result.status,
                            "stop_reason": result.stop_reason,
                        }),
                        None,
                    ));
                }
            }
        }
    }

    BridgeEventsViewReport {
        schema: "kiana.app-server.events-view.v1",
        session_id: session_id.to_string(),
        message_count: messages.len(),
        messages,
    }
}

fn bridge_message_view_messages(
    event: &RuntimeEvent,
    message: &Value,
    default_role: &str,
) -> Vec<BridgeEventsViewMessage> {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or(default_role);
    let role = match role {
        "assistant" => "assistant",
        "tool" => "tool",
        "user" => "user",
        _ => "system",
    };
    let content = message.get("content").unwrap_or(&Value::Null);
    let Some(blocks) = content.as_array() else {
        return vec![bridge_view_message(
            event,
            role,
            bridge_runtime_text(content),
            json!({
                "kind": "message",
            }),
            None,
        )];
    };

    let mut messages = Vec::new();
    let mut skipped_tool_blocks = false;
    for block in blocks {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("text");
        if matches!(block_type, "tool_use" | "tool_result") {
            skipped_tool_blocks = true;
            continue;
        }
        let content = bridge_content_block_text(block);
        if content.trim().is_empty() {
            continue;
        }
        messages.push(bridge_view_message(
            event,
            role,
            content,
            json!({
                "kind": "message_block",
                "block_type": block_type,
            }),
            None,
        ));
    }
    if messages.is_empty() {
        if skipped_tool_blocks {
            return messages;
        }
        messages.push(bridge_view_message(
            event,
            role,
            bridge_runtime_text(content),
            json!({
                "kind": "message",
            }),
            None,
        ));
    }
    messages
}

fn bridge_content_block_text(block: &Value) -> String {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => block
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => bridge_runtime_text(block),
    }
}

fn bridge_runtime_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(blocks) = value.as_array() {
        return blocks
            .iter()
            .map(bridge_content_block_text)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
    }
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn bridge_view_message(
    event: &RuntimeEvent,
    role: &str,
    content: String,
    metadata: Value,
    changed_files: Option<Value>,
) -> BridgeEventsViewMessage {
    BridgeEventsViewMessage {
        event_id: event.event_id.clone(),
        turn_id: event.turn_id.clone(),
        sequence: event.sequence,
        timestamp: event.timestamp.clone(),
        source_type: bridge_runtime_event_type(&event.payload),
        role: role.to_string(),
        content,
        changed_files,
        metadata,
    }
}

fn bridge_runtime_event_type(payload: &RuntimeEventPayload) -> &'static str {
    match payload {
        RuntimeEventPayload::UserMessage(_) => "user_message",
        RuntimeEventPayload::AssistantMessage(_) => "assistant_message",
        RuntimeEventPayload::StreamDelta(_) => "stream_delta",
        RuntimeEventPayload::ToolCall(_) => "tool_call",
        RuntimeEventPayload::ToolResult(_) => "tool_result",
        RuntimeEventPayload::PermissionRequest(_) => "permission_request",
        RuntimeEventPayload::SessionEvent(_) => "session_event",
        RuntimeEventPayload::Error(_) => "error",
        RuntimeEventPayload::Result(_) => "result",
    }
}

fn safe_relative_path(path: &str) -> Option<&Path> {
    let path = Path::new(path);
    if path.is_absolute() {
        return None;
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return None;
    }
    Some(path)
}

fn append_tool_events_from_message(
    events: &mut Vec<RuntimeEvent>,
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    timestamp: &str,
    start_sequence: u64,
    message: &Value,
) {
    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
        return;
    };
    let mut sequence = start_sequence;
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("tool_use") => {
                let Some(tool_call_id) = block.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(name) = block.get("name").and_then(Value::as_str) else {
                    continue;
                };
                events.push(runtime_event(
                    session_id,
                    turn_id,
                    parent_turn_id.clone(),
                    sequence,
                    timestamp,
                    RuntimeEventPayload::ToolCall(RuntimeToolCallEvent {
                        tool_call_id: tool_call_id.to_string(),
                        name: name.to_string(),
                        workbench: workbench_from_block(block),
                        input: block.get("input").cloned().unwrap_or_default(),
                    }),
                ));
                sequence += 1;
            }
            Some("tool_result") => {
                let Some(tool_call_id) = block.get("tool_use_id").and_then(Value::as_str) else {
                    continue;
                };
                events.push(runtime_event(
                    session_id,
                    turn_id,
                    parent_turn_id.clone(),
                    sequence,
                    timestamp,
                    RuntimeEventPayload::ToolResult(RuntimeToolResultEvent {
                        tool_call_id: tool_call_id.to_string(),
                        name: None,
                        workbench: workbench_from_block(block),
                        is_error: block
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        content: block.get("content").cloned().unwrap_or_default(),
                        changed_files: block.get("changed_files").cloned(),
                        error: block.get("error").cloned(),
                    }),
                ));
                sequence += 1;
            }
            _ => {}
        }
    }
}

fn workbench_from_block(block: &Value) -> Option<String> {
    block
        .get("workbench")
        .and_then(Value::as_str)
        .filter(|workbench| !workbench.trim().is_empty())
        .map(str::to_string)
}

fn content_to_message_value(role: &str, content: ContentBlock) -> Value {
    match content {
        ContentBlock::Text(text) => json!({ "role": role, "content": text }),
        ContentBlock::Blocks(blocks) => json!({ "role": role, "content": blocks }),
    }
}

fn runtime_event(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    payload: RuntimeEventPayload,
) -> RuntimeEvent {
    RuntimeEvent::new(
        format!("{session_id}:{turn_id}:{sequence}"),
        session_id,
        turn_id,
        parent_turn_id,
        sequence,
        timestamp,
        payload,
    )
}

#[derive(Debug, Serialize)]
pub struct BridgeSessionChangesReport {
    schema: &'static str,
    session_id: String,
    changed: bool,
    file_count: usize,
    change_count: usize,
    files: Vec<BridgeSessionChangedFile>,
}

#[derive(Debug, Serialize)]
pub struct BridgeSessionChangedFile {
    path: String,
    operations: BTreeSet<String>,
    sources: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
pub struct BridgeEventsViewReport {
    schema: &'static str,
    session_id: String,
    message_count: usize,
    messages: Vec<BridgeEventsViewMessage>,
}

#[derive(Debug, Serialize)]
pub struct BridgeEventsViewMessage {
    event_id: String,
    turn_id: String,
    sequence: u64,
    timestamp: String,
    source_type: &'static str,
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    changed_files: Option<Value>,
    metadata: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentBlock, MessageContent};
    use std::collections::HashMap;

    #[test]
    fn bridge_sdk_adapter_preserves_tool_result_changed_files() {
        let block = HashMap::from([
            ("type".to_string(), json!("tool_result")),
            ("tool_use_id".to_string(), json!("toolu_write")),
            (
                "content".to_string(),
                json!("The file src/lib.rs has been updated successfully."),
            ),
            ("is_error".to_string(), json!(false)),
            ("workbench".to_string(), json!("local")),
            (
                "changed_files".to_string(),
                json!([
                    {
                        "path": "src/lib.rs",
                        "operation": "update",
                        "source": "Write"
                    }
                ]),
            ),
        ]);

        let events = runtime_events_from_bridge_sdk_message(
            "session-1",
            "turn-1",
            None,
            0,
            "2026-07-04T00:00:00Z",
            SDKMessage::User {
                uuid: "user-1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Blocks(vec![block]),
                },
            },
        );

        assert_eq!(events.len(), 2);
        let event = serde_json::to_value(&events[1]).unwrap();
        assert_eq!(event["type"], "tool_result");
        assert_eq!(event["tool_call_id"], "toolu_write");
        assert_eq!(event["changed_files"][0]["path"], "src/lib.rs");
        assert_eq!(event["changed_files"][0]["operation"], "update");
        assert_eq!(event["changed_files"][0]["source"], "Write");
    }

    #[test]
    fn bridge_session_changes_report_aggregates_runtime_changed_files() {
        let first_block = HashMap::from([
            ("type".to_string(), json!("tool_result")),
            ("tool_use_id".to_string(), json!("toolu_write")),
            ("content".to_string(), json!("updated")),
            (
                "changed_files".to_string(),
                json!([
                    {
                        "path": "src/lib.rs",
                        "operation": "update",
                        "source": "Write"
                    },
                    {
                        "path": "../outside.rs",
                        "operation": "update",
                        "source": "Write"
                    }
                ]),
            ),
        ]);
        let second_block = HashMap::from([
            ("type".to_string(), json!("tool_result")),
            ("tool_use_id".to_string(), json!("toolu_edit")),
            ("content".to_string(), json!("edited")),
            (
                "changed_files".to_string(),
                json!([
                    {
                        "path": "src/lib.rs",
                        "operation": "edit",
                        "source": "Edit"
                    },
                    {
                        "path": "README.md",
                        "operation": "create",
                        "source": "Write"
                    }
                ]),
            ),
        ]);

        let mut events = runtime_events_from_bridge_sdk_message(
            "session-bridge",
            "turn-1",
            None,
            0,
            "2026-07-05T00:00:00Z",
            SDKMessage::User {
                uuid: "user-1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Blocks(vec![first_block]),
                },
            },
        );
        events.extend(runtime_events_from_bridge_sdk_message(
            "session-bridge",
            "turn-2",
            Some("turn-1".to_string()),
            10,
            "2026-07-05T00:00:01Z",
            SDKMessage::User {
                uuid: "user-2".to_string(),
                message: MessageContent {
                    content: ContentBlock::Blocks(vec![second_block]),
                },
            },
        ));

        let report = bridge_session_changes_report("session-bridge", &events);
        let value = serde_json::to_value(report).unwrap();

        assert_eq!(value["schema"], "kiana.diff.session_changes.v1");
        assert_eq!(value["session_id"], "session-bridge");
        assert_eq!(value["changed"], true);
        assert_eq!(value["file_count"], 2);
        assert_eq!(value["change_count"], 3);
        assert_eq!(value["files"][0]["path"], "README.md");
        assert_eq!(value["files"][0]["operations"][0], "create");
        assert_eq!(value["files"][0]["sources"][0], "Write");
        assert_eq!(value["files"][1]["path"], "src/lib.rs");
        assert_eq!(value["files"][1]["operations"], json!(["edit", "update"]));
        assert_eq!(value["files"][1]["sources"], json!(["Edit", "Write"]));
    }
}
