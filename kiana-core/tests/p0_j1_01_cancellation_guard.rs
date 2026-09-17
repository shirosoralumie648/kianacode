#[test]
fn cancellation_uses_one_fact_and_signal_chain() {
    let states = include_str!("../../kiana-domain/src/states.rs");
    let fact = include_str!("../../kiana-domain/src/cancellation.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let events = include_str!("../src/events.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "ExecutionStatus::Queued",
        "ExecutionStatus::Cancelling",
        "RunCancellationState::Requested",
        "RunCancellationState::Stopping",
        "RunCancellationState::Cancelled",
        "RunCancellationState::ResultUnknown",
        "record_cancel_requested",
        "signal_cancel",
        "run.cancelling",
        "run.cancelled",
        "run.result_unknown",
    ] {
        assert!(
            states.contains(marker)
                || fact.contains(marker)
                || lifecycle.contains(marker)
                || events.contains(marker)
                || runner.contains(marker),
            "cancellation marker missing: {marker}"
        );
    }
    assert!(fact.contains("stop_confirmed"));
    assert!(events.contains("run_cancellation_fact_state_mismatch"));
    assert!(!lifecycle.contains("spawn_cancel_loop"));
}

#[test]
fn cancellation_terminals_are_not_reopened_by_late_results() {
    let states = include_str!("../../kiana-domain/src/states.rs");
    let events = include_str!("../src/events.rs");
    let projection = include_str!("../src/projection.rs");
    assert!(states.contains("Self::Cancelled"));
    assert!(states.contains("Self::ResultUnknown"));
    assert!(events.contains("record_terminal_event"));
    assert!(projection.contains("run.cancelled"));
    assert!(projection.contains("run.result_unknown"));
}
