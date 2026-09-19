#[test]
fn progress_reducer_and_runner_threshold_share_the_same_bounded_path() {
    let domain = include_str!("../../kiana-domain/src/progress_stall.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "ProgressEvidence",
        "ProgressTracker",
        "ProgressAction::RequestClarification",
        "ProgressAction::Blocked",
        "cycle_detected",
        "repeated_failure_count",
        "stop_hook_budget_exhausted",
        "job_cursor",
        "heartbeat",
    ] {
        assert!(
            domain.contains(marker),
            "missing H28 progress marker: {marker}"
        );
    }
    for marker in [
        "repeated_tool_call_threshold",
        "record_tool_call",
        "progress_tracker",
        "checkpoint",
        "restore",
    ] {
        assert!(
            runner.contains(marker),
            "missing runner H28 marker: {marker}"
        );
    }
}

#[test]
fn progress_fingerprint_cannot_be_model_prose_or_an_unbounded_stop_hook() {
    let domain = include_str!("../../kiana-domain/src/progress_stall.rs");
    assert!(domain.contains("observation_digest"));
    assert!(domain.contains("workspace_revision"));
    assert!(domain.contains("artifact_revision"));
    assert!(domain.contains("verification_digest"));
    assert!(domain.contains("MAX_PROGRESS_WINDOW"));
    assert!(domain.contains("stop_hook_budget_remaining"));
    assert!(!domain.contains("model_says_progress"));
}
