use kiana_domain::*;

fn reservation(id: &str, org: &str, project: &str, amount: u64) -> CompanyCapacityReservation {
    let mut value = CompanyCapacityReservation {
        schema: COMPANY_PORTFOLIO_SCHEMA.to_owned(),
        reservation_id: id.to_owned(),
        organization_id: org.to_owned(),
        project_id: project.to_owned(),
        run_id: format!("run-{id}"),
        amount,
        priority: 1,
        authority_epoch: 7,
        state: CompanyCostState::Reserved,
        source_ref: format!("event:reserve-{id}"),
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn two_projects_share_capacity_fairly_with_independent_evidence_and_budgets() {
    let mut ledger = CompanyPortfolioLedger::default();
    ledger.organization_limits.insert("org-1".to_owned(), 150);
    ledger.project_limits.insert("project-a".to_owned(), 100);
    ledger.project_limits.insert("project-b".to_owned(), 100);
    ledger
        .reserve(reservation("a", "org-1", "project-a", 80))
        .expect("a");
    ledger
        .reserve(reservation("b", "org-1", "project-b", 70))
        .expect("b");
    assert_eq!(
        ledger
            .reserve(reservation("a2", "org-1", "project-a", 1))
            .unwrap_err(),
        "company_capacity_exceeded"
    );
    ledger
        .settle("a", CompanyCostState::Released, "event:release-a")
        .expect("release");
    ledger
        .reserve(reservation("a2", "org-1", "project-a", 1))
        .expect("fair capacity");
}

#[test]
fn organization_budget_cannot_be_bypassed_by_new_project_or_unknown_cost() {
    let mut ledger = CompanyPortfolioLedger::default();
    ledger.organization_limits.insert("org-1".to_owned(), 100);
    ledger.project_limits.insert("project-a".to_owned(), 100);
    ledger
        .reserve(reservation("a", "org-1", "project-a", 90))
        .expect("a");
    assert_eq!(
        ledger
            .reserve(reservation("new", "org-1", "project-new", 1))
            .unwrap_err(),
        "company_project_budget_missing"
    );
    ledger
        .settle("a", CompanyCostState::Unknown, "event:unknown-a")
        .expect("unknown");
    assert_eq!(
        ledger
            .reserve(reservation("a2", "org-1", "project-a", 11))
            .unwrap_err(),
        "company_capacity_exceeded"
    );
}

#[test]
fn duplicate_reservation_and_cross_organization_capacity_are_rejected() {
    let mut ledger = CompanyPortfolioLedger::default();
    ledger.organization_limits.insert("org-1".to_owned(), 100);
    ledger.project_limits.insert("project-a".to_owned(), 100);
    let first = reservation("a", "org-1", "project-a", 10);
    ledger.reserve(first.clone()).expect("first");
    ledger.reserve(first).expect("idempotent");
    let mut drift = reservation("a", "org-1", "project-a", 11);
    drift.digest = drift.canonical_digest();
    assert_eq!(
        ledger.reserve(drift).unwrap_err(),
        "company_capacity_duplicate_digest_mismatch"
    );
    assert_eq!(
        ledger
            .reserve(reservation("foreign", "org-2", "project-a", 1))
            .unwrap_err(),
        "company_organization_budget_missing"
    );
}
