#[test]
fn bq13_cost_receipt_contract_is_pinned_and_read_only() {
    let domain = include_str!("../../kiana-domain/src/billing_cost.rs");
    let receipts = include_str!("../src/receipts.rs");
    let query = include_str!("../../kiana-query/src/cost_projector.rs");
    for marker in [
        "CostBreakdown",
        "ReceiptCostBreakdown",
        "CostBreakdownKind::Estimated",
        "CostBreakdownKind::Measured",
        "CostBreakdownKind::Unknown",
        "rate_card_version",
        "provider_receipt",
        "unknown_reasons",
        "project_receipt_cost_breakdown",
        "cost_breakdown",
    ] {
        assert!(
            domain.contains(marker) || receipts.contains(marker) || query.contains(marker),
            "BQ-13 source marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBrokerPort",
        "EventStorePort",
        "tokio::spawn",
        "reqwest",
        "commit_transition",
        "FinancialBudget",
        "cost = 0",
        "estimate_settle",
    ] {
        assert!(
            !domain.contains(forbidden),
            "domain authority widened: {forbidden}"
        );
        assert!(
            !query.contains(forbidden),
            "query authority widened: {forbidden}"
        );
    }
}
