use kiana_domain::*;
use std::collections::BTreeMap;

fn request(decision: ProjectAcceptanceDecision) -> ProjectAcceptanceRequest {
    let mut assignment = ReviewerAssignment {
        schema: COMPANY_REVIEW_ASSIGNMENT_SCHEMA.to_owned(),
        assignment_id: "project-review-assignment".to_owned(),
        project_id: "project-1".to_owned(),
        packet_id: "project".to_owned(),
        packet_version: 1,
        baseline_version: 2,
        author_principal_ids: vec!["builder".to_owned()],
        author_session_ids: vec!["builder-session".to_owned()],
        reviewer_principal_id: "reviewer".to_owned(),
        reviewer_session_id: "reviewer-session".to_owned(),
        reviewer_role: "reviewer".to_owned(),
        reviewer_role_instance_id: "reviewer-instance".to_owned(),
        criteria_snapshot_digest: format!("sha256:{}", "a".repeat(64)),
        evidence_bundle_digest: format!("sha256:{}", "b".repeat(64)),
        digest: String::new(),
    };
    assignment.digest = assignment.canonical_digest();
    let mut review = IndependentReview {
        schema: COMPANY_INDEPENDENT_REVIEW_SCHEMA.to_owned(),
        review_id: "project-review".to_owned(),
        assignment,
        criterion_results: BTreeMap::from([(
            "project-criterion".to_owned(),
            CriterionReviewResult {
                verdict: ReviewVerdict::Pass,
                evidence_refs: vec!["evidence:project".to_owned()],
                reason: "all milestones reviewed".to_owned(),
                waiver_ref: None,
            },
        )]),
        completed_at: 90,
        digest: String::new(),
    };
    review.digest = review.canonical_digest();
    let mut request = ProjectAcceptanceRequest {
        schema: PROJECT_ACCEPTANCE_SCHEMA.to_owned(),
        acceptance_id: "project-acceptance-1".to_owned(),
        project_id: "project-1".to_owned(),
        project_version: 4,
        required_milestone_acceptance_ids: vec![
            "m1-acceptance".to_owned(),
            "m2-acceptance".to_owned(),
        ],
        milestone_acceptance_digests: BTreeMap::from([
            (
                "m1-acceptance".to_owned(),
                format!("sha256:{}", "c".repeat(64)),
            ),
            (
                "m2-acceptance".to_owned(),
                format!("sha256:{}", "d".repeat(64)),
            ),
        ]),
        review,
        criteria_refs: vec!["criterion:project".to_owned()],
        acceptor_assignment: "assignment:sponsor".to_owned(),
        acceptor_session: "sponsor-session".to_owned(),
        acceptor_role: "sponsor".to_owned(),
        decision,
        reasons: vec!["project baseline reviewed".to_owned()],
        waiver_ref: None,
        decided_at: 100,
        digest: String::new(),
    };
    request.digest = request.canonical_digest();
    request
}

#[test]
fn project_acceptance_rejects_stale_baseline_missing_milestone_and_self_review() {
    let mut missing = request(ProjectAcceptanceDecision::Accept);
    missing.milestone_acceptance_digests.remove("m2-acceptance");
    missing.digest = missing.canonical_digest();
    assert_eq!(
        missing.validate().unwrap_err(),
        "project_acceptance_milestone_digest_invalid"
    );

    let mut self_review = request(ProjectAcceptanceDecision::Accept);
    self_review.acceptor_session = "reviewer-session".to_owned();
    self_review.digest = self_review.canonical_digest();
    assert_eq!(
        self_review.validate().unwrap_err(),
        "project_acceptance_acceptor_not_independent"
    );
}

#[test]
fn project_acceptance_aggregates_all_required_milestones_without_overwriting_them() {
    let mut ledger = ProjectAcceptanceLedger::default();
    let request = request(ProjectAcceptanceDecision::Accept);
    ledger.record(request.clone()).expect("record");
    ledger.record(request).expect("idempotent");
    assert_eq!(ledger.requests.len(), 1);
    assert_eq!(
        ledger.requests["project-acceptance-1"]
            .required_milestone_acceptance_ids
            .len(),
        2
    );
}

#[test]
fn project_waiver_requires_sponsor_and_named_reason() {
    let mut waiver = request(ProjectAcceptanceDecision::Waive);
    waiver.waiver_ref = Some("waiver:project".to_owned());
    waiver.acceptor_role = "closer".to_owned();
    waiver.digest = waiver.canonical_digest();
    assert_eq!(
        waiver.validate().unwrap_err(),
        "project_acceptance_waiver_requires_sponsor"
    );
}
