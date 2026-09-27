use kiana_domain::{BillingQueryKind, BillingQueryRequest, ProjectId};
use kiana_protocol::{
    BillingQueryCursor, RequestBody, RequestEnvelope, RequestMetadata, BILLING_QUERY_SCHEMA,
};

#[test]
fn billing_query_uses_one_versioned_read_only_wire_envelope() {
    let query = BillingQueryRequest {
        schema: BILLING_QUERY_SCHEMA.to_owned(),
        kind: BillingQueryKind::UsageBreakdown,
        project_id: ProjectId::new(),
        data_boundary_digest:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        source_cursor: Some(8),
        after_cursor: 0,
        limit: 20,
        cursor: None,
        read_only: true,
    };
    query.validate().unwrap();
    let envelope =
        RequestEnvelope::billing_query(RequestMetadata::local("session-1", "/repo"), query);
    assert!(matches!(envelope.body, RequestBody::BillingQuery(_)));
    let encoded = serde_json::to_value(&envelope).unwrap();
    assert_eq!(encoded["body"]["type"], "billing_query");
    assert_eq!(
        serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
        envelope
    );
}

#[test]
fn billing_query_cursor_cannot_cross_boundary_or_projection() {
    let cursor = BillingQueryCursor::new(
        8,
        2,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .unwrap();
    let mut request = BillingQueryRequest {
        schema: BILLING_QUERY_SCHEMA.to_owned(),
        kind: BillingQueryKind::CostExport,
        project_id: ProjectId::new(),
        data_boundary_digest:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        source_cursor: Some(8),
        after_cursor: 2,
        limit: 20,
        cursor: Some(cursor),
        read_only: true,
    };
    request.validate().unwrap();
    request.data_boundary_digest =
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();
    assert_eq!(
        request.validate().unwrap_err(),
        "billing_query_cursor_binding_invalid"
    );
}
