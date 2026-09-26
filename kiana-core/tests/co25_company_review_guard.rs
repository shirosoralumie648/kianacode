#[test]
fn independent_review_uses_frozen_assignment_and_criterion_evidence() {
    let review = include_str!("../../kiana-domain/src/company_review.rs");
    let core = include_str!("../src/company_review.rs");
    let company = include_str!("../src/company.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let business = include_str!("../../kiana-domain/src/company_business.rs");

    for marker in [
        "COMPANY_REVIEW_ASSIGNMENT_SCHEMA",
        "COMPANY_INDEPENDENT_REVIEW_SCHEMA",
        "ReviewerAssignment",
        "author_session_ids",
        "reviewer_session_id",
        "reviewer_role_instance_id",
        "criteria_snapshot_digest",
        "evidence_bundle_digest",
        "ReviewVerdict::NotApplicable",
        "company_review_not_applicable_waiver_required",
        "company_review_evidence_refs_required",
        "company_reviewer_session_not_independent",
        "IndependentReviewLedger",
        "record_independent_review",
        "RecordReview",
        "review_author_run",
    ] {
        assert!(
            review.contains(marker)
                || core.contains(marker)
                || company.contains(marker)
                || collaboration.contains(marker)
                || business.contains(marker),
            "CO-25 marker missing: {marker}"
        );
    }
    assert!(review.contains("criterion_results"));
    assert!(!review.contains("CapabilityBroker"));
    assert!(!review.contains("Runner"));
}
