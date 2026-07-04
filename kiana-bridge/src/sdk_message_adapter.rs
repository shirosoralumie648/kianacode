use crate::types::{ContentBlock, ControlRequestType, SDKMessage};
use kiana_types::{
    MessageRuntimeEvent, RuntimeErrorEvent, RuntimeEvent, RuntimeEventPayload,
    RuntimePermissionRequestEvent, RuntimeResultEvent, RuntimeSessionEvent,
    RuntimeStreamDeltaEvent, RuntimeToolCallEvent, RuntimeToolResultEvent,
};
use serde_json::{json, Value};

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
