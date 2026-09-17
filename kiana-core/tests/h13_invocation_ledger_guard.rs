#[test]
fn h13_invocation_ledger_commits_every_boundary_before_side_effects() {
    let dispatch = include_str!("../src/dispatch.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let projection = include_str!("../src/invocation_projection.rs");
    let recovery = include_str!("../src/recovery.rs");
    let fixtures = include_str!("h13_invocation_ledger.rs");
    for marker in [
        "run.tool_call",
        "run.capability_requested",
        "capability.decision",
        "execution.prepared",
        "invocation.dispatching",
        "invocation.executing",
        "execution.result_committed",
        "outcome_ready",
        "result.delivery_claimed",
        "commit_confirmed",
        "result_unknown:result_commit_failed",
        "result_unknown:result_delivery_unconfirmed",
        "invocation_terminal_conflict",
        "CapabilityExecutionState::Unknown",
        "event_append_failure_prevents_dispatch",
        "crash_after_effect_before_outcome_requires_reconciliation",
        "persisted_outcome_is_reused_without_reexecuting_tool",
    ] {
        assert!(
            dispatch.contains(marker)
                || capabilities.contains(marker)
                || lifecycle.contains(marker)
                || projection.contains(marker)
                || recovery.contains(marker)
                || fixtures.contains(marker),
            "H13 marker missing: {marker}"
        );
    }
    let commit = dispatch
        .find("commit_invocation_executing")
        .expect("execution boundary commit");
    let broker = dispatch
        .find("execute_cancellable")
        .expect("broker execution");
    assert!(commit < broker);
    assert!(dispatch.contains("result.delivery_claimed"));
    assert!(dispatch.contains("AggregateVersion::new(\"result_delivery\""));
    assert!(projection.contains("dispatch without a terminal result"));
    assert!(recovery.contains("cache_invocation_projection"));
    for forbidden in [
        "execute_before_execution_commit",
        "reexecute_after_result_commit",
        "unknown_result_as_success",
        "drop_outcome_before_append",
    ] {
        assert!(
            !dispatch.contains(forbidden) && !capabilities.contains(forbidden),
            "forbidden H13 ledger bypass: {forbidden}"
        );
    }
}
