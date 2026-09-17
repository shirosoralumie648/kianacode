#[test]
fn cp15_persists_cancel_before_signal_and_closes_queued_invocations() {
    let domain = include_str!("../../kiana-domain/src/cancellation.rs");
    let states = include_str!("../../kiana-domain/src/states.rs");
    let event_contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let events = include_str!("../src/events.rs");
    let approvals = include_str!("../src/approvals.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let sessions = include_str!("../src/sessions.rs");
    let runner_protocol = include_str!("../../kiana-runner-protocol/src/lib.rs");
    for marker in [
        "RunCancellationFact",
        "RUN_CANCELLATION_SCHEMA",
        "RunCancellationState::Stopping",
        "RunCancellationState::Cancelled",
        "RunCancellationState::ResultUnknown",
        "record_cancel_requested",
        "commit_confirmed",
        "run.cancelling",
        "cancellation_fact",
        "signal_cancel",
        "await_capability_stop",
        "ToolCancelled",
        "not_executed",
        "run.cancelled",
        "run.result_unknown",
        "record_terminal_event",
    ] {
        assert!(
            domain.contains(marker)
                || states.contains(marker)
                || event_contracts.contains(marker)
                || lifecycle.contains(marker)
                || events.contains(marker)
                || approvals.contains(marker)
                || dispatch.contains(marker)
                || sessions.contains(marker)
                || runner_protocol.contains(marker),
            "CP-15 marker missing: {marker}"
        );
    }

    let persisted = lifecycle
        .find("record_cancel_requested(")
        .expect("cancel command must persist a state fact");
    let signal = lifecycle
        .find("self.signal_cancel(run_id)")
        .expect("runner signal must remain after persistence");
    assert!(
        persisted < signal,
        "cancel signal cannot precede durable intent"
    );
    assert!(lifecycle.contains("run_cancel_command_conflict"));
    assert!(lifecycle.contains("run_cancel_terminal_state_missing"));
    assert!(events.contains("run_cancellation_fact_state_mismatch"));
    assert!(approvals.contains("not_executed"));
    assert!(lifecycle.contains("cancelled:approval_pending"));
    assert!(dispatch.contains("commit_confirmed"));
    for forbidden in [
        "spawn_cancel_loop",
        "authorize_and_execute_from_cancel",
        "broker.execute_cancel",
    ] {
        assert!(
            !lifecycle.contains(forbidden) && !dispatch.contains(forbidden),
            "cancellation must not add {forbidden}"
        );
    }
}
