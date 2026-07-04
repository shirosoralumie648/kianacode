use crate::types::*;
use kiana_types::{
    MessageRuntimeEvent, RuntimeErrorEvent, RuntimeEvent, RuntimeEventPayload,
    RuntimePermissionRequestEvent, RuntimeResultEvent, RuntimeSessionEvent,
    RuntimeStreamDeltaEvent, RuntimeToolCallEvent, RuntimeToolResultEvent,
};
use serde_json::json;

#[derive(Debug, Clone)]
pub enum ConvertedMessage {
    Message(serde_json::Value),
    StreamEvent(serde_json::Value),
    Ignored,
}

#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    pub convert_tool_results: bool,
    pub convert_user_text_messages: bool,
}

pub fn convert_sdk_message(msg: SDKMessage, opts: &ConvertOptions) -> ConvertedMessage {
    match msg {
        SDKMessage::Assistant(assistant_msg) => ConvertedMessage::Message(json!({
            "type": "assistant",
            "message": assistant_msg.message,
            "uuid": assistant_msg.uuid,
            "requestId": null,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "error": assistant_msg.error
        })),
        SDKMessage::User(user_msg) => {
            if opts.convert_tool_results || opts.convert_user_text_messages {
                ConvertedMessage::Message(json!({
                    "type": "user",
                    "content": user_msg.message,
                    "uuid": user_msg.uuid,
                    "timestamp": user_msg.timestamp,
                    "toolUseResult": user_msg.tool_use_result
                }))
            } else {
                ConvertedMessage::Ignored
            }
        }
        SDKMessage::StreamEvent(stream_msg) => ConvertedMessage::StreamEvent(stream_msg.event),
        SDKMessage::Result(result_msg) => {
            if result_msg.subtype != "success" {
                let content = result_msg
                    .errors
                    .map(|e| e.join(", "))
                    .unwrap_or_else(|| "Unknown error".to_string());

                ConvertedMessage::Message(json!({
                    "type": "system",
                    "subtype": "informational",
                    "content": content,
                    "level": "warning",
                    "uuid": result_msg.uuid,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            } else {
                ConvertedMessage::Ignored
            }
        }
        SDKMessage::System(system_msg) => match system_msg.subtype.as_str() {
            "init" => {
                let model = system_msg.model.unwrap_or_default();
                ConvertedMessage::Message(json!({
                    "type": "system",
                    "subtype": "informational",
                    "content": format!("Remote session initialized (model: {})", model),
                    "level": "info",
                    "uuid": system_msg.uuid,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            "status" => {
                if let Some(status) = system_msg.status {
                    let content = if status == "compacting" {
                        "Compacting conversation…".to_string()
                    } else {
                        format!("Status: {}", status)
                    };
                    ConvertedMessage::Message(json!({
                        "type": "system",
                        "subtype": "informational",
                        "content": content,
                        "level": "info",
                        "uuid": system_msg.uuid,
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }))
                } else {
                    ConvertedMessage::Ignored
                }
            }
            "compact_boundary" => ConvertedMessage::Message(json!({
                "type": "system",
                "subtype": "compact_boundary",
                "content": "Conversation compacted",
                "level": "info",
                "uuid": system_msg.uuid,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "compactMetadata": system_msg.compact_metadata
            })),
            _ => ConvertedMessage::Ignored,
        },
        SDKMessage::ToolProgress(progress_msg) => ConvertedMessage::Message(json!({
            "type": "system",
            "subtype": "informational",
            "content": format!(
                "Tool {} running for {}s…",
                progress_msg.tool_name, progress_msg.elapsed_time_seconds
            ),
            "level": "info",
            "uuid": progress_msg.uuid,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "toolUseID": progress_msg.tool_use_id
        })),
        SDKMessage::AuthStatus | SDKMessage::ToolUseSummary | SDKMessage::RateLimitEvent => {
            ConvertedMessage::Ignored
        }
    }
}

pub fn is_session_end_message(msg: &SDKMessage) -> bool {
    matches!(msg, SDKMessage::Result(_))
}

pub fn is_success_result(msg: &SDKResultMessage) -> bool {
    msg.subtype == "success"
}

pub fn get_result_text(msg: &SDKResultMessage) -> Option<String> {
    if msg.subtype == "success" {
        msg.result.clone()
    } else {
        None
    }
}

pub fn runtime_events_from_sdk_message(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    start_sequence: u64,
    timestamp: &str,
    msg: SDKMessage,
) -> Vec<RuntimeEvent> {
    match msg {
        SDKMessage::Assistant(assistant_msg) => {
            let mut events = vec![runtime_event(
                session_id,
                turn_id,
                parent_turn_id.clone(),
                start_sequence,
                timestamp,
                RuntimeEventPayload::AssistantMessage(MessageRuntimeEvent {
                    message: assistant_msg.message.clone(),
                }),
            )];
            append_tool_events_from_message(
                &mut events,
                session_id,
                turn_id,
                parent_turn_id,
                timestamp,
                start_sequence + 1,
                &assistant_msg.message,
            );
            events
        }
        SDKMessage::User(user_msg) => {
            let message = user_msg.message.unwrap_or_else(|| {
                json!({
                    "role": "user",
                    "content": user_msg.tool_use_result.unwrap_or_default()
                })
            });
            let mut events = vec![runtime_event(
                session_id,
                turn_id,
                parent_turn_id.clone(),
                start_sequence,
                timestamp,
                RuntimeEventPayload::UserMessage(MessageRuntimeEvent {
                    message: message.clone(),
                }),
            )];
            append_tool_events_from_message(
                &mut events,
                session_id,
                turn_id,
                parent_turn_id,
                timestamp,
                start_sequence + 1,
                &message,
            );
            events
        }
        SDKMessage::StreamEvent(stream_msg) => vec![runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            RuntimeEventPayload::StreamDelta(RuntimeStreamDeltaEvent {
                delta: stream_msg.event,
            }),
        )],
        SDKMessage::Result(result_msg) if result_msg.subtype == "success" => vec![runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            RuntimeEventPayload::Result(RuntimeResultEvent {
                status: result_msg.subtype,
                stop_reason: result_msg
                    .stop_reason
                    .unwrap_or_else(|| "model_stop".to_string()),
                assistant_text: result_msg.result,
                metadata: json!({ "uuid": result_msg.uuid }),
            }),
        )],
        SDKMessage::Result(result_msg) => vec![runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            RuntimeEventPayload::Error(RuntimeErrorEvent {
                code: Some(result_msg.subtype),
                message: result_msg
                    .errors
                    .unwrap_or_default()
                    .join(", ")
                    .trim()
                    .to_string()
                    .if_empty("Unknown SDK result error"),
                details: json!({ "uuid": result_msg.uuid }),
            }),
        )],
        SDKMessage::System(system_msg) => vec![runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            RuntimeEventPayload::SessionEvent(RuntimeSessionEvent {
                subtype: system_msg.subtype,
                message: system_msg.status,
                metadata: json!({
                    "uuid": system_msg.uuid,
                    "model": system_msg.model,
                    "compact_metadata": system_msg.compact_metadata
                }),
            }),
        )],
        SDKMessage::ToolProgress(progress_msg) => vec![runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            RuntimeEventPayload::SessionEvent(RuntimeSessionEvent {
                subtype: "tool_progress".to_string(),
                message: Some(format!(
                    "Tool {} running for {}s",
                    progress_msg.tool_name, progress_msg.elapsed_time_seconds
                )),
                metadata: json!({
                    "uuid": progress_msg.uuid,
                    "tool_use_id": progress_msg.tool_use_id,
                    "tool_name": progress_msg.tool_name,
                    "elapsed_time_seconds": progress_msg.elapsed_time_seconds
                }),
            }),
        )],
        SDKMessage::AuthStatus | SDKMessage::ToolUseSummary | SDKMessage::RateLimitEvent => {
            Vec::new()
        }
    }
}

pub fn runtime_event_from_control_request(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    request: SDKControlRequest,
) -> Option<RuntimeEvent> {
    match request.request {
        SDKControlRequestInner::CanUseTool(permission) => Some(runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            sequence,
            timestamp,
            RuntimeEventPayload::PermissionRequest(RuntimePermissionRequestEvent {
                request_id: request.request_id,
                tool_name: permission.tool_name,
                action: "can_use_tool".to_string(),
                input: json!(permission.input),
                reason: permission_reason(
                    permission.blocked_path,
                    permission.decision_reason,
                    permission.agent_id,
                ),
            }),
        )),
        _ => None,
    }
}

fn append_tool_events_from_message(
    events: &mut Vec<RuntimeEvent>,
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    timestamp: &str,
    start_sequence: u64,
    message: &serde_json::Value,
) {
    let Some(blocks) = message.get("content").and_then(serde_json::Value::as_array) else {
        return;
    };
    let mut sequence = start_sequence;
    for block in blocks {
        match block.get("type").and_then(serde_json::Value::as_str) {
            Some("tool_use") => {
                let Some(tool_call_id) = block.get("id").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let Some(name) = block.get("name").and_then(serde_json::Value::as_str) else {
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
                let Some(tool_call_id) =
                    block.get("tool_use_id").and_then(serde_json::Value::as_str)
                else {
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
                            .and_then(serde_json::Value::as_bool)
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

fn workbench_from_block(block: &serde_json::Value) -> Option<String> {
    block
        .get("workbench")
        .and_then(serde_json::Value::as_str)
        .filter(|workbench| !workbench.trim().is_empty())
        .map(str::to_string)
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

fn permission_reason(
    blocked_path: Option<String>,
    decision_reason: Option<serde_json::Value>,
    agent_id: Option<String>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(value) = decision_reason {
        parts.push(format!("decision_reason={value}"));
    }
    if let Some(value) = blocked_path {
        parts.push(format!("blocked_path={value}"));
    }
    if let Some(value) = agent_id {
        parts.push(format!("agent_id={value}"));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

trait EmptyStringDefault {
    fn if_empty(self, fallback: &str) -> String;
}

impl EmptyStringDefault for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod runtime_event_tests {
    use super::*;
    use crate::{
        SDKAssistantMessage, SDKControlPermissionRequest, SDKControlRequest,
        SDKControlRequestInner, SDKResultMessage, SDKUserMessage,
    };
    use serde_json::{json, Value};
    use std::collections::HashMap;

    #[test]
    fn remote_sdk_adapter_emits_runtime_events_for_messages_tools_and_results() {
        let assistant_events = runtime_events_from_sdk_message(
            "session-1",
            "turn-1",
            None,
            0,
            "2026-06-23T00:00:00Z",
            SDKMessage::Assistant(SDKAssistantMessage {
                uuid: "assistant-1".to_string(),
                error: None,
                message: json!({
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "I will read it"},
                        {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {"file_path": "README.md"}, "workbench": "mcp"}
                    ]
                }),
            }),
        );
        assert_eq!(assistant_events.len(), 2);
        assert_eq!(
            serde_json::to_value(&assistant_events[0]).unwrap()["type"],
            "assistant_message"
        );
        assert_eq!(
            serde_json::to_value(&assistant_events[1]).unwrap()["type"],
            "tool_call"
        );
        assert_eq!(
            serde_json::to_value(&assistant_events[1]).unwrap()["name"],
            "Read"
        );
        assert_eq!(
            serde_json::to_value(&assistant_events[1]).unwrap()["workbench"],
            "mcp"
        );

        let user_events = runtime_events_from_sdk_message(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            2,
            "2026-06-23T00:00:01Z",
            SDKMessage::User(SDKUserMessage {
                uuid: "user-1".to_string(),
                timestamp: Some("2026-06-23T00:00:01Z".to_string()),
                message: Some(json!({
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_1",
                            "content": "file body",
                            "is_error": true,
                            "workbench": "mcp",
                            "changed_files": [
                                {
                                    "path": "src/lib.rs",
                                    "operation": "update",
                                    "source": "Write"
                                }
                            ],
                            "error": {
                                "type": "tool_error",
                                "code": "tool_validation_error",
                                "message": "path is required",
                                "repair_hint": "Provide the required input fields for Read and retry the tool call."
                            }
                        }
                    ]
                })),
                tool_use_result: None,
            }),
        );
        assert_eq!(user_events.len(), 2);
        assert_eq!(
            serde_json::to_value(&user_events[0]).unwrap()["type"],
            "user_message"
        );
        assert_eq!(
            serde_json::to_value(&user_events[1]).unwrap()["type"],
            "tool_result"
        );
        assert_eq!(
            serde_json::to_value(&user_events[1]).unwrap()["tool_call_id"],
            "toolu_1"
        );
        assert_eq!(
            serde_json::to_value(&user_events[1]).unwrap()["workbench"],
            "mcp"
        );
        assert_eq!(
            serde_json::to_value(&user_events[1]).unwrap()["is_error"],
            true
        );
        assert_eq!(
            serde_json::to_value(&user_events[1]).unwrap()["changed_files"][0]["path"],
            "src/lib.rs"
        );
        assert_eq!(
            serde_json::to_value(&user_events[1]).unwrap()["changed_files"][0]["operation"],
            "update"
        );
        assert_eq!(
            serde_json::to_value(&user_events[1]).unwrap()["error"]["code"],
            "tool_validation_error"
        );
        assert_eq!(
            serde_json::to_value(&user_events[1]).unwrap()["error"]["repair_hint"],
            "Provide the required input fields for Read and retry the tool call."
        );

        let stream_events = runtime_events_from_sdk_message(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            4,
            "2026-06-23T00:00:02Z",
            SDKMessage::StreamEvent(crate::SDKPartialAssistantMessage {
                event: json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "hi"}}),
            }),
        );
        assert_eq!(
            serde_json::to_value(&stream_events[0]).unwrap()["type"],
            "stream_delta"
        );

        let result_events = runtime_events_from_sdk_message(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            5,
            "2026-06-23T00:00:03Z",
            SDKMessage::Result(SDKResultMessage {
                subtype: "success".to_string(),
                uuid: "result-1".to_string(),
                stop_reason: Some("max_turns".to_string()),
                errors: None,
                result: Some("done".to_string()),
            }),
        );
        assert_eq!(
            serde_json::to_value(&result_events[0]).unwrap()["type"],
            "result"
        );
        assert_eq!(
            serde_json::to_value(&result_events[0]).unwrap()["stop_reason"],
            "max_turns"
        );

        let error_events = runtime_events_from_sdk_message(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            6,
            "2026-06-23T00:00:04Z",
            SDKMessage::Result(SDKResultMessage {
                subtype: "error".to_string(),
                uuid: "result-2".to_string(),
                stop_reason: None,
                errors: Some(vec!["boom".to_string()]),
                result: None,
            }),
        );
        assert_eq!(
            serde_json::to_value(&error_events[0]).unwrap()["type"],
            "error"
        );
    }

    #[test]
    fn remote_control_adapter_emits_runtime_permission_request() {
        let request = SDKControlRequest {
            request_id: "perm-1".to_string(),
            request: SDKControlRequestInner::CanUseTool(SDKControlPermissionRequest {
                subtype: "can_use_tool".to_string(),
                tool_name: "Bash".to_string(),
                tool_use_id: "toolu_bash".to_string(),
                input: HashMap::from([("command".to_string(), Value::String("pwd".to_string()))]),
                permission_suggestions: None,
                blocked_path: Some("/tmp/work".to_string()),
                decision_reason: Some(json!("ask mode")),
                agent_id: Some("agent-1".to_string()),
            }),
        };

        let event = runtime_event_from_control_request(
            "session-1",
            "turn-3",
            Some("turn-2".to_string()),
            7,
            "2026-06-23T00:00:05Z",
            request,
        )
        .expect("permission event");
        let value = serde_json::to_value(event).unwrap();

        assert_eq!(value["type"], "permission_request");
        assert_eq!(value["request_id"], "perm-1");
        assert_eq!(value["tool_name"], "Bash");
        assert_eq!(value["input"]["command"], "pwd");
        assert!(value["reason"].as_str().unwrap().contains("ask mode"));
    }
}
