use kiana_domain::{validate_connector_notification_payload, NotificationEventClass};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn payload() -> serde_json::Value {
    json!({
        "source": "eventlog",
        "connector_id": "connector-1",
        "binding_id": "binding-1",
        "owner_id": "owner-1",
        "source_cursor": 12,
        "evidence_digest": D,
        "dedup_key": "connector-1:invocation-1",
        "summary": "Connector invocation requires review",
        "limitation": "provider outcome is not inferred",
    })
}

#[test]
fn connector_health_invocation_reconcile_and_approval_are_registered_projections() {
    assert_eq!(
        validate_connector_notification_payload("connector.health", &payload()).unwrap(),
        NotificationEventClass::Status
    );
    assert_eq!(
        validate_connector_notification_payload("connector.invocation", &payload()).unwrap(),
        NotificationEventClass::Status
    );
    assert_eq!(
        validate_connector_notification_payload("connector.reconciliation", &payload()).unwrap(),
        NotificationEventClass::Incident
    );
    assert_eq!(
        validate_connector_notification_payload("connector.approval", &payload()).unwrap(),
        NotificationEventClass::Approval
    );
}

#[test]
fn connector_notification_requires_cursor_evidence_limitation_and_redaction() {
    let mut missing = payload();
    missing["source_cursor"] = json!(0);
    assert_eq!(
        validate_connector_notification_payload("connector.health", &missing).unwrap_err(),
        "connector_notification_source_cursor_required"
    );

    let mut secret = payload();
    secret["summary"] = json!("Authorization: Bearer abc123");
    assert_eq!(
        validate_connector_notification_payload("connector.health", &secret).unwrap_err(),
        "connector_notification_summary_invalid"
    );

    let mut unknown = payload();
    unknown["source"] = json!("model");
    assert_eq!(
        validate_connector_notification_payload("connector.health", &unknown).unwrap_err(),
        "notification_event_source_untrusted"
    );

    assert_eq!(
        validate_connector_notification_payload("connector.unknown", &payload()).unwrap_err(),
        "connector_notification_kind_unregistered"
    );
}
