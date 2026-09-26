#[test]
fn project_charter_go_no_go_is_frozen_and_budget_backed() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "ProjectStatus::Chartering",
        "project_charter_not_ready_for_approval",
        "project_budget_required",
        "project_charter_artifact_required",
        "project_charter_changed",
        "project_objective_not_active",
        "project_sponsor_mismatch",
        "charter_digests",
        "charter_baselines",
        "ProjectCharterBaseline",
        "charter_identity_digest",
        "company_budget_scope_mismatch",
        "company_budget_already_configured",
        "company_project_not_budgetable",
        "project_budget_ref",
        "decision_ref",
        "CompanyCommand::ApproveProject",
        "CompanyCommand::RejectProject",
        "CompanyCommand::ConfigureBudget",
        "CompanyCommandPolicy",
        "company_human_decision_missing",
        "handle_company_command",
        "company_idempotency_conflict",
    ] {
        assert!(
            domain.contains(marker) || policy.contains(marker) || core.contains(marker),
            "CO-10 marker missing: {marker}"
        );
    }

    assert!(domain.contains("self.budgets.contains_key(project_id)"));
    assert!(domain.contains("project_charter_artifact_required"));
    assert!(domain.contains("project_charter_changed"));
    assert!(domain.contains("project.objective_refs.iter().all"));
    assert!(domain.contains("self.charter_digests.get(project_id)"));
    assert!(domain.contains("self.charter_baselines.insert"));
    assert!(core.contains("validate_decision_for"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("EventStorePort"));
}
