#[test]
fn connector_notifications_are_projection_only_and_secret_free() {
    let domain = include_str!("../../kiana-domain/src/connector_notifications.rs");
    let events = include_str!("../../kiana-domain/src/notification_events.rs");
    let core = include_str!("../src/connector_notifications.rs");
    for marker in [
        "connector.health",
        "connector.invocation",
        "connector.reconciliation",
        "connector.approval",
        "source_cursor",
        "evidence_digest",
        "dedup_key",
        "limitation",
        "validate_connector_notification",
    ] {
        assert!(
            domain.contains(marker) || events.contains(marker) || core.contains(marker),
            "INT-27 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "send_notification",
        "approve_command",
        "raw_response",
    ] {
        assert!(
            !domain.contains(forbidden) && !events.contains(forbidden) && !core.contains(forbidden),
            "INT-27 notification boundary widened: {forbidden}"
        );
    }
}
