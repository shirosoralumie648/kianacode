#[test]
fn event_identity_links_and_projection_use_stable_ids_not_request_sequence() {
    let states = include_str!("../../kiana-domain/src/states.rs");
    let events = include_str!("../src/events.rs");
    let correlation = include_str!("../../kiana-domain/src/correlation.rs");
    let projection = include_str!("../src/invocation_projection.rs");
    let span = include_str!("../src/span_projection.rs");
    let baseline = include_str!("../../docs/roadmap/event-receipt-identity-baseline.md");

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
    assert!(projection.contains("event_run_id"));
    assert!(projection.contains("event_matches_run"));
    assert!(span.contains("turn_id"));
    assert!(span.contains("invocation_id"));
    assert!(baseline.contains("event_id_reuse_is_denied"));
    assert!(baseline.contains("same_request_different_command_digest_conflicts"));
    assert!(baseline.contains("cross_run_result_cannot_pair_by_sequence"));
    assert!(baseline.contains("legacy"));
}
