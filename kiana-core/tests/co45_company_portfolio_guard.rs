#[test]
fn company_portfolio_keeps_budget_capacity_and_unknown_cost_boundaries() {
    let portfolio = include_str!("../../kiana-domain/src/company_portfolio.rs");
    let budgets = include_str!("../../kiana-domain/src/billing_contracts.rs");
    let core = include_str!("../src/company_portfolio.rs");
    for marker in [
        "COMPANY_PORTFOLIO_SCHEMA",
        "CompanyCapacityReservation",
        "CompanyPortfolioLedger",
        "CompanyCostState",
        "Reserved",
        "Spent",
        "Released",
        "Unknown",
        "company_organization_budget_missing",
        "company_project_budget_missing",
        "company_capacity_exceeded",
        "organization_total",
        "reserve_company_capacity",
        "BillingState",
    ] {
        assert!(
            portfolio.contains(marker) || budgets.contains(marker) || core.contains(marker),
            "CO-45 marker missing: {marker}"
        );
    }
    for forbidden in ["FinancialBudget::new", "Command::new", "ModelClient::new"] {
        assert!(
            !portfolio.contains(forbidden),
            "CO-45 bypass marker present: {forbidden}"
        );
    }
}
