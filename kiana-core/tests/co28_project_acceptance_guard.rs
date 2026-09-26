#[test]
fn project_acceptance_requires_all_milestones_and_human_waiver() {
    let acceptance = include_str!("../../kiana-domain/src/project_acceptance.rs");
    let review = include_str!("../../kiana-domain/src/company_review.rs");
    let closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let core = include_str!("../src/project_acceptance.rs");

    for marker in [
        "PROJECT_ACCEPTANCE_SCHEMA",
        "ProjectAcceptanceRequest",
        "required_milestone_acceptance_ids",
        "milestone_acceptance_digests",
        "project_acceptance_milestone_digest_invalid",
        "project_acceptance_acceptor_not_independent",
        "project_acceptance_waiver_requires_sponsor",
        "ProjectAcceptanceLedger",
        "record_project_acceptance",
        "AcceptanceTarget::Project",
        "ClosingReceipt",
        "CompanyClosingReceipt",
    ] {
        assert!(
            acceptance.contains(marker)
                || review.contains(marker)
                || closeout.contains(marker)
                || core.contains(marker),
            "CO-28 marker missing: {marker}"
        );
    }
    assert!(!acceptance.contains("CapabilityBroker"));
    assert!(!acceptance.contains("Runner"));
}
