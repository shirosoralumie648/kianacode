use kiana_domain::{AuditActionKind, AuditDecision};
use kiana_protocol::{
    AuditQueryRequest, RequestBody, RequestEnvelope, RequestMetadata, AUDIT_QUERY_SCHEMA,
};

#[test]
fn audit_query_wire_is_versioned_bounded_and_round_trips() {
    let metadata = RequestMetadata::local("session-1", "/repo");
    let query = AuditQueryRequest {
        source_cursor: Some(9),
        after_cursor: 3,
        limit: 10,
        action_kind: Some(AuditActionKind::Capability),
        decision: Some(AuditDecision::Denied),
        target_kind: Some("capability".to_owned()),
        cursor: None,
    };
    query.validate().unwrap();
    let request = RequestEnvelope::audit_query(metadata, query.clone());
    assert!(matches!(request.body, RequestBody::AuditQuery(_)));
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["body"]["type"], "audit_query");
    assert_eq!(encoded["body"]["request"]["limit"], 10);
    assert_eq!(
        serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
        request
    );
    assert_eq!(AUDIT_QUERY_SCHEMA, "kiana.audit-query.v1");
}

#[test]
fn audit_query_rejects_unbounded_or_stale_client_filters() {
    let base = AuditQueryRequest {
        source_cursor: Some(4),
        after_cursor: 0,
        limit: 1,
        action_kind: None,
        decision: None,
        target_kind: None,
        cursor: None,
    };
    let mut unlimited = base.clone();
    unlimited.limit = 1_001;
    assert_eq!(
        unlimited.validate().unwrap_err(),
        "audit_query_limit_invalid"
    );
    let mut cursor = base.clone();
    cursor.source_cursor = Some(0);
    assert_eq!(cursor.validate().unwrap_err(), "audit_query_cursor_invalid");
    let mut after = base;
    after.after_cursor = 5;
    assert_eq!(after.validate().unwrap_err(), "audit_query_cursor_invalid");
    let mut target = AuditQueryRequest {
        source_cursor: None,
        after_cursor: 0,
        limit: 1,
        action_kind: None,
        decision: None,
        target_kind: Some(" ".to_owned()),
        cursor: None,
    };
    assert_eq!(target.validate().unwrap_err(), "audit_query_filter_invalid");
    target.target_kind = Some("run".to_owned());
    target.validate().unwrap();
}

#[test]
fn audit_query_wire_rejects_owner_scope_and_raw_event_fields() {
    let metadata = RequestMetadata::local("session-1", "/repo");
    let request = RequestEnvelope::audit_query(
        metadata,
        AuditQueryRequest {
            source_cursor: None,
            after_cursor: 0,
            limit: 10,
            action_kind: None,
            decision: None,
            target_kind: None,
            cursor: None,
        },
    );
    let mut encoded = serde_json::to_value(&request).unwrap();
    encoded["body"]["request"]["owner"] = serde_json::json!("foreign");
    assert!(serde_json::from_value::<RequestEnvelope>(encoded).is_err());
    let mut encoded = serde_json::to_value(&request).unwrap();
    encoded["body"]["request"]["raw_events"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RequestEnvelope>(encoded).is_err());
}
