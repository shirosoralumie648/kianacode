use kiana_domain::{AuditExportFormat, AuditQueryCursor};
use kiana_protocol::{
    AuditExportRequest, AuditQueryRequest, RequestBody, RequestEnvelope, RequestMetadata,
};

fn export_request() -> AuditExportRequest {
    AuditExportRequest {
        query: AuditQueryRequest {
            source_cursor: None,
            after_cursor: 0,
            limit: 10,
            action_kind: None,
            decision: None,
            target_kind: None,
            cursor: None,
        },
        format: AuditExportFormat::Jsonl,
        purpose: "security review".to_owned(),
        recipient: "principal:operator".to_owned(),
        retention_class: "audit".to_owned(),
        deliver: false,
    }
}

#[test]
fn audit_export_wire_is_closed_and_round_trips_without_scope_fields() {
    let request = RequestEnvelope::audit_export(
        RequestMetadata::local("session-1", "/repo"),
        export_request(),
    );
    assert!(matches!(request.body, RequestBody::AuditExport(_)));
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["body"]["type"], "audit_export");
    assert!(encoded["body"]["request"].get("owner").is_none());
    assert!(encoded["body"]["request"].get("raw_events").is_none());
    assert_eq!(
        serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
        request
    );
}

#[test]
fn audit_export_rejects_missing_purpose_recipient_retention_and_bad_query() {
    let mut export = export_request();
    export.purpose.clear();
    assert_eq!(export.validate().unwrap_err(), "audit_export_purpose");
    let mut export = export_request();
    export.recipient = " ".to_owned();
    assert_eq!(export.validate().unwrap_err(), "audit_export_recipient");
    let mut export = export_request();
    export.retention_class = "x".repeat(65);
    assert_eq!(
        export.validate().unwrap_err(),
        "audit_export_retention_class"
    );
    let mut export = export_request();
    export.query.limit = 0;
    assert_eq!(export.validate().unwrap_err(), "audit_query_limit_invalid");
}

#[test]
fn export_request_cannot_smuggle_a_cursor_with_conflicting_legacy_fields() {
    let mut export = export_request();
    export.query.cursor = Some(
        AuditQueryCursor::new(
            "epoch:one",
            1,
            3,
            2,
            kiana_domain::json_digest(&serde_json::json!("filters")),
        )
        .unwrap(),
    );
    export.query.source_cursor = Some(4);
    assert_eq!(export.validate().unwrap_err(), "audit_query_cursor_invalid");
}
