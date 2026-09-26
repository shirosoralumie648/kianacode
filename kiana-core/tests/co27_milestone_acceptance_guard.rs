#[test]
fn milestone_acceptance_is_local_and_does_not_reintroduce_project_waiting() {
    let milestone = include_str!("../../kiana-domain/src/milestone_acceptance.rs");
    let core = include_str!("../src/milestone_acceptance.rs");
    let company = include_str!("../src/company.rs");
    let readiness = include_str!("../../kiana-domain/src/company_readiness.rs");

    for marker in [
        "MILESTONE_ACCEPTANCE_SCHEMA",
        "MilestoneAcceptanceRequest",
        "required_packet_ids",
        "accepted_packet_ids",
        "packet_acceptance_digests",
        "milestone_acceptance_packet_missing",
        "milestone_acceptance_foreign_evidence",
        "MilestoneAcceptanceLedger",
        "record_milestone_acceptance",
        "AcceptanceTarget::Milestone",
        "MilestoneStatus::Accepted",
        "project_company_readiness",
        "ready_packets",
    ] {
        assert!(
            milestone.contains(marker)
                || core.contains(marker)
                || company.contains(marker)
                || readiness.contains(marker),
            "CO-27 marker missing: {marker}"
        );
    }
    assert!(!milestone.contains("values_mut"));
    assert!(!milestone.contains("CapabilityBroker"));
}
