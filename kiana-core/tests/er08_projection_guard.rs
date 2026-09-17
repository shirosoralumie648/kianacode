#[test]
fn er08_run_projection_is_terminal_and_replay_safe() {
    let projection = include_str!("../src/projection.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    for marker in [
        "project_run_state",
        "RunProjectionError::TerminalConflict",
        "outcome_for_kind",
        "seen_keys",
        "run.prompt",
        "run.queued",
        "run.cancelling",
        "approval.requested",
        "RunPhase::AwaitingApproval",
        "RunPhase::Terminal",
        "if outcome.is_some()",
        "TerminalConflict",
    ] {
        assert!(
            projection.contains(marker) || lifecycle.contains(marker),
            "ER-08 marker missing: {marker}"
        );
    }
    let terminal = projection
        .find("if let Some(event_outcome) = outcome_for_kind")
        .expect("terminal reducer boundary");
    let late_event_guard = projection
        .find("if outcome.is_some()")
        .expect("late event guard");
    assert!(terminal < late_event_guard);
    for forbidden in [
        "CapabilityBrokerPort",
        "commit_transition",
        "signal_cancel",
        "auto_resume_terminal",
    ] {
        assert!(
            !projection.contains(forbidden),
            "projection must not {forbidden}"
        );
    }
}
