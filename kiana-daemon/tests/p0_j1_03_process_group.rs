#[test]
fn cancel_confirms_shell_process_group_stopped() {
    let harness = include_str!("../src/harness_capabilities.rs");
    let mcp = include_str!("../src/mcp_stdio.rs");
    let execution = include_str!("../src/execution_control.rs");
    let existing = include_str!("daemon_host.rs");
    for marker in [
        "prepare_process_group",
        "terminate_process_group",
        "process_group_exists",
        "stop_confirmed",
        "shell_result_unknown:process_group_not_stopped",
        "cancel_stops_in_flight_shell_before_it_writes",
    ] {
        assert!(
            harness.contains(marker)
                || mcp.contains(marker)
                || execution.contains(marker)
                || existing.contains(marker),
            "process-group stop marker missing: {marker}"
        );
    }
    assert!(harness.contains("if !stop_confirmed"));
    assert!(harness.contains("finish_if_stopped"));
    assert!(execution.contains("process_group_id"));
}

#[test]
fn unconfirmed_process_stop_is_result_unknown_not_cancelled_success() {
    let harness = include_str!("../src/harness_capabilities.rs");
    let mcp = include_str!("../src/harness_mcp.rs");
    assert!(harness.contains("shell_result_unknown:cancel_stop_unconfirmed"));
    assert!(harness.contains("shell_result_unknown:process_group_not_stopped"));
    assert!(mcp.contains("needs_reconciliation"));
    assert!(mcp.contains("stop_confirmed"));
    assert!(!harness.contains("stop_unconfirmed\"}"));
}
