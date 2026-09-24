#[test]
fn notification_materializer_is_projection_only_and_server_scoped() {
    let source = include_str!("../src/notification_materializer.rs");
    for marker in [
        "NotificationMaterializer",
        "apply_committed",
        "validate_notification_runtime_event",
        "NotificationEventSource::EventLog",
        "source_event_id",
        "source_cursor",
        "materialization_digest",
        "human_inbox_items",
        "server_recipient_id",
        "decider_principal_id",
    ] {
        assert!(
            source.contains(marker),
            "materializer marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "DeliveryWorker",
        "NotificationStore",
        "reqwest",
        "tokio::spawn",
        "std::fs",
        "Provider",
    ] {
        assert!(
            !source.contains(forbidden),
            "materializer authority widened: {forbidden}"
        );
    }
}
