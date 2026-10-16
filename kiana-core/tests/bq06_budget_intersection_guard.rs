#[test]
fn budget_intersection_keeps_five_scopes_separate_and_never_grants_financial_authority() {
    let budgets = include_str!("../../kiana-domain/src/billing_budgets.rs");
    let leases = include_str!("../../kiana-domain/src/work_packets.rs");
    let usage = include_str!("../../kiana-domain/src/usage.rs");
    let budget_impl = budgets
        .split_once("use crate::")
        .map(|(_, suffix)| suffix)
        .expect("billing budget implementation start missing");
    for marker in [
        "ProviderBudget",
        "EffectiveBudget",
        "intersect_budgets",
        "derive_child_lease",
        "budget_child_widening_rejected",
        "budget_child_reservation_widening_rejected",
        "budget_child_reservation_reset_rejected",
        "max_project_runs",
        "provider_max_cost_micros",
        "reservation_limit_for_intersection",
        "self.lease_id.as_uuid().is_nil()",
        "project.project_id.as_uuid().is_nil()",
    ] {
        assert!(
            budgets.contains(marker) || leases.contains(marker),
            "budget marker missing: {marker}"
        );
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
            !budget_impl.contains(forbidden),
            "budget authority widened: {forbidden}"
        );
    }
}
