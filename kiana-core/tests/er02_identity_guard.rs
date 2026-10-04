#[test]
fn event_identity_links_and_projection_use_stable_ids_not_request_sequence() {
    let states = include_str!("../../kiana-domain/src/states.rs");
    let events = include_str!("../src/events.rs");
    let correlation = include_str!("../../kiana-domain/src/correlation.rs");
    let projection = include_str!("../src/invocation_projection.rs");
    let history = include_str!("../src/history.rs");
    let history_fixture = include_str!("./control_plane.rs");
    let span = include_str!("../src/span_projection.rs");
    let baseline = include_str!("../../docs/roadmap/event-receipt-identity-baseline.md");
    let eventlog_fixtures = include_str!("../../kiana-eventlog/tests/er02_identity.rs");

    for marker in [
        "command_id",
        "correlation_id",
        "causation_event_id",
        "parent_event_id",
        "validate_identity_links",
        "with_identity_links",
    ] {
        assert!(
            states.contains(marker),
            "missing RuntimeEvent link {marker}"
        );
    }
    assert!(events.contains("stamp_event_links"));
    assert!(correlation.contains("CausationRef"));
    assert!(correlation.contains("AttemptRef"));
    for marker in [
        "correlation_attempt_id_invalid",
        "correlation_causation_event_id_invalid",
        "correlation_parent_link_invalid",
        "correlation_span_link_self",
    ] {
        assert!(
            correlation.contains(marker),
            "OA-02 malformed correlation guard missing: {marker}"
        );
    }
    assert!(projection.contains("event_run_id"));
    assert!(projection.contains("event_matches_run"));
    assert!(history.contains("event.request_id, event_order"));
    assert!(history.contains("sequence` is request-local"));
    assert!(history_fixture.contains("history_same_legacy_sequence_keeps_distinct_requests"));
    assert!(span.contains("turn_id"));
    assert!(span.contains("invocation_id"));
    assert!(baseline.contains("event_id_reuse_is_denied"));
    assert!(baseline.contains("same_request_different_command_digest_conflicts"));
    assert!(baseline.contains("cross_run_result_cannot_pair_by_sequence"));
    assert!(baseline.contains("legacy"));
    let event_store = include_str!("../../kiana-eventlog/src/event_store_core.rs");
    assert!(event_store.contains("event_identity_links_invalid"));
    for comparison in [
        "existing.command_id == candidate.command_id",
        "existing.correlation_id == candidate.correlation_id",
        "existing.causation_event_id == candidate.causation_event_id",
        "existing.parent_event_id == candidate.parent_event_id",
    ] {
        assert!(
            event_store.contains(comparison),
            "idempotent replay comparison missing: {comparison}"
        );
    }
    for fixture in [
        "idempotent_replay_rejects_command_id_drift",
        "idempotent_replay_rejects_correlation_id_drift",
        "idempotent_replay_rejects_causation_event_id_drift",
        "idempotent_replay_rejects_parent_event_id_drift",
    ] {
        assert!(
            eventlog_fixtures.contains(fixture),
            "idempotent replay denial fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "baseline fixture missing: {fixture}"
        );
    }
    assert!(eventlog_fixtures.contains("event_idempotency_key_payload_mismatch"));
}
