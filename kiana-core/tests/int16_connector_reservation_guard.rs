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
        "connector_reservation_required",
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

const DOMAIN_FIXTURES: &str =
    include_str!("../../kiana-domain/tests/int16_connector_reservation.rs");
const BROKER_FIXTURES: &str =
    include_str!("../../kiana-capability-broker/tests/int16_connector_reservation.rs");
const DAEMON_FIXTURES: &str =
    include_str!("../../kiana-daemon/tests/int16_connector_reservation.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int16-connector-reservation.yml");

#[test]
fn connector_reservation_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    for fixture in [
        // Domain CAS/command-digest fixtures.
        "same_idempotency_key_with_different_command_digest_is_denied",
        "all_server_owned_revisions_are_rechecked_at_effect_boundary",
        "uncommitted_reservation_old_permit_and_replayed_receipt_have_zero_new_effect",
        "command_digest_covers_every_server_owned_field_and_refuses_tampering",
        "same_command_replays_the_first_reservation_and_cas_races_are_denied",
        "one_attempt_consumes_exactly_one_permit_and_uncommitted_work_cannot_consume",
        "stale_leases_permits_and_epochs_never_reach_the_adapter",
        "receipt_conflicts_and_effects_before_consumption_are_refused",
        // Broker effect-boundary fixtures: deny-first with a recording handler at zero calls.
        "committed_reservation_reaches_the_handler_exactly_once_with_its_command_digest",
        "uncommitted_stale_and_tampered_reservations_never_reach_the_handler",
        // Daemon spine fixtures: command digest conflict, replay and wire smuggling.
        "same_key_with_a_different_command_digest_is_denied_before_any_effect",
        "replayed_command_returns_the_original_receipt_and_keeps_one_attempt",
        "wire_supplied_reservation_envelope_is_refused_before_any_effect",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture)
                || BROKER_FIXTURES.contains(fixture)
                || DAEMON_FIXTURES.contains(fixture),
            "INT-16 fixture missing: {fixture}"
        );
    }
    for step in [
        "cargo test -p kiana-domain --test int16_connector_reservation",
        "cargo test -p kiana-capability-broker --test int16_connector_reservation",
        "cargo test -p kiana-daemon --test int16_connector_reservation",
        "cargo test -p kiana-core --test int16_connector_reservation_guard",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-16 lane step: {step}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("contents: read"));
    for forbidden in ["push:", "pull_request:", "cargo test --workspace"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "INT-16 lane must not contain {forbidden}"
        );
    }
}
