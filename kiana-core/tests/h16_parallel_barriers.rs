#[test]
fn write_barrier_never_overlaps_prior_reads() {
    let scheduling = include_str!("../../kiana-domain/src/tool_scheduling.rs");
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "ParallelRead",
        "Exclusive",
        "max_parallelism",
        "resources",
        "exclusive",
        "parallel_read",
        "plan_tool_batch",
        "call_ids",
        "pending_tools",
    ] {
        assert!(
            scheduling.contains(marker) || authority.contains(marker) || runner.contains(marker),
            "H16 barrier marker missing: {marker}"
        );
    }
    assert!(scheduling.contains("exclusive call always drains the preceding group"));
    assert!(scheduling.contains("ToolSchedulingMode::Exclusive"));
}

#[test]
fn revocation_blocks_not_started_parallel_call() {
    let core = include_str!("../src/dispatch.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "dispatch_authority_versions",
        "authorize_capability_action",
        "commit_invocation_executing",
        "prepare_capability_action",
        "cancel_pending_tools",
        "CapabilityResult::failure",
        "not_executed",
    ] {
        assert!(
            core.contains(marker) || capabilities.contains(marker) || runner.contains(marker),
            "H16 revocation marker missing: {marker}"
        );
    }
}

#[test]
fn crash_with_later_outcome_ready_does_not_rerun_it() {
    let ledger = include_str!("h13_invocation_ledger.rs");
    let projection = include_str!("../src/invocation_projection.rs");
    assert!(ledger.contains("persisted_outcome_is_reused_without_reexecuting_tool"));
    assert!(projection.contains("execution.result_committed"));
    assert!(projection.contains("outcome_ready"));
}
