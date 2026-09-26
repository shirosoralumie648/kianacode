#[test]
fn symposium_governance_binds_assignment_baseline_and_decision_evidence() {
    let governance = include_str!("../../kiana-domain/src/symposium_governance.rs");
    let existing = include_str!("../../kiana-domain/src/symposiums.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "GOVERNED_SYMPOSIUM_SCHEMA",
        "SYMPOSIUM_CONTRIBUTION_SCHEMA",
        "SYMPOSIUM_DECISION_SCHEMA",
        "SymposiumGovernance",
        "SymposiumBoard",
        "SymposiumContributionKind::Claim",
        "SymposiumContributionKind::Vote",
        "SymposiumContributionKind::Draft",
        "PrivateBrief",
        "attendee_assignments",
        "target_baseline_version",
        "symposium_contributor_uninvited",
        "symposium_contribution_stale",
        "symposium_contribution_duplicate",
        "symposium_decision_chair_required",
        "attach_sponsor_approval",
        "sponsor_approval_ref",
        "dissent",
        "unresolved",
        "evidence_refs",
    ] {
        assert!(
            governance.contains(marker) || existing.contains(marker) || core.contains(marker),
            "CO-14 marker missing: {marker}"
        );
    }
    assert!(existing.contains("pub struct Symposium"));
    assert!(existing.contains("pub struct DecisionRecord"));
    assert!(!governance.contains("CapabilityBroker"));
    assert!(!governance.contains("EventStorePort"));
    assert!(!core.contains("SymposiumBoard::execute"));
}
