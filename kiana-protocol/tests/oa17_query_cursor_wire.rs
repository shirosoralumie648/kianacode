//! OA-17 wire fixtures for the eventlog cursor snapshot and bounded query scan contracts.
//!
//! The response fields are additive: clients that predate them must still parse a response,
//! while forged snapshots or inconsistent scan counters fail closed instead of being ignored.

use kiana_domain::{AuditQueryScan, EventLogCursorSnapshot};
use kiana_protocol::{AuditQueryResponse, AUDIT_QUERY_SCHEMA};

fn response() -> AuditQueryResponse {
    AuditQueryResponse {
        schema: AUDIT_QUERY_SCHEMA.to_owned(),
        records: Vec::new(),
        next_cursor: None,
        source_cursor: 3,
        projection_version: 1,
        snapshot: Some(EventLogCursorSnapshot::new(1, 3, 3).unwrap()),
        scan: Some(AuditQueryScan::new(3, 3, 2, 2, 1).unwrap()),
        limitations: vec!["eventlog_cursor_snapshot_unsupported".to_owned()],
    }
}

#[test]
fn audit_query_response_carries_snapshot_and_scan_additively() {
    let response = response();
    let encoded = serde_json::to_value(&response).unwrap();
    assert!(encoded["snapshot"]["snapshot_digest"].is_string());
    assert_eq!(encoded["snapshot"]["last_committed_cursor"], 3);
    assert_eq!(encoded["scan"]["scan_class"], "bounded");
    assert_eq!(encoded["scan"]["page_records"], 1);
    assert_eq!(
        serde_json::from_value::<AuditQueryResponse>(encoded.clone()).unwrap(),
        response
    );

    // 旧客户端响应（没有 snapshot/scan 字段）仍必须能解析：新增字段是 additive。
    let mut legacy = encoded;
    let object = legacy.as_object_mut().unwrap();
    object.remove("snapshot");
    object.remove("scan");
    let parsed = serde_json::from_value::<AuditQueryResponse>(legacy.clone()).unwrap();
    assert!(parsed.snapshot.is_none());
    assert!(parsed.scan.is_none());
}

#[test]
fn eventlog_snapshot_and_scan_reject_forged_or_inconsistent_shapes() {
    // scan 计数必须满足 page <= matched <= scoped <= projected <= scanned。
    assert_eq!(
        AuditQueryScan::new(3, 3, 1, 2, 1).unwrap_err(),
        "audit_query_scan_counts_inconsistent"
    );

    // 分类必须与扫描量一致，不能手写一个"小扫描量 + heavy"。
    let mut forged = serde_json::to_value(AuditQueryScan::new(1, 1, 1, 1, 1).unwrap()).unwrap();
    forged["scan_class"] = serde_json::json!("heavy");
    let forged: AuditQueryScan = serde_json::from_value(forged).unwrap();
    assert_eq!(
        forged.validate().unwrap_err(),
        "audit_query_scan_class_inconsistent"
    );

    // 未知字段必须被拒绝，而不是被静默忽略。
    let mut unknown = serde_json::to_value(EventLogCursorSnapshot::new(1, 1, 0).unwrap()).unwrap();
    unknown["owner"] = serde_json::json!("foreign");
    assert!(serde_json::from_value::<EventLogCursorSnapshot>(unknown).is_err());

    // 摘要被改写后必须 fail closed。
    let mut bad_digest =
        serde_json::to_value(EventLogCursorSnapshot::new(1, 1, 0).unwrap()).unwrap();
    bad_digest["snapshot_digest"] = serde_json::json!(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    let bad_digest: EventLogCursorSnapshot = serde_json::from_value(bad_digest).unwrap();
    assert_eq!(
        bad_digest.validate().unwrap_err(),
        "eventlog_cursor_snapshot_digest_mismatch"
    );
}
