//! ER-22 source guard for cancellation recovery and stop confirmation.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-22 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cancellation_intent_covers_queued_pending_and_started_work() {
    let domain = include_str!("../../kiana-domain/src/cancellation.rs");
    let states = include_str!("../../kiana-domain/src/states.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let approvals = include_str!("../src/approvals.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let driver = include_str!("../../kiana-runner/src/state_driver.rs");
    let sessions = include_str!("../src/sessions.rs");

    require(
        domain,
        &[
            "RunCancellationFact",
            "command_id",
            "expected_version",
            "target_invocation_ids",
            "stop_requested",
            "stop_confirmed",
            "RunCancellationState::Stopping",
            "RunCancellationState::Cancelled",
            "RunCancellationState::ResultUnknown",
            "run_cancellation_fact_stop_confirmation_required",
        ],
        "durable cancellation fact",
    );
    require(
        states,
        &[
            "pub enum ExecutionStatus",
            "Queued",
            "AwaitingApproval",
            "Running",
            "Cancelling",
            "Cancelled",
            "ResultUnknown",
        ],
        "run states",
    );
    require(
        lifecycle,
        &[
            "record_cancel_requested",
            "run.cancelling",
            "signal_cancel",
            "RunnerCommand::Cancel",
            "await_capability_stop",
            "run.cancelled",
            "run.result_unknown",
            "result_unknown:cancel_stop_unconfirmed",
            "cancelled:approval_pending",
            "replay_safe",
        ],
        "ControlPlane cancellation",
    );
    require(
        approvals,
        &[
            "cancel_pending_tools",
            "cancelled:before_dispatch",
            "not_executed",
        ],
        "approval cancellation",
    );
    require(
        runner,
        &[
            "cancellation.request",
            "cancelled:queued",
            "pending_tools.drain(..)",
            "ToolCancelled",
            "not_executed",
        ],
        "runner queued cancellation",
    );
    require(
        driver,
        &[
            "RequestCancel",
            "StopConfirmed",
            "CancelPendingTools",
            "RecoveryRequired",
        ],
        "runner state driver",
    );
    require(
        sessions,
        &["signal_cancel", "pre-signaled", "await_capability_stop"],
        "cancellation signal",
    );
}

#[test]
fn stop_ack_and_effect_fences_block_late_results_and_success() {
    let dispatch = include_str!("../src/dispatch.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let events = include_str!("../src/events.rs");
    let shell = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let projection = include_str!("../src/projection.rs");

    require(
        dispatch,
        &[
            "cancelled:before_dispatch",
            "result.delivery_claimed",
            "cancelled:result_delivery_run_inactive",
            "stop_requested",
            "stop_confirmed",
            "fenced",
            "commit_confirmed",
        ],
        "dispatch fence",
    );
    require(
        lifecycle,
        &[
            "response_run_ids_match",
            "cancel_response_run_id_mismatch",
            "cancel_confirmation_inconsistent",
            "cancel_confirmation_missing",
            "stop_confirmed",
        ],
        "stop acknowledgement",
    );
    require(
        events,
        &[
            "record_terminal_event",
            "run_cancellation_fact_state_mismatch",
        ],
        "terminal event boundary",
    );
    require(
        supervisor,
        &[
            "ProcessSupervisor::stop",
            "StopReport",
            "stop_confirmed",
            "shell_result_unknown:cancel_stop_unconfirmed",
            "kill_on_drop",
        ],
        "shell stop evidence",
    );
    require(
        mcp,
        &[
            "client.stop()",
            "mcp_stop_unconfirmed",
            "mcp_call_unconfirmed",
            "stop_confirmed",
        ],
        "MCP stop evidence",
    );
    require(
        ports,
        &[
            "wait_for_cancellation",
            "cancelled:before_prepare",
            "result_unknown:cancel_stop_unconfirmed",
        ],
        "port cancellation boundary",
    );
    require(
        projection,
        &["run.cancelled", "run.result_unknown", "TerminalConflict"],
        "terminal projection",
    );

    for source in [dispatch, lifecycle, shell, mcp, ports] {
        for forbidden in [
            "cancelled_without_stop_confirmation_is_success",
            "retry_after_cancel",
            "unknown_effect_is_success",
            "late_result_resurrects_run",
        ] {
            assert!(
                !source.contains(forbidden),
                "ER-22 cancellation bypass marker present: {forbidden}"
            );
        }
    }
}

#[test]
fn cancellation_regression_fixtures_are_ci_bound() {
    let p0_j1 = include_str!("p0_j1_01_cancellation_guard.rs");
    let p0_j1_02 = include_str!("p0_j1_02_cancellation_guard.rs");
    let cp15 = include_str!("cp15_cancellation_guard.rs");
    let sc15 = include_str!("sc15_cancel_unknown_guard.rs");
    let h08 = include_str!("../../kiana-runner/tests/h08_cancellation_guard.rs");
    let h02 = include_str!("h02_lifecycle.rs");

    require(
        p0_j1,
        &[
            "cancellation_uses_one_fact_and_signal_chain",
            "late_results",
        ],
        "P0-J1-01 fixture",
    );
    require(
        p0_j1_02,
        &[
            "cancel_drains_queued_tool_calls",
            "ToolCancelled",
            "replay_safe",
        ],
        "P0-J1-02 fixture",
    );
    require(
        cp15,
        &[
            "cancel_before_signal",
            "await_capability_stop",
            "run.result_unknown",
        ],
        "CP-15 fixture",
    );
    require(
        sc15,
        &[
            "cancel_fence_and_unknown_reconciliation_boundary_is_present",
            "result_unknown",
        ],
        "SC-15 fixture",
    );
    require(
        h08,
        &[
            "h08_cancellation_fence_covers_model_and_effect_boundaries",
            "stop_confirmed",
        ],
        "H08 fixture",
    );
    require(
        h02,
        &[
            "late_result_cannot_complete_a_new_turn",
            "continue_closed_run_requires_new_admission",
        ],
        "H02 fixture",
    );
}
