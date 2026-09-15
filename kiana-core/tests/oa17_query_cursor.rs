use kiana_core::{AuditProjectionError, AuditQueryInput};
use kiana_domain::{AuditActionKind, AuditDecision, AuditQueryCursor};

fn filter_digest() -> String {
    kiana_domain::json_digest(&serde_json::json!({
        "action_kind": "capability",
        "decision": "denied",
        "target_kind": "capability",
    }))
}

#[test]
fn cursor_binds_epoch_projection_source_and_filter_digest() {
    let cursor = AuditQueryCursor::new("epoch:one", 1, 10, 4, filter_digest()).unwrap();
    cursor.validate().unwrap();
    let encoded = serde_json::to_string(&cursor).unwrap();
    assert_eq!(
        serde_json::from_str::<AuditQueryCursor>(&encoded).unwrap(),
        cursor
    );
    let mut forged = cursor.clone();
    forged.after_cursor = 5;
    assert_eq!(
        forged.validate().unwrap_err(),
        "audit_query_cursor_out_of_range"
    );
    let mut bad_digest = cursor;
    bad_digest.filter_digest =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
    assert_eq!(
        bad_digest.validate().unwrap_err(),
        "audit_query_cursor_digest_mismatch"
    );
}

#[test]
fn query_input_rejects_cursor_field_mismatch_and_unbounded_page() {
    let cursor = AuditQueryCursor::new("epoch:one", 1, 10, 4, filter_digest()).unwrap();
    let mut query = AuditQueryInput {
        source_cursor: Some(10),
        after_cursor: 0,
        limit: 10,
        action_kind: Some(AuditActionKind::Capability),
        decision: Some(AuditDecision::Denied),
        target_kind: Some("capability".to_owned()),
        cursor: Some(cursor.clone()),
    };
    query.validate().unwrap();
    query.after_cursor = 3;
    assert_eq!(
        query.validate().unwrap_err(),
        AuditProjectionError::QueryCursorInvalid
    );
    query.after_cursor = 0;
    query.source_cursor = Some(9);
    assert_eq!(
        query.validate().unwrap_err(),
        AuditProjectionError::QueryCursorInvalid
    );
    query.source_cursor = Some(10);
    query.limit = 1_001;
    assert_eq!(
        query.validate().unwrap_err(),
        AuditProjectionError::QueryLimitInvalid
    );
    let mut stale = cursor;
    stale.projection_version = 2;
    stale.cursor_digest = stale.digest();
    stale.validate().unwrap();
}
