//! H08 focused source guard for the daemon/Core half of the deadline-and-cancel boundary.
//!
//! The runner-side fixtures (`kiana-runner/tests/h08_cancellation.rs`) prove the model
//! port fence: a silent model is interrupted by the attempt deadline and a cancel during
//! retry backoff prevents the next provider attempt. This file pins the other two named
//! H08 fixtures that live at the effect boundary:
//!
//! * `unconfirmed_tool_stop_is_unknown` — a cancelled shell/MCP execution whose stop is
//!   not confirmed must stay `result_unknown`, never a deterministic cancel success.
//! * `cancel_reaps_process_group_and_records_one_turn_terminal` — a confirmed cancel
//!   reaps the process group and the run records exactly one terminal event; the
//!   behavioural fixtures referenced here are the existing
//!   `cancel_stops_in_flight_shell_before_it_writes` and
//!   `cancelling_mid_stream_never_completes_or_emits_a_late_delta` daemon tests.
//!
//! These are source guards only: H08 proof ceiling is `source`, and the behavioural
//! daemon fixtures run in the same focused GitHub workflow lane.

#[test]
fn unconfirmed_tool_stop_is_unknown() {
    let shell = include_str!("../src/harness_capabilities.rs");
    let mcp_stdio = include_str!("../src/mcp_stdio.rs");
    let mcp_harness = include_str!("../src/harness_mcp.rs");
    let core_caps = include_str!("../../kiana-core/src/capabilities.rs");
    let lifecycle = include_str!("../../kiana-core/src/lifecycle.rs");

    // A cancelled shell that already started may only resolve to `cancelled:*` when the
    // supervisor actually confirmed the stop; otherwise the result is Unknown.
    assert!(shell.contains("shell_result_unknown:cancel_stop_unconfirmed"));
    assert!(shell.contains("shell_result_unknown:process_group_not_stopped"));
    assert!(shell.contains("if !report.confirmed"));
    assert!(shell.contains("ProcessSupervisor::stop"));
    assert!(mcp_stdio.contains("result_unknown:mcp_stop_unconfirmed"));
    assert!(mcp_harness.contains("needs_reconciliation"));
    assert!(mcp_harness.contains("stop_confirmed"));

    // Core keeps the three-state shape: requested cancel, confirmed stop, unknown result.
    assert!(core_caps.contains("result_unknown:cancel_stop_unconfirmed"));
    assert!(core_caps.contains("await_capability_stop"));
    assert!(core_caps.contains("cancelled:user"));
    // A handler may legitimately expose a `stop_confirmed` output field; that is not
    // durable cancellation evidence unless the ControlPlane cancellation watch is set.
    assert!(core_caps.contains("!*cancellation.borrow() && receipt.stop_confirmed.is_some()"));
    assert!(lifecycle.contains("run.result_unknown"));
    assert!(lifecycle.contains("run.cancelled"));

    for forbidden in [
        "cancelled_without_stop_confirmation_is_success",
        "unconfirmed_stop_is_cancelled_success",
        "unknown_effect_is_success",
    ] {
        assert!(
            !shell.contains(forbidden)
                && !mcp_stdio.contains(forbidden)
                && !mcp_harness.contains(forbidden)
                && !core_caps.contains(forbidden),
            "forbidden H08 unconfirmed-stop success: {forbidden}"
        );
    }
}

#[test]
fn cancel_reaps_process_group_and_records_one_turn_terminal() {
    let shell = include_str!("../src/harness_capabilities.rs");
    let supervisor = include_str!("../src/process_supervisor.rs");
    let execution = include_str!("../src/execution_control.rs");
    let daemon_host = include_str!("daemon_host.rs");
    let core_caps = include_str!("../../kiana-core/src/capabilities.rs");
    let lifecycle = include_str!("../../kiana-core/src/lifecycle.rs");

    for marker in [
        "ProcessSupervisor::prepare_command",
        "ProcessSupervisor::stop",
        "TERM_GRACE",
        "leader_reaped",
        "process_group_exists",
        "stop_confirmed",
    ] {
        assert!(
            shell.contains(marker)
                || supervisor.contains(marker)
                || execution.contains(marker),
            "H08 process-group reap marker missing: {marker}"
        );
    }
    assert!(execution.contains("process_group_id"));

    // The success path is carried by the existing daemon fixtures: a confirmed cancel
    // stops the in-flight shell before it writes, and the mid-stream cancel records a
    // single terminal event with no late delta.
    assert!(daemon_host.contains("cancel_stops_in_flight_shell_before_it_writes"));
    assert!(daemon_host.contains("cancelling_mid_stream_never_completes_or_emits_a_late_delta"));
    assert!(daemon_host.contains("terminal_count != 1"));
    assert!(lifecycle.contains("record_terminal_event"));

    for forbidden in [
        "cancelled_without_stop_confirmation_is_success",
        "drop_child_without_reap",
        "late_delta_after_terminal",
    ] {
        assert!(
            !shell.contains(forbidden)
                && !supervisor.contains(forbidden)
                && !execution.contains(forbidden)
                && !daemon_host.contains(forbidden),
            "forbidden H08 cancel-success bypass: {forbidden}"
        );
    }
}
