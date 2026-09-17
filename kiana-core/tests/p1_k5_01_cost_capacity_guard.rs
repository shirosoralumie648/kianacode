#[test]
fn runtime_and_project_budgets_are_not_interchangeable() {
    let usage = include_str!("../../kiana-domain/src/usage.rs");
    let budget_facts = include_str!("../../kiana-domain/src/budget_contracts.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let model_budget = include_str!("../src/model_budget.rs");
    let cell_registry = include_str!("../src/cell_registry.rs");
    let runner_budget = include_str!("../../kiana-runner/src/budget.rs");
    let receipt = include_str!("../src/receipts.rs");
    let baseline = include_str!("../../docs/roadmap/p1-k5-01-cost-capacity-baseline.md");
    for marker in [
        "UsageRecord",
        "CostLedger",
        "cost_micros: None",
        "RuntimeBudget",
        "ProjectBudget",
        "Quota",
        "check_reservation",
        "BudgetReservationFact",
        "BudgetSettlementFact",
        "BudgetScope",
        "reserve_prepared",
        "consume_model_call",
        "settle_attempt",
        "usage_known",
        "unknown_attempts",
        "runtime_budget_for_run",
        "ProjectBudget",
        "cost_ledger_from_events",
        "source_cursor",
    ] {
        assert!(
            usage.contains(marker)
                || budget_facts.contains(marker)
                || company.contains(marker)
                || model_budget.contains(marker)
                || cell_registry.contains(marker)
                || runner_budget.contains(marker)
                || receipt.contains(marker)
                || baseline.contains(marker),
            "cost/capacity marker missing: {marker}"
        );
    }
    assert!(company.contains("project: policy.project"));
    assert!(company.contains("runtime: policy.runtime"));
    assert!(company.contains("quota: policy.quota"));
    assert!(model_budget.contains("let limit: RuntimeBudget"));
    assert!(receipt.contains("CostLedger::from_records"));
    assert!(!runner_budget.contains("cost_micros"));
    assert!(!usage.contains("refund_unknown_usage"));
}
