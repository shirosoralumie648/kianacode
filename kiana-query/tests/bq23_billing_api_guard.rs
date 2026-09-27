#[test]
fn billing_query_is_read_only_and_does_not_widen_authority() {
    let source = include_str!("../src/billing_api.rs");
    for marker in [
        "query_billing_projection",
        "BillingQueryKind::BudgetSummary",
        "BillingQueryKind::UsageBreakdown",
        "BillingQueryKind::CostExport",
        "BillingQueryKind::ReconciliationInbox",
        "CursorProjectionMismatch",
        "QueryDataDisposition::Allowed",
    ] {
        assert!(source.contains(marker), "BQ-23 marker missing: {marker}");
    }
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "ApprovalStore",
        "append(",
        "reserve(",
        "consume_approval",
        "release_reservation",
    ] {
        assert!(
            !source.contains(forbidden),
            "BQ-23 query widened: {forbidden}"
        );
    }
}
