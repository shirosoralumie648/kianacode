use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEvent {
    pub event_id: String,
    pub session_id: String,
    pub turn_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_turn_id: Option<String>,
    pub sequence: u64,
    pub timestamp: String,
    #[serde(flatten)]
    pub payload: RuntimeEventPayload,
}

impl RuntimeEvent {
    pub fn new(
        event_id: impl Into<String>,
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        parent_turn_id: Option<String>,
        sequence: u64,
        timestamp: impl Into<String>,
        payload: RuntimeEventPayload,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            parent_turn_id,
            sequence,
            timestamp: timestamp.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEventPayload {
    UserMessage(MessageRuntimeEvent),
    AssistantMessage(MessageRuntimeEvent),
    StreamDelta(RuntimeStreamDeltaEvent),
    ToolCall(RuntimeToolCallEvent),
    ToolResult(RuntimeToolResultEvent),
    PermissionRequest(RuntimePermissionRequestEvent),
    SessionEvent(RuntimeSessionEvent),
    Error(RuntimeErrorEvent),
    Result(RuntimeResultEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageRuntimeEvent {
    pub message: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeStreamDeltaEvent {
    pub delta: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeToolCallEvent {
    pub tool_call_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workbench: Option<String>,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeToolResultEvent {
    pub tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workbench: Option<String>,
    pub is_error: bool,
    pub content: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_files: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimePermissionRequestEvent {
    pub request_id: String,
    pub tool_name: String,
    pub action: String,
    pub input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeSessionEvent {
    pub subtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeErrorEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeResultEvent {
    pub status: String,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_text: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

pub fn sdk_message_to_runtime_event(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    message: Value,
) -> RuntimeEvent {
    let payload = match message.get("role").and_then(Value::as_str) {
        Some("user") => RuntimeEventPayload::UserMessage(MessageRuntimeEvent { message }),
        Some("assistant") => RuntimeEventPayload::AssistantMessage(MessageRuntimeEvent { message }),
        role => RuntimeEventPayload::SessionEvent(RuntimeSessionEvent {
            subtype: "sdk_message".to_string(),
            message: Some("SDK message role is not user or assistant".to_string()),
            metadata: json!({
                "role": role.unwrap_or("unknown"),
                "message": message
            }),
        }),
    };
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
