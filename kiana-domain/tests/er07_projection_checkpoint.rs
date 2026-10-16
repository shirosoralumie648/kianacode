//! ER-07 Domain projection checkpoint 合同测试。
//!
//! 这些测试只构造内存 JSON state、合成事件 ID 和 checkpoint，不连接 EventStore 或 ProjectionStore。
//! 它们检查 DTO 的 schema/版本、Serde 未知字段策略、摘要绑定、事件身份列表边界，以及空 cursor 锚点可用性。

use kiana_domain::{
    EventId, ProjectionCheckpoint, RequestId, RunId, RuntimeEvent, SchemaVersion,
    PROJECTION_CHECKPOINT_SCHEMA, PROJECTION_CHECKPOINT_VERSION,
};
use serde_json::json;

/// 生成带 delta 载荷的合成事件，供调用者测试 fold/replay 使用。
fn event(request_id: RequestId, sequence: u64, delta: i64) -> RuntimeEvent {
    RuntimeEvent::new(
        request_id,
        sequence,
        "projection.delta",
        json!({"delta":delta}),
    )
    .unwrap()
}

/// 检查 checkpoint 的固定 schema、JSON 往返、未知字段拒绝、state 摘要保护和不兼容版本拒绝。
#[test]
fn projection_checkpoint_is_strict_and_digest_bound() {
    let event_id = EventId::new();
    let checkpoint =
        ProjectionCheckpoint::new("counter", 4, vec![event_id], json!({"count":4})).unwrap();
    assert_eq!(checkpoint.schema, PROJECTION_CHECKPOINT_SCHEMA);
    assert_eq!(checkpoint.version, PROJECTION_CHECKPOINT_VERSION);
    assert!(checkpoint.validate().is_ok());
    assert_eq!(
        ProjectionCheckpoint::from_json(&checkpoint.to_json().unwrap()).unwrap(),
        checkpoint
    );

    let mut unknown = checkpoint.to_json().unwrap();
    unknown["secret_state"] = json!("must-not-be-accepted");
    assert_eq!(
        ProjectionCheckpoint::from_json(&unknown).unwrap_err(),
        "projection_checkpoint_decode_failed"
    );

    let mut tampered = checkpoint.clone();
    tampered.state["count"] = json!(5);
    assert_eq!(
        tampered.validate().unwrap_err(),
        "projection_checkpoint_state_digest_mismatch"
    );

    let mut wrong_version = checkpoint;
    wrong_version.version = SchemaVersion::new(2, 0);
    assert_eq!(
        wrong_version.validate().unwrap_err(),
        "projection_checkpoint_header_invalid"
    );
}

/// 确认反序列化/直接字段修改得到的重复 EventId 列表不能靠重算摘要绕过结构校验。
#[test]
fn projection_checkpoint_rejects_invalid_source_identity() {
    let run_id = RunId::new();
    let mut checkpoint =
        ProjectionCheckpoint::new("run", 1, Vec::new(), json!({"run":run_id})).unwrap();
    let duplicate = EventId::new();
    checkpoint.source_event_ids = vec![duplicate, duplicate];
    checkpoint.checkpoint_digest = checkpoint.digest();
    assert_eq!(
        checkpoint.validate().unwrap_err(),
        "projection_checkpoint_header_invalid"
    );
}

/// 固定一个合法边界：空 state 可以从 EventLog 游标 0 开始，表示全量重建前的空锚点。
#[test]
fn projection_checkpoint_allows_empty_rebuild_anchor() {
    let checkpoint = ProjectionCheckpoint::new("empty", 0, Vec::new(), json!({})).unwrap();
    assert_eq!(checkpoint.source_cursor, 0);
    assert!(checkpoint.source_event_ids.is_empty());
    assert!(checkpoint.validate().is_ok());
}
