#[test]
fn budget_intersection_keeps_five_scopes_separate_and_never_grants_financial_authority() {
    let budgets = include_str!("../../kiana-domain/src/billing_budgets.rs");
    let usage = include_str!("../../kiana-domain/src/usage.rs");
    for marker in [
        "ProviderBudget",
        "EffectiveBudget",
        "intersect_budgets",
        "derive_child_lease",
        "budget_child_widening_rejected",
        "max_project_runs",
        "provider_max_cost_micros",
        "reservation_limit_for_intersection",
    ] {
        assert!(budgets.contains(marker), "budget marker missing: {marker}");
    }
    assert!(usage.contains("RuntimeBudget"));
    assert!(usage.contains("ProjectBudget"));
    assert!(usage.contains("Quota"));
    for forbidden in [
        "FinancialBudget",
        "CapabilityBroker",
        "reqwest",
        "tokio::spawn",
        "std::fs",
    ] {
        assert!(
            !budgets.contains(forbidden),
            "budget authority widened: {forbidden}"
        );
    }
}
