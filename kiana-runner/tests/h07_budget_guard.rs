#[test]
fn h07_budget_is_shared_and_reserved_before_effects() {
    let budget = include_str!("../src/budget.rs");
    let harness = include_str!("../src/harness.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/usage.rs");
    let facts = include_str!("../../kiana-domain/src/budget_contracts.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "HarnessBudgetConfig",
        "max_attempts_per_task",
        "max_tool_calls_per_task",
        "max_repairs_per_task",
        "max_compactions_per_task",
        "max_tokens_per_task",
        "max_wall_time_per_task",
        "reserve_attempt",
        "settle_attempt",
        "reserve_tool_call",
        "reserve_repair",
        "reserve_compaction",
        "model_budget",
        "reserve_prepared",
        "usage_known",
        "unknown_attempts",
        "KIANA_HARNESS_MAX_ATTEMPTS",
        "KIANA_HARNESS_MAX_TOKENS",
        "runtime_config_invalid",
    ] {
        assert!(
            budget.contains(marker)
                || harness.contains(marker)
                || daemon.contains(marker)
                || domain.contains(marker)
                || facts.contains(marker)
                || ports.contains(marker),
            "H07 marker missing: {marker}"
        );
    }
    let reserve = harness
        .find("reserve_attempt(&budget_scope")
        .expect("model attempt must reserve before provider");
    let provider = harness
        .find("complete_prepared(prepared")
        .expect("model provider call must remain visible");
    assert!(reserve < provider);
    assert!(harness.contains("settle_attempt(reservation, measured)"));
    assert!(harness.contains("budget_scope"));
    for forbidden in [
        "reset_task_chain_budget",
        "refund_unknown_usage",
        "provider_retry_free",
        "call_provider_before_reserve",
    ] {
        assert!(
            !budget.contains(forbidden) && !harness.contains(forbidden),
            "forbidden H07 budget bypass: {forbidden}"
        );
    }
}
