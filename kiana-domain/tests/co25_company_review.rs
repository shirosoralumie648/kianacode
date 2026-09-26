use kiana_domain::*;
use std::collections::BTreeMap;

fn assignment() -> ReviewerAssignment {
    let mut assignment = ReviewerAssignment {
        schema: COMPANY_REVIEW_ASSIGNMENT_SCHEMA.to_owned(),
        assignment_id: "review-assignment-1".to_owned(),
        project_id: "project-1".to_owned(),
        packet_id: "packet-1".to_owned(),
        packet_version: 2,
        baseline_version: 3,
        author_principal_ids: vec!["builder-principal".to_owned()],
        author_session_ids: vec!["builder-session".to_owned()],
        reviewer_principal_id: "reviewer-principal".to_owned(),
        reviewer_session_id: "reviewer-session".to_owned(),
        reviewer_role: "reviewer".to_owned(),
        reviewer_role_instance_id: "reviewer-role-instance".to_owned(),
        criteria_snapshot_digest: format!("sha256:{}", "a".repeat(64)),
        evidence_bundle_digest: format!("sha256:{}", "b".repeat(64)),
        digest: String::new(),
    };
    assignment.digest = assignment.canonical_digest();
    assignment
}

fn review() -> IndependentReview {
    let mut review = IndependentReview {
        schema: COMPANY_INDEPENDENT_REVIEW_SCHEMA.to_owned(),
        review_id: "review-1".to_owned(),
        assignment: assignment(),
        criterion_results: BTreeMap::from([(
            "criterion-1".to_owned(),
            CriterionReviewResult {
                verdict: ReviewVerdict::Pass,
                evidence_refs: vec!["evidence:bundle-1".to_owned()],
                reason: "receipt and output match".to_owned(),
                waiver_ref: None,
            },
        )]),
        completed_at: 100,
        digest: String::new(),
    };
    review.digest = review.canonical_digest();
    review
}

#[test]
fn review_denies_every_author_even_after_session_rotation() {
    let mut same_session = review();
    same_session.assignment.reviewer_session_id = "builder-session".to_owned();
    same_session.assignment.digest = same_session.assignment.canonical_digest();
    same_session.digest = same_session.canonical_digest();
    assert_eq!(
        same_session.validate().unwrap_err(),
        "company_reviewer_session_not_independent"
    );

    let mut duplicate_author = review();
    duplicate_author.assignment.author_session_ids =
        vec!["builder-session".to_owned(), "builder-session".to_owned()];
    duplicate_author.assignment.digest = duplicate_author.assignment.canonical_digest();
    duplicate_author.digest = duplicate_author.canonical_digest();
    assert_eq!(
        duplicate_author.validate().unwrap_err(),
        "company_reviewer_author_session_duplicate"
    );
}

#[test]
fn independent_review_records_per_criterion_evidence_and_missing_coverage() {
    let mut ledger = IndependentReviewLedger::default();
    let review = review();
    ledger.record(review.clone()).expect("record");
    ledger.record(review.clone()).expect("idempotent");
    assert_eq!(ledger.reviews.len(), 1);

    let mut not_applicable = review.clone();
    not_applicable.review_id = "review-2".to_owned();
    not_applicable
        .criterion_results
        .get_mut("criterion-1")
        .unwrap()
        .verdict = ReviewVerdict::NotApplicable;
    not_applicable.digest = not_applicable.canonical_digest();
    assert_eq!(
        not_applicable.validate().unwrap_err(),
        "company_review_not_applicable_waiver_required"
    );

    let mut missing_evidence = review;
    missing_evidence.review_id = "review-3".to_owned();
    missing_evidence
        .criterion_results
        .get_mut("criterion-1")
        .unwrap()
        .evidence_refs
        .clear();
    missing_evidence.digest = missing_evidence.canonical_digest();
    assert_eq!(
        missing_evidence.validate().unwrap_err(),
        "company_review_evidence_refs_required"
    );
}

#[test]
fn reviewer_assignment_and_review_reopen_without_authority_widening() {
    let review = review();
    let reopened: IndependentReview =
        serde_json::from_value(serde_json::to_value(&review).expect("serialize")).expect("reopen");
    assert_eq!(reopened, review);
    assert_eq!(reopened.assignment.reviewer_role, "reviewer");
    assert!(reopened.validate().is_ok());
}
