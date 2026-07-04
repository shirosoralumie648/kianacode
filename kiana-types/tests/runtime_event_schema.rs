use kiana_types::{
    sdk_message_to_runtime_event, MessageRuntimeEvent, RuntimeErrorEvent, RuntimeEvent,
    RuntimeEventPayload, RuntimePermissionRequestEvent, RuntimeResultEvent, RuntimeSessionEvent,
    RuntimeStreamDeltaEvent, RuntimeToolCallEvent, RuntimeToolResultEvent,
};
use serde_json::json;

#[test]
fn runtime_event_serializes_user_message_with_parent_turn() {
    let event = RuntimeEvent::new(
        "evt-1",
        "session-1",
        "turn-2",
        Some("turn-1".to_string()),
        7,
        "2026-06-23T00:00:00Z",
        RuntimeEventPayload::UserMessage(MessageRuntimeEvent {
            message: json!({"role": "user", "content": "hello"}),
        }),
    );

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        json!({
            "event_id": "evt-1",
            "session_id": "session-1",
            "turn_id": "turn-2",
            "parent_turn_id": "turn-1",
            "sequence": 7,
            "timestamp": "2026-06-23T00:00:00Z",
            "type": "user_message",
            "message": {"role": "user", "content": "hello"}
        })
    );
}

#[test]
fn runtime_event_schema_covers_required_payloads() {
    let payloads = vec![
        RuntimeEventPayload::AssistantMessage(MessageRuntimeEvent {
            message: json!({"role": "assistant", "content": [{"type": "text", "text": "hi"}]}),
        }),
        RuntimeEventPayload::StreamDelta(RuntimeStreamDeltaEvent {
            delta: json!({"type": "text_delta", "text": "hi"}),
        }),
        RuntimeEventPayload::ToolCall(RuntimeToolCallEvent {
            tool_call_id: "toolu_1".to_string(),
            name: "MCP".to_string(),
            workbench: Some("mcp".to_string()),
            input: json!({"file_path": "README.md"}),
        }),
        RuntimeEventPayload::ToolResult(RuntimeToolResultEvent {
            tool_call_id: "toolu_1".to_string(),
            name: Some("MCP".to_string()),
            workbench: Some("mcp".to_string()),
            is_error: false,
            content: json!({"text": "content"}),
            error: Some(json!({
                "type": "tool_error",
                "code": "tool_validation_error",
                "message": "path is required",
                "repair_hint": "Provide the required input fields for Read and retry the tool call."
            })),
        }),
        RuntimeEventPayload::PermissionRequest(RuntimePermissionRequestEvent {
            request_id: "perm-1".to_string(),
            tool_name: "Bash".to_string(),
            action: "run".to_string(),
            input: json!({"command": "cargo test"}),
            reason: Some("command requires approval".to_string()),
        }),
        RuntimeEventPayload::SessionEvent(RuntimeSessionEvent {
            subtype: "created".to_string(),
            message: Some("session created".to_string()),
            metadata: json!({"cwd": "/work"}),
        }),
        RuntimeEventPayload::Error(RuntimeErrorEvent {
            code: Some("api_error".to_string()),
            message: "request failed".to_string(),
            details: json!({"status": 500}),
        }),
        RuntimeEventPayload::Result(RuntimeResultEvent {
            status: "completed".to_string(),
            assistant_text: Some("done".to_string()),
            metadata: json!({"iterations": 2}),
        }),
    ];

    let types: Vec<_> = payloads
        .into_iter()
        .map(|payload| {
            let event = RuntimeEvent::new(
                "evt",
                "session",
                "turn",
                None,
                1,
                "2026-06-23T00:00:00Z",
                payload,
            );
            serde_json::to_value(event).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();

    assert_eq!(
        types,
        vec![
            "assistant_message",
            "stream_delta",
            "tool_call",
            "tool_result",
            "permission_request",
            "session_event",
            "error",
            "result",
        ]
    );
}

#[test]
fn sdk_message_adapter_maps_roles_to_runtime_events() {
    let user = sdk_message_to_runtime_event(
        "session-1",
        "turn-1",
        None,
        0,
        "2026-06-23T00:00:00Z",
        json!({"role": "user", "content": "question"}),
    );
    let assistant = sdk_message_to_runtime_event(
        "session-1",
        "turn-2",
        Some("turn-1".to_string()),
        1,
        "2026-06-23T00:00:01Z",
        json!({"role": "assistant", "content": [{"type": "text", "text": "answer"}]}),
    );

    assert_eq!(serde_json::to_value(&user).unwrap()["type"], "user_message");
    assert_eq!(
        serde_json::to_value(&assistant).unwrap()["type"],
        "assistant_message"
    );
    assert_eq!(assistant.parent_turn_id.as_deref(), Some("turn-1"));
}
