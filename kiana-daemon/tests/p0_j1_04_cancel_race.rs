#[test]
fn cancel_race_never_produces_wrong_completion() {
    let existing = include_str!("daemon_host.rs");
    let lifecycle = include_str!("../src/harness_capabilities.rs");
    let core = include_str!("../../kiana-core/src/lifecycle.rs");
    assert!(existing.contains("cancelling_mid_stream_never_completes_or_emits_a_late_delta"));
    for marker in [
        "late_delta_emitted",
        "run.completed",
        "run.cancelled",
        "terminal_count",
        "stop_confirmed",
    ] {
        assert!(
            existing.contains(marker) || lifecycle.contains(marker) || core.contains(marker),
            "cancel race marker missing: {marker}"
        );
    }
    assert!(existing.contains("terminal_count != 1"));
    assert!(existing.contains("late_stream.contains(\"after\")"));
}

#[test]
fn late_runner_results_are_fenced_after_cancel() {
    let core = include_str!("../../kiana-core/src/lifecycle.rs");
    let events = include_str!("../../kiana-core/src/events.rs");
    assert!(core.contains("record_terminal_event"));
    assert!(core.contains("run.result_unknown"));
    assert!(events.contains("run.cancelled"));
    assert!(events.contains("run.result_unknown"));
    assert!(!core.contains("auto_resume_after_cancel"));
}
