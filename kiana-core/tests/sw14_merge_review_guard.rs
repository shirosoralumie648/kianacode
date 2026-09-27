#[test]
fn swarm_merge_review_keeps_independence_and_company_boundary() {
    let domain = include_str!("../../kiana-domain/src/swarm_merge_review.rs");
    let core = include_str!("../src/swarm_merge_review.rs");
    for marker in [
        "SwarmPartitionReview",
        "reviewer_id",
        "author_id",
        "company_acceptance_required",
        "conflict_refs",
        "validate_swarm_review",
        "validate_swarm_receipt",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-14 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "self_review_auto_accept",
        "company_accept",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-14 review widened authority: {forbidden}"
        );
    }
}
