#[test]
fn er15_adapters_share_bounded_commit_and_stop_boundary() {
    let domain = include_str!("../../kiana-domain/src/adapter_result.rs");
    let shell_patch = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let hooks = include_str!("../../kiana-daemon/src/pre_tool_hooks.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let combined = [domain, shell_patch, memory, mcp, hooks, patch].join("\n");

    for marker in [
        "kiana.adapter-result.v1",
        "AdapterResultKind",
        "AdapterCommitState",
        "attach_adapter_result",
        "output_digest",
        "evidence_ref_digests",
        "reconciliation_required",
        "effect_known",
        "stop_confirmed",
        "ADAPTER_RESULT_MAX_OUTPUT_BYTES",
    ] {
        assert!(combined.contains(marker), "ER-15 marker missing: {marker}");
    }
    for adapter in ["AdapterResultKind::Shell", "AdapterResultKind::Patch"] {
        assert!(
            shell_patch.contains(adapter),
            "shell/patch missing {adapter}"
        );
    }
    assert!(memory.matches("AdapterResultKind::Memory").count() >= 3);
    assert!(mcp.matches("AdapterResultKind::Mcp").count() >= 3);
    assert!(hooks.contains("AdapterResultKind::Hook"));

    // Negative boundaries named by ER-15 remain explicit in their owning adapter.
    assert!(hooks.contains("hook_update_input_unsupported"));
    assert!(mcp.contains("mcp_call_unconfirmed"));
    assert!(mcp.contains("retain_for_reconciliation"));
    assert!(memory.contains("result_unknown:memory_write_unconfirmed"));
    assert!(patch.contains("result_unknown:apply_patch_rollback_failed"));
    assert!(patch.contains("PatchTransaction::begin"));

    for forbidden in [
        "retry_without_idempotency",
        "mcp_disconnect_is_success",
        "memory_write_cancelled_success",
        "patch_partial_commit_completed",
    ] {
        assert!(
            !combined.contains(forbidden),
            "forbidden ER-15 path: {forbidden}"
        );
    }
}
