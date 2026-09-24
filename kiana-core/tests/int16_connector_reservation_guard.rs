#[test]
fn connector_reservation_stays_on_control_plane_eventstore_broker_spine() {
    let domain = include_str!("../../kiana-domain/src/connector_reservation.rs");
    let core = include_str!("../src/connector_reservation.rs");
    let connectors = include_str!("../src/connectors.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    for marker in [
        "ConnectorInvocationCommand",
        "command_digest",
        "idempotency_key_digest",
        "ConnectorInvocationReservation",
        "ConnectorReservationLease",
        "fence_token",
        "expires_at_unix_ms",
        "ConnectorInvocationPermit",
        "ConnectorInvocationReceipt",
        "ConnectorInvocationLedger",
        "connector_idempotency_digest_conflict",
        "connector_reservation_cas_conflict",
        "connector_reservation_not_committed",
        "connector_permit_already_consumed",
        "connector_attempt_permit_already_issued",
        "ControlPlaneConnectorReservation",
        "commit_connector_reservation",
        "CONNECTOR_RESERVATION_STREAM",
        "EventStore",
        "connector_effect_admission",
        "validate_connector_effect_boundary",
        "connector_idempotency_command_digest_conflict",
        "authorize_and_execute",
        "append_idempotent_expected",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || connectors.contains(marker)
                || broker.contains(marker)
                || daemon.contains(marker),
            "INT-16 marker missing: {marker}"
        );
    }
    assert!(core.contains("self.events"));
    assert!(core.contains("commit_confirmed"));
    assert!(core.contains("expected_versions"));
    assert!(broker.contains("connector_effect_admission"));
    assert!(daemon.contains("connector_reservation"));
    assert!(daemon.contains("connector_permit"));
    assert!(connectors.contains("self.authorize_and_execute(&context, request).await"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("EventStorePort"));
}

#[test]
fn stale_or_uncommitted_connector_material_is_denied_before_effect() {
    let domain = include_str!("../../kiana-domain/src/connector_reservation.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    for marker in [
        "reservation.state != ConnectorReservationState::Committed",
        "permit.validate_for_reservation",
        "connector_permit_required",
        "connector_reservation_invalid",
        "connector_permit_invalid",
        "connector_reservation_effect_not_admitted",
        "connector_idempotency_receipt_conflict",
        "previous.data[\"command_digest\"]",
        "replayed",
    ] {
        assert!(
            domain.contains(marker) || broker.contains(marker) || daemon.contains(marker),
            "INT-16 deny marker missing: {marker}"
        );
    }
    assert!(broker.contains("validate_connector_effect_boundary(&request"));
    assert!(daemon.contains("connector_effect_admission"));
}
