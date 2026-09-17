#[test]
fn cp11_budget_is_one_model_tool_ledger_with_conservative_unknown_usage() {
    let domain = include_str!("../../kiana-domain/src/budget_contracts.rs");
    let lease = include_str!("../../kiana-domain/src/work_packets.rs");
    let cell = include_str!("../src/cell_registry.rs");
    let model = include_str!("../src/model_budget.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "BudgetScope",
        "BudgetReservationFact",
        "BudgetSettlementFact",
        "model_calls",
        "tool_calls",
        "parent_reservation_id",
        "consume_model_call",
        "account_model_usage",
        "usage_known",
        "model_budget_exhausted_before_provider",
        "reserve_prepared",
        "execution_id",
    ] {
        assert!(
            domain.contains(marker)
                || lease.contains(marker)
                || cell.contains(marker)
                || model.contains(marker)
                || runner.contains(marker)
                || ports.contains(marker),
            "CP-11 marker missing: {marker}"
        );
    }
    assert!(cell.contains("consume_model_call"));
    assert!(model.contains("reservation_fact"));
    assert!(model.contains("settlement_fact"));
    assert!(model.contains("model_budget_duplicate_settlement"));
    for forbidden in ["default_allow", "refund_unknown_usage", "unbounded_budget"] {
        assert!(
            !domain.contains(forbidden) && !cell.contains(forbidden) && !model.contains(forbidden),
            "budget boundary must not enable {forbidden}"
        );
    }
}
