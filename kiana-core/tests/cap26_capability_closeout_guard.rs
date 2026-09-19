//! CAP-26 source guard for the CI-only five-tool plus Hook closeout.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-26 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn all_effectful_adapters_share_control_plane_broker_and_result_boundaries() {
    let entry_paths = include_str!("cp05_entry_paths_guard.rs");
    let core_caps = include_str!("../src/capabilities.rs");
    let core_dispatch = include_str!("../src/dispatch.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let shell_patch = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let hooks = include_str!("../../kiana-daemon/src/pre_tool_hooks.rs");
    let adapters = include_str!("er15_adapter_result_guard.rs");
    let smoke = include_str!("../../scripts/harness-golden-smoke.sh");
    let workbench_smoke = include_str!("../../scripts/v10-workbench-smoke.sh");
    let baseline = include_str!("../../docs/roadmap/cap26-capability-closeout-baseline.md");

    require(
        entry_paths,
        &[
            "prepare_capability_action",
            "authorize_capability_action",
            "dispatch_capability_action",
            "finalize_capability_action",
            "ExecutionPermitVerifierPort",
        ],
        "shared ControlPlane path",
    );
    require(
        core_caps,
        &[
            "prepare_capability_action_cancellable",
            "authorize_capability_action_cancellable",
            "dispatch_authorized",
            "finalize_capability_action",
            "begin_cell_capability_from_request",
            "finish_cell_capability",
        ],
        "capability lifecycle",
    );
    require(
        core_dispatch,
        &[
            "verify_and_consume",
            "execution.prepared",
            "execute_cancellable",
            "execution.result_committed",
        ],
        "permit dispatch",
    );
    require(
        daemon,
        &[
            "ControlPlane::with_pre_tool_hooks_and_runtime_config",
            "CapabilityBroker",
        ],
        "DaemonHost composition",
    );
    require(
        broker,
        &["CapabilityBroker", "register_static", "execute_cancellable"],
        "Broker",
    );
    require(
        shell_patch,
        &[
            "CapabilityKind::Process",
            "shell.exec",
            "apply_patch",
            "AdapterResultKind::Shell",
        ],
        "shell and patch adapters",
    );
    require(
        patch,
        &[
            "PatchTransaction",
            "workspace_transaction",
            "result_unknown",
        ],
        "patch transaction",
    );
    require(
        mcp,
        &[
            "mcp.call",
            "mcp.discover",
            "AdapterResultKind::Mcp",
            "mcp_call_unconfirmed",
        ],
        "MCP adapter",
    );
    require(
        memory,
        &[
            "memory.search",
            "memory.write",
            "AdapterResultKind::Memory",
            "memory_write_unconfirmed",
        ],
        "Memory adapter",
    );
    require(
        hooks,
        &[
            "PreToolHookDecision",
            "AdapterResultKind::Hook",
            "hook_update_input_unsupported",
        ],
        "Hook adapter",
    );
    require(
        adapters,
        &[
            "attach_adapter_result",
            "effect_known",
            "stop_confirmed",
            "reconciliation_required",
        ],
        "adapter result boundary",
    );
    require(
        smoke,
        &["run --packet", "files_changed", "GOLDEN_PATH.txt"],
        "golden smoke",
    );
    require(
        workbench_smoke,
        &[
            "workspace-write",
            "GOLDEN_PATH.txt",
            "workspace_write_requires_trusted_non_safe_profile",
        ],
        "workbench smoke",
    );
    require(
        baseline,
        &[
            "all_effectful_adapters_enforce_the_same_scope_contract",
            "capability_crash_windows_never_create_duplicate_effects",
            "product_entrypoints_cannot_reach_host_execution_bypass",
            "shell",
            "stdio MCP",
            "memory candidate",
            "Receipt",
        ],
        "CAP-26 acceptance card",
    );
    for source in [
        core_caps,
        core_dispatch,
        daemon,
        broker,
        shell_patch,
        patch,
        mcp,
        memory,
        hooks,
    ] {
        for forbidden in ["auto_approve", "autoapprove", "ModelClient::new"] {
            assert!(
                !source.contains(forbidden),
                "CAP-26 bypass marker present: {forbidden}"
            );
        }
    }
}

#[test]
fn closeout_keeps_negative_evidence_and_proof_ceiling_explicit() {
    let cap17 = include_str!("cap17_execution_set_cancel_guard.rs");
    let cap18 = include_str!("cap18_hook_execution_guard.rs");
    let cap20 = include_str!("cap20_mcp_trust_discovery_guard.rs");
    let cap21 = include_str!("cap21_mcp_bounded_transport_guard.rs");
    let cap24 = include_str!("cap24_invocation_recovery_guard.rs");
    let receipts = include_str!("../src/receipts.rs");
    let baseline = include_str!("../../docs/roadmap/cap26-capability-closeout-baseline.md");

    require(
        cap17,
        &["not_executed", "result_unknown", "StopReport"],
        "cancel gate",
    );
    require(
        cap18,
        &["hook_update_input_unsupported", "project_trusted"],
        "Hook gate",
    );
    require(
        cap20,
        &["mcp_project_untrusted", "mcp_transport_unsupported"],
        "MCP trust gate",
    );
    require(
        cap21,
        &["mcp_response_frame_limit", "mcp_call_unconfirmed"],
        "MCP transport gate",
    );
    require(
        cap24,
        &[
            "CapabilityExecutionState::Unknown",
            "automatic_retry_allowed:false",
        ],
        "recovery gate",
    );
    require(
        receipts,
        &[
            "receipt_owner_mismatch",
            "receipt_data_revoked",
            "run_terminal_conflict",
        ],
        "receipt gate",
    );
    require(
        baseline,
        &[
            "local_behavior",
            "live provider",
            "physical install",
            "GitHub-CI-only",
            "No local runtime tests",
        ],
        "proof ceiling",
    );
}
