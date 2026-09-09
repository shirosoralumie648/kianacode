use kiana_domain::{ConversationMessage, ConversationRole, ExecutionStatus, RequestId, RoleSpec};
use kiana_protocol::{
    RequestBody, RequestEnvelope, RequestMetadata, ResponseEnvelope, RunStreamEnvelope,
    RunStreamEvent, SymposiumRequest, DEPARTMENT_EXECUTING, PROTOCOL_SCHEMA, ROLE_BUILDER,
};
use serde_json::json;

#[test]
fn metadata_defaults_and_role_assignment_survive_wire_round_trip() {
    // 旧客户端可以省略角色字段；反序列化后必须回到执行部门的默认 Builder。
    let request_id = RequestId::new();
    let raw = json!({
        "schema": PROTOCOL_SCHEMA,
        "metadata": {
            "request_id": request_id,
            "session_id": "session-1",
            "project_root": "/repo",
            "actor_id": "local-user",
            "project_trusted": true,
            "permission_profile": "balanced"
        },
        "body": { "type": "run", "request": { "prompt": "hello" } }
    });
    let decoded: RequestEnvelope = serde_json::from_value(raw).unwrap();
    assert_eq!(decoded.metadata.role_id, ROLE_BUILDER);
    assert_eq!(decoded.metadata.department_id, DEPARTMENT_EXECUTING);

    let mut metadata = RequestMetadata::local("session-2", "/repo");
    metadata.assign_role(&RoleSpec::pm());
    metadata.path_allow = vec!["plan/WORK.md".to_owned()];
    let envelope = RequestEnvelope::run_with_history(
        metadata,
        "continue",
        vec![ConversationMessage {
            role: ConversationRole::Assistant,
            text: "ready".to_owned(),
            tool_call_id: None,
        }],
        Some("workspace-write".to_owned()),
    );
    let encoded = serde_json::to_value(&envelope).unwrap();
    let round_trip: RequestEnvelope = serde_json::from_value(encoded).unwrap();
    assert_eq!(round_trip, envelope);
}

#[test]
fn symposium_defaults_and_rejected_response_are_stable() {
    // Symposium 的缺省轮数和拒绝响应的状态、空输出必须保持稳定，便于入口统一处理。
    let metadata = RequestMetadata::local("session-1", "/repo");
    let request = RequestEnvelope::symposium(metadata, "decide", false, 3, None);
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["body"]["type"], "symposium");
    assert_eq!(encoded["body"]["request"]["max_rounds"], 3);
    let parsed: SymposiumRequest = serde_json::from_value(json!({
        "goal": "decide"
    }))
    .unwrap();
    assert_eq!(
        parsed.max_rounds,
        kiana_domain::Symposium::DEFAULT_MAX_ROUNDS
    );

    let response = ResponseEnvelope::rejected(RequestId::new(), "project_untrusted");
    assert_eq!(response.schema, PROTOCOL_SCHEMA);
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.output, serde_json::Value::Null);
    assert_eq!(response.error.as_deref(), Some("project_untrusted"));
    assert!(matches!(request.body, RequestBody::Symposium(_)));
}

#[test]
fn run_stream_events_are_additive_and_unknown_events_are_ignored() {
    let run_id = kiana_domain::RunId::new();
    let envelope = RunStreamEnvelope::new(RunStreamEvent::Delta {
        run_id,
        text: "alpha".to_owned(),
    });
    let encoded = serde_json::to_value(&envelope).unwrap();
    assert_eq!(encoded["schema"], PROTOCOL_SCHEMA);
    assert_eq!(encoded["event"]["type"], "delta");
    assert_eq!(encoded["event"]["run_id"], json!(run_id));
    assert_eq!(encoded["event"]["text"], "alpha");

    let decoded: RunStreamEnvelope = serde_json::from_value(json!({
        "schema": PROTOCOL_SCHEMA,
        "event": {
            "type": "future_usage",
            "run_id": run_id,
            "input_tokens": 42
        }
    }))
    .unwrap();
    assert_eq!(decoded.schema, PROTOCOL_SCHEMA);
    assert_eq!(decoded.event, RunStreamEvent::Unknown);

    // 旧客户端仍只读取 output；新增字段不能改变原有 ResponseEnvelope 的语义。
    let response: ResponseEnvelope = serde_json::from_value(json!({
        "schema": PROTOCOL_SCHEMA,
        "request_id": RequestId::new(),
        "status": "completed",
        "output": { "text": "legacy output" },
        "error": null,
        "future_field": { "ignored": true }
    }))
    .unwrap();
    assert_eq!(response.output["text"], "legacy output");
    assert_eq!(response.status, ExecutionStatus::Completed);
}
