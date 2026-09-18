#[test]
fn notification_projector_is_committed_only_and_checkpoint_bound() {
    let source = include_str!("../src/notification_projector.rs");
    for marker in [
        "NotificationProjection",
        "validate_notification_runtime_event",
        "NotificationEventSource::EventLog",
        "ReplayProjection::from_checkpoint",
        "ReplayProjection::from_zero",
        "source_cursor",
        "notification_projection_payload_missing",
    ] {
        assert!(
            source.contains(marker),
            "notification projector marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "reqwest",
        "tokio::spawn",
        "send(",
    ] {
        assert!(
            !source.contains(forbidden),
            "notification authority widened: {forbidden}"
        );
    }
}
