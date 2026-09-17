#[test]
fn reviewer_cannot_rewrite_builder_facts() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let company_business = include_str!("../src/company_business.rs");
    let company = include_str!("../src/company.rs");
    let governance = include_str!("../src/company_governance.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let baseline = include_str!("../../docs/roadmap/p3-i04-acceptance-review-baseline.md");

    for marker in [
        "CriteriaSnapshot",
        "criteria_snapshot",
        "Acceptance",
        "CompanyReview",
        "RequestAcceptance",
        "RecordReview",
        "DecideAcceptance",
        "AcceptanceStatus::Requested",
        "AcceptanceStatus::ReadyForDecision",
        "review_author_session_denied",
        "review_author_evidence_required",
        "review_criteria_snapshot_mismatch",
        "acceptance_review_identity_mismatch",
        "acceptance_reviewer_session_mismatch",
        "acceptance_independent_review_required",
        "author_session_id",
        "reviewer_session_id",
        "criterion_results",
        "evidence_refs",
        "project_version",
        "milestone_version",
        "packet_version",
        "project_criteria",
        "milestone_criteria",
        "packet_criteria",
        "reviewer_cannot_rewrite_builder_facts",
        "CompanyGovernance",
        "review_author_session_overlap",
        "company_business_proof",
        "independent",
        "source_event_ids",
    ] {
        assert!(
            domain.contains(marker)
                || company_business.contains(marker)
                || company.contains(marker)
                || governance.contains(marker)
                || collaboration.contains(marker)
                || baseline.contains(marker),
            "acceptance review marker missing: {marker}"
        );
    }

    assert!(domain.contains("a.session_id != acceptance.author_session_id"));
    assert!(domain.contains("review.criteria_snapshot == acceptance.criteria_snapshot"));
    assert!(domain.contains("review.author_run_id == acceptance.author_run_id"));
    assert!(domain.contains("criterion_results.keys().cloned().collect::<Vec<_>>()"));
    assert!(company.contains("Self::evidence(p, evidence_refs)"));
    assert!(company_business.contains("Review this frozen business target independently"));
    assert!(governance.contains("acceptance.author_session_id == review.reviewer_session_id"));
    assert!(collaboration.contains("review_author_session_denied"));
    assert!(!company_business.contains("CapabilityBroker"));
    assert!(!company_business.contains("ModelClient"));
}
