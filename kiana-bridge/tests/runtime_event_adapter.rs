use kiana_bridge::{types::ControlRequestType, ContentBlock, MessageContent, SDKMessage};
use serde_json::{json, Value};
use std::collections::HashMap;

#[test]
fn bridge_sdk_adapter_emits_runtime_events_for_messages_tools_and_results() {
    let events = kiana_bridge::runtime_events_from_bridge_sdk_message(
        "session-1",
        "turn-1",
        None,
        0,
        "2026-06-23T00:00:00Z",
        SDKMessage::Assistant {
            uuid: "assistant-1".to_string(),
            message: MessageContent {
                content: ContentBlock::Blocks(vec![
                    HashMap::from([
                        ("type".to_string(), json!("text")),
                        ("text".to_string(), json!("checking")),
                    ]),
                    HashMap::from([
                        ("type".to_string(), json!("tool_use")),
                        ("id".to_string(), json!("toolu_1")),
                        ("name".to_string(), json!("Read")),
                        ("input".to_string(), json!({"file_path": "README.md"})),
                        ("workbench".to_string(), json!("mcp")),
                    ]),
                ]),
            },
        },
    );

    assert_eq!(events.len(), 2);
    assert_eq!(
        serde_json::to_value(&events[0]).unwrap()["type"],
        "assistant_message"
    );
    assert_eq!(
        serde_json::to_value(&events[1]).unwrap()["type"],
        "tool_call"
    );
    assert_eq!(serde_json::to_value(&events[1]).unwrap()["name"], "Read");
    assert_eq!(
        serde_json::to_value(&events[1]).unwrap()["workbench"],
        "mcp"
    );

    let result_events = kiana_bridge::runtime_events_from_bridge_sdk_message(
        "session-1",
        "turn-2",
        Some("turn-1".to_string()),
        2,
        "2026-06-23T00:00:01Z",
        SDKMessage::User {
            uuid: "user-1".to_string(),
            message: MessageContent {
                content: ContentBlock::Blocks(vec![HashMap::from([
                    ("type".to_string(), json!("tool_result")),
                    ("tool_use_id".to_string(), json!("toolu_1")),
                    ("content".to_string(), json!("body")),
                    ("is_error".to_string(), json!(false)),
                    ("workbench".to_string(), json!("mcp")),
                ])]),
            },
        },
    );

    assert_eq!(
        serde_json::to_value(&result_events[1]).unwrap()["type"],
        "tool_result"
    );
    assert_eq!(
        serde_json::to_value(&result_events[1]).unwrap()["workbench"],
        "mcp"
    );
}

#[test]
fn bridge_sdk_adapter_emits_runtime_event_stream_delta_and_results() {
    let stream_events = kiana_bridge::runtime_events_from_bridge_sdk_message(
        "session-1",
        "turn-3",
        Some("turn-2".to_string()),
        4,
        "2026-06-23T00:00:02Z",
        SDKMessage::StreamEvent {
            data: HashMap::from([
                ("type".to_string(), json!("content_block_delta")),
                ("index".to_string(), json!(0)),
                (
                    "delta".to_string(),
                    json!({"type": "text_delta", "text": "partial"}),
                ),
            ]),
        },
    );

    assert_eq!(
        serde_json::to_value(&stream_events[0]).unwrap()["type"],
        "stream_delta"
    );
    assert_eq!(
        serde_json::to_value(&stream_events[0]).unwrap()["delta"]["delta"]["text"],
        "partial"
    );

    let result_events = kiana_bridge::runtime_events_from_bridge_sdk_message(
        "session-1",
        "turn-4",
        Some("turn-3".to_string()),
        5,
        "2026-06-23T00:00:03Z",
        SDKMessage::Result {
            data: HashMap::from([
                ("status".to_string(), json!("completed")),
                ("stop_reason".to_string(), json!("model_stop")),
                ("assistant_text".to_string(), json!("done")),
            ]),
        },
    );

    assert_eq!(
        serde_json::to_value(&result_events[0]).unwrap()["type"],
        "result"
    );
    assert_eq!(
        serde_json::to_value(&result_events[0]).unwrap()["assistant_text"],
        "done"
    );
    assert_eq!(
        serde_json::to_value(&result_events[0]).unwrap()["stop_reason"],
        "model_stop"
    );

    let error_events = kiana_bridge::runtime_events_from_bridge_sdk_message(
        "session-1",
        "turn-5",
        Some("turn-4".to_string()),
        6,
        "2026-06-23T00:00:04Z",
        SDKMessage::Result {
            data: HashMap::from([
                ("status".to_string(), json!("failed")),
                ("error".to_string(), json!("boom")),
            ]),
        },
    );

    assert_eq!(
        serde_json::to_value(&error_events[0]).unwrap()["type"],
        "error"
    );
    assert_eq!(
        serde_json::to_value(&error_events[0]).unwrap()["code"],
        "failed"
    );
    assert_eq!(
        serde_json::to_value(&error_events[0]).unwrap()["message"],
        "boom"
    );
}

#[test]
fn bridge_control_adapter_emits_runtime_event_permission_request() {
    let event = kiana_bridge::runtime_event_from_bridge_control_request(
        "session-1",
        "turn-3",
        Some("turn-2".to_string()),
        4,
        "2026-06-23T00:00:02Z",
        "perm-1",
        ControlRequestType::CanUseTool {
            tool_name: "Bash".to_string(),
            input: HashMap::from([("command".to_string(), Value::String("pwd".to_string()))]),
            tool_use_id: "toolu_bash".to_string(),
        },
    )
    .expect("permission event");
    let value = serde_json::to_value(event).unwrap();

    assert_eq!(value["type"], "permission_request");
    assert_eq!(value["request_id"], "perm-1");
    assert_eq!(value["tool_name"], "Bash");
    assert_eq!(value["input"]["command"], "pwd");
}
