#[test]
fn er08_run_projection_is_terminal_and_replay_safe() {
    let projection = include_str!("../src/projection.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    for marker in [
        "project_run_state",
        "RunProjectionError::TerminalConflict",
        "RunProjectionError::EventRunIdConflict",
        "RunProjectionError::EventIdentityConflict",
        "RunProjectionError::IdentityLinksInvalid",
        "run_event_run_id_conflict",
        "run_event_run_id_invalid",
        "run_identity_links_invalid",
        ".validate_identity_links()",
        "event_run_id(event)?",
        "stream_version.is_some()",
        "Request-local sequence",
        "outcome_for_kind",
        "seen_events",
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
    let identity_validation = projection
        .find(".validate_identity_links()")
        .expect("identity-link validation boundary");
    let run_id_validation = projection
        .find("event_run_id(event)?")
        .expect("run identity validation boundary");
    assert!(identity_validation < run_id_validation);
    assert!(projection.contains("let mut ordered_events = Vec::new()"));
    assert!(projection.contains("if previous != event"));
    assert!(projection.contains("run_event_identity_conflict:"));
    assert!(projection.contains("stream_version.is_some()"));
    assert!(projection.contains("Request-local sequence"));
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
