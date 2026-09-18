#[test]
fn cp16_handler_stop_and_file_commit_evidence_is_explicit() {
    let shell = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let stdio = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let process_fixture = include_str!("../../kiana-daemon/tests/p0_j1_03_process_group.rs");
    let race_fixture = include_str!("../../kiana-daemon/tests/p0_j1_04_cancel_race.rs");

    for marker in [
        "prepare_process_group",
        "terminate_process_group",
        "process_group_exists",
        "finish_if_stopped",
        "PROCESS_GROUP_TERM_GRACE",
        "PROCESS_GROUP_EXIT_GRACE",
        "IO_DRAIN_TIMEOUT",
        "kill_on_drop(true)",
        "stop_confirmed",
        "shell_result_unknown:cancel_stop_unconfirmed",
        "shell_result_unknown:process_group_not_stopped",
        "effect_started",
        "effect_known",
    ] {
        assert!(
            shell.contains(marker) || stdio.contains(marker),
            "CP-16 shell/MCP stop marker missing: {marker}"
        );
    }
    for marker in [
        "capture_preconditions",
        "verify_preconditions",
        "open_commit_directories",
        "reject_symlink_components",
        "reject_hardlink",
        "commit_planned",
        "rollback_patch_guarded",
        "result_unknown:apply_patch_rollback_failed",
        "renameat(",
        "O_NOFOLLOW",
    ] {
        assert!(
            patch.contains(marker),
            "CP-16 patch marker missing: {marker}"
        );
    }
    for marker in [
        "mcp_stop_unconfirmed",
        "needs_reconciliation",
        "mcp_call_unconfirmed",
        "mcp_workspace_retain_unconfirmed",
        "mcp_tool_schema_changed",
    ] {
        assert!(
            mcp.contains(marker) || stdio.contains(marker),
            "CP-16 MCP marker missing: {marker}"
        );
    }
    for marker in [
        "execute_cancellable",
        "cancelled:before_broker",
        "execution_permit_verifier_required",
        "handler.execute_cancellable",
    ] {
        assert!(
            broker.contains(marker),
            "CP-16 Broker marker missing: {marker}"
        );
    }
    for marker in [
        "Cancellation must report uncertainty",
        "execute_cancellable",
        "result_unknown:cancel_stop_unconfirmed",
    ] {
        assert!(
            ports.contains(marker),
            "CP-16 port stop marker missing: {marker}"
        );
    }
    for marker in ["--unshare-all", "--cap-drop", "--clearenv", "sandbox_env"] {
        assert!(
            sandbox.contains(marker),
            "CP-16 sandbox marker missing: {marker}"
        );
    }
    assert!(execution.contains("process_group_id"));
    assert!(process_fixture.contains("cancel_confirms_shell_process_group_stopped"));
    assert!(race_fixture.contains("cancelling_mid_stream_never_completes_or_emits_a_late_delta"));
    for source in [shell, patch, mcp, stdio, broker] {
        for forbidden in [
            "drop_child_without_reap",
            "cancelled_without_stop_confirmation_is_success",
        ] {
            assert!(
                !source.contains(forbidden),
                "CP-16 bypass marker: {forbidden}"
            );
        }
    }
}
