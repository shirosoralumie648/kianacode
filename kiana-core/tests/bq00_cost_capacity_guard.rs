#[test]
fn billing_quota_cost_baseline_covers_current_sources_and_conflicts() {
    let usage = include_str!("../../kiana-domain/src/usage.rs");
    let budget = include_str!("../src/model_budget.rs");
    let receipts = include_str!("../src/receipts.rs");
    let cell_registry = include_str!("../src/cell_registry.rs");
    let baseline = include_str!("../../docs/roadmap/billing-quota-cost-baseline.md");
    for marker in [
        "UsageRecord",
        "CostLedger",
        "cost_micros",
        "RuntimeBudget",
        "ProjectBudget",
        "Quota",
        "BudgetReservationFact",
        "BudgetSettlementFact",
        "reserve_prepared",
        "cost_ledger_from_events",
        "CellRegistryPort",
        "result_unknown",
    ] {
        assert!(
            usage.contains(marker)
                || budget.contains(marker)
                || receipts.contains(marker)
                || cell_registry.contains(marker)
                || baseline.contains(marker),
            "billing baseline marker missing: {marker}"
        );
    }
    for marker in [
        "UsageVector",
        "NormalizedUsage",
        "RateCard",
        "QuotaReservation",
        "CostCorrection",
        "BQ-01",
        "BQ-30",
        "missing usage is not zero",
    ] {
        assert!(
            baseline.contains(marker),
            "migration marker missing: {marker}"
        );
    }
    assert!(usage.contains("cost_micros: None"));
    assert!(baseline.contains("feature_status=partial"));
    assert!(baseline.contains("proof_level=source"));
}
