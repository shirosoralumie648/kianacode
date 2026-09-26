#[test]
fn company_proposals_are_structured_preview_facts_with_atomic_approval() {
    let proposals = include_str!("../../kiana-domain/src/company_proposals.rs");
    let plan = include_str!("../../kiana-domain/src/plan.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "COMPANY_PLAN_PROPOSAL_SCHEMA",
        "COMPANY_PLAN_APPROVAL_SCHEMA",
        "CompanyPlanProposal",
        "CompanyPlanApproval",
        "CompanyProposalLedger",
        "proposal_digest",
        "expected_revision",
        "company_proposal_duplicate",
        "company_plan_approval_proposal_mismatch",
        "company_plan_approval_duplicate",
        "approved_plan",
        "canonical_digest",
        "PlanProposal",
        "ResultContract",
    ] {
        assert!(
            proposals.contains(marker) || plan.contains(marker) || core.contains(marker),
            "CO-16 marker missing: {marker}"
        );
    }
    assert!(!proposals.contains("serde_json::from_str::<CompanyCommand"));
    assert!(!proposals.contains("CapabilityBroker"));
    assert!(!proposals.contains("EventStorePort"));
}
