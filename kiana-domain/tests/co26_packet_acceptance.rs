use kiana_domain::*;
use serde_json::json;
use std::collections::BTreeMap;

fn assignment(evidence_digest: &str) -> ReviewerAssignment {
    let mut assignment = ReviewerAssignment {
        schema: COMPANY_REVIEW_ASSIGNMENT_SCHEMA.to_owned(),
        assignment_id: "review-assignment".to_owned(),
        project_id: "project-1".to_owned(),
        packet_id: "packet-1".to_owned(),
        packet_version: 2,
        baseline_version: 3,
        author_principal_ids: vec!["builder".to_owned()],
        author_session_ids: vec!["builder-session".to_owned()],
        reviewer_principal_id: "reviewer".to_owned(),
        reviewer_session_id: "reviewer-session".to_owned(),
        reviewer_role: "reviewer".to_owned(),
        reviewer_role_instance_id: "reviewer-instance".to_owned(),
        criteria_snapshot_digest: format!("sha256:{}", "a".repeat(64)),
        evidence_bundle_digest: evidence_digest.to_owned(),
        digest: String::new(),
    };
    assignment.digest = assignment.canonical_digest();
    assignment
}

fn evidence() -> CompanyEvidenceReady {
    let run_id = RunId::new();
    let invocation_id = InvocationId::new();
    let bundle_digest = format!("sha256:{}", "b".repeat(64));
    let mut ready = CompanyEvidenceReady {
        schema: COMPANY_EVIDENCE_READY_SCHEMA.to_owned(),
        bundle_id: "bundle-1".to_owned(),
        bundle_digest,
        run_id,
        invocation_id,
        source_cursor: 8,
        ready_at: 100,
        digest: String::new(),
    };
    ready.digest = json_digest(&json!({
        "schema": ready.schema,
        "bundle_id": ready.bundle_id,
        "bundle_digest": ready.bundle_digest,
        "run_id": ready.run_id,
        "invocation_id": ready.invocation_id,
        "source_cursor": ready.source_cursor,
        "ready_at": ready.ready_at,
    }));
    ready
}

fn request(decision: PacketAcceptanceDecision) -> PacketAcceptanceRequest {
    let evidence = evidence();
    let mut review = IndependentReview {
        schema: COMPANY_INDEPENDENT_REVIEW_SCHEMA.to_owned(),
        review_id: "review-1".to_owned(),
        assignment: assignment(&evidence.bundle_digest),
        criterion_results: BTreeMap::from([(
            "criterion-1".to_owned(),
            CriterionReviewResult {
                verdict: ReviewVerdict::Pass,
                evidence_refs: vec!["evidence:bundle-1".to_owned()],
                reason: "all observed evidence matches".to_owned(),
                waiver_ref: None,
            },
        )]),
        completed_at: 90,
        digest: String::new(),
    };
    review.digest = review.canonical_digest();
    let mut request = PacketAcceptanceRequest {
        schema: PACKET_ACCEPTANCE_SCHEMA.to_owned(),
        acceptance_id: "acceptance-1".to_owned(),
        project_id: "project-1".to_owned(),
        packet_id: "packet-1".to_owned(),
        packet_version: 2,
        attempt_ref: "attempt:1".to_owned(),
        author_session_ids: vec!["builder-session".to_owned()],
        review,
        evidence,
        acceptor_assignment: "assignment:closer".to_owned(),
        acceptor_session: "closer-session".to_owned(),
        acceptor_role: "closer".to_owned(),
        decision,
        reasons: vec!["packet output reviewed".to_owned()],
        waiver_ref: None,
        declared_dependents: vec!["packet-2".to_owned()],
        decided_at: 110,
        digest: String::new(),
    };
    request.digest = request.canonical_digest();
    request
}

#[test]
fn packet_acceptance_rejects_run_success_without_review_or_complete_evidence() {
    let mut missing_review = request(PacketAcceptanceDecision::Accept);
    missing_review.review.criterion_results.clear();
    missing_review.review.digest = missing_review.review.canonical_digest();
    missing_review.digest = missing_review.canonical_digest();
    assert_eq!(
        missing_review.validate().unwrap_err(),
        "company_review_criteria_required"
    );

    let mut mismatch = request(PacketAcceptanceDecision::Accept);
    mismatch.evidence.bundle_digest = format!("sha256:{}", "f".repeat(64));
    mismatch.evidence.digest = json_digest(&json!({
        "schema": mismatch.evidence.schema,
        "bundle_id": mismatch.evidence.bundle_id,
        "bundle_digest": mismatch.evidence.bundle_digest,
        "run_id": mismatch.evidence.run_id,
        "invocation_id": mismatch.evidence.invocation_id,
        "source_cursor": mismatch.evidence.source_cursor,
        "ready_at": mismatch.evidence.ready_at,
    }));
    mismatch.digest = mismatch.canonical_digest();
    assert_eq!(
        mismatch.validate().unwrap_err(),
        "packet_acceptance_evidence_binding_mismatch"
    );
}

#[test]
fn accepted_packet_unlocks_only_its_declared_dependents() {
    let mut ledger = PacketAcceptanceLedger::default();
    let request = request(PacketAcceptanceDecision::Accept);
    ledger.record(request).expect("record");
    assert_eq!(
        ledger
            .accepted_dependents("acceptance-1")
            .map(|values| values.to_vec()),
        Some(vec!["packet-2".to_owned()])
    );
    let mut duplicate = request(PacketAcceptanceDecision::Accept);
    duplicate.acceptance_id = "acceptance-2".to_owned();
    duplicate.declared_dependents = vec!["packet-foreign".to_owned()];
    duplicate.digest = duplicate.canonical_digest();
    ledger.record(duplicate).expect("second target");
    assert_eq!(
        ledger.accepted_dependents("acceptance-1").unwrap(),
        ["packet-2"]
    );
}

#[test]
fn not_applicable_or_waiver_never_silently_counts_as_acceptance() {
    let mut request = request(PacketAcceptanceDecision::Accept);
    request
        .review
        .criterion_results
        .get_mut("criterion-1")
        .unwrap()
        .verdict = ReviewVerdict::NotApplicable;
    request.review.digest = request.review.canonical_digest();
    request.digest = request.canonical_digest();
    assert_eq!(
        request.validate().unwrap_err(),
        "company_review_not_applicable_waiver_required"
    );
}
