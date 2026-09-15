#[test]
fn p0_g04_projection_and_recovery_paths_are_event_owned() {
    let projection = include_str!("../src/projection.rs");
    let invocation = include_str!("../src/invocation_projection.rs");
    let recovery = include_str!("../src/recovery.rs");
    let events = include_str!("../src/events.rs");
    let control_plane_tests = include_str!("control_plane.rs");
    let baseline = include_str!("../../docs/roadmap/event-receipt-recovery-baseline.md");

    assert!(projection.contains("project_run_state"));
    assert!(projection.contains("cache_invocation_projection"));
    assert!(projection.contains("invalidate_invocation_projection"));
    assert!(invocation.contains("project_invocations"));
    assert!(invocation.contains("invocation_terminal_conflict"));
    assert!(invocation.contains("invocation_state_transition_invalid"));
    assert!(recovery.contains("rebuild_pending_invocation"));
    assert!(recovery.contains("cache_invocation_projection"));
    assert!(recovery.contains("approval_continuation_unavailable"));
    assert!(events.contains("invalidate_invocation_projection"));
    for fixture in [
        "new_process_rebuilds_run_state_from_events_alone",
        "new_process_rebuilds_invocation_state_from_events_alone",
        "invocation_projection_conflicting_terminals_fail_closed",
        "projection_cache_miss_rebuilds_pending_invocations_with_authorization_recheck",
    ] {
        assert!(
            control_plane_tests.contains(fixture),
            "missing P0-G-04 fixture {fixture}"
        );
    }
    assert!(baseline.contains("EventLog"));
    assert!(baseline.contains("Receipt projection"));
    assert!(baseline.contains("result_unknown"));
}
