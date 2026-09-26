#[test]
fn symposium_runtime_reuses_bounded_harness_path_and_never_approves_on_budget_failure() {
    let runtime = include_str!("../../kiana-domain/src/symposium_runtime.rs");
    let governance = include_str!("../../kiana-domain/src/symposium_governance.rs");
    let core = include_str!("../src/control_plane.rs");
    let tests = include_str!("control_plane.rs");

    for marker in [
        "SYMPOSIUM_RUN_BUDGET_SCHEMA",
        "max_rounds",
        "max_messages",
        "max_tokens",
        "max_wall_time_ms",
        "max_stall_rounds",
        "symposium_round_budget_exhausted",
        "symposium_message_budget_exhausted",
        "symposium_stall_limit_exhausted",
        "symposium_cancelled",
        "decision_allowed",
        "convene_symposium",
        "skipped_meeting",
        "symposium_budget_exhaustion_or_cancel_never_emits_an_approved_decision",
        "symposium_and_async_proposal_share_the_same_decision_gate",
    ] {
        assert!(
            runtime.contains(marker)
                || governance.contains(marker)
                || core.contains(marker)
                || tests.contains(marker),
            "CO-15 marker missing: {marker}"
        );
    }
    assert!(!runtime.contains("CapabilityBroker"));
    assert!(!runtime.contains("EventStorePort"));
}
