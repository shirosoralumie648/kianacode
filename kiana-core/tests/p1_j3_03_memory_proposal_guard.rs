#[test]
fn extraction_proposals_are_evidence_bound_and_three_tiered() {
    let domain = include_str!("../../kiana-domain/src/memory_proposals.rs");
    let distillation = include_str!("../../kiana-domain/src/memory_distillation.rs");
    let core = include_str!("../src/memory_distillation.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_memory.rs");
    for marker in [
        "MEMORY_PROPOSAL_SCHEMA",
        "MemoryEvidence",
        "similar_records",
        "memory.proposed",
        "memory.extraction_incident",
        "MemoryAdmission::Candidate",
        "MemoryAdmission::Qualified",
        "MemoryAdmission::Rejected",
        "MemoryState::Draft",
        "MemoryState::Active",
        "MemoryReviewHandler",
        "accept_proposal",
    ] {
        assert!(
            domain.contains(marker)
                || distillation.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker),
            "memory proposal marker missing: {marker}"
        );
    }
    assert!(domain.contains("fact.evidence.is_empty()"));
    assert!(domain.contains("fact.similar_records.len() > 3"));
    assert!(core.contains("queue_memory_distillation"));
    assert!(daemon.contains("proposal_authorized"));
}
