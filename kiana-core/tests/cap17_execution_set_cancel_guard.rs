//! CAP-17 source guard for run-wide cancellation, execution-set stop and Unknown fencing.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-17 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cancel_persists_intent_then_stops_all_execution_classes() {
    let domain = include_str!("../../kiana-domain/src/cancellation.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let approvals = include_str!("../src/approvals.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let shell = include_str!("../../kiana-daemon/src/harness_capabilities.rs");

    require(
        domain,
        &[
            "RunCancellationFact",
            "RunCancellationState::Stopping",
            "RunCancellationState::Cancelled",
            "RunCancellationState::ResultUnknown",
            "stop_requested",
            "stop_confirmed",
        ],
        "durable cancellation",
    );
    require(
        lifecycle,
        &[
            "record_cancel_requested",
            "run.cancelling",
            "signal_cancel",
            "await_capability_stop",
            "run.cancelled",
            "run.result_unknown",
            "record_terminal_event",
        ],
        "control-plane order",
    );
    require(
        dispatch,
        &[
            "commit_confirmed",
            "not_executed",
            "result.delivery_claimed",
        ],
        "queued/late dispatch fence",
    );
    require(
        approvals,
        &["cancel_pending_tools", "not_executed"],
        "approval fence",
    );
    require(
        runner,
        &[
            "cancellation.request",
            "pending_tools.drain(..)",
            "ToolCancelled",
        ],
        "runner queued fence",
    );
    require(
        supervisor,
        &[
            "ProcessSupervisor",
            "StopReport",
            "confirmed",
            "wait_leader",
        ],
        "process stop evidence",
    );
    require(
        shell,
        &[
            "ProcessSupervisor::stop",
            "stop_report",
            "shell_result_unknown:cancel_stop_unconfirmed",
        ],
        "shell stop routing",
    );
    let persisted = lifecycle.find("record_cancel_requested(").unwrap();
    let signal = lifecycle.find("self.signal_cancel(run_id)").unwrap();
    assert!(
        persisted < signal,
        "cancel signal must follow durable intent"
    );
    for forbidden in [
        "cancelled_without_stop_confirmation_is_success",
        "auto_retry_after_cancel",
    ] {
        assert!(
            !lifecycle.contains(forbidden),
            "CAP-17 bypass marker present: {forbidden}"
        );
    }
}

#[test]
fn cancel_race_fixtures_remain_part_of_the_remote_gate() {
    let race = include_str!("../../kiana-daemon/tests/p0_j1_04_cancel_race.rs");
    let host = include_str!("../../kiana-daemon/tests/daemon_host.rs");
    require(
        race,
        &[
            "cancelling_mid_stream_never_completes_or_emits_a_late_delta",
            "run.cancelled",
            "run.result_unknown",
        ],
        "cancel race fixture",
    );
    require(
        host,
        &[
            "cancelling_mid_stream_never_completes_or_emits_a_late_delta",
            "cancel_stops_in_flight_shell_before_it_writes",
            "shell_timeout_ms_is_brokered_as_a_timed_out_tool_result",
        ],
        "daemon integration fixtures",
    );
}
