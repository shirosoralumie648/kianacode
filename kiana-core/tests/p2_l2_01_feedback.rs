#[test]
fn feedback_cannot_mutate_policy_or_history() {
    let platform = include_str!("../src/platform.rs");
    let versioning = include_str!("../src/versioning.rs");
    let quality = include_str!("../../kiana-domain/src/quality.rs");
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    let baseline = include_str!("../../docs/roadmap/p2-l2-01-feedback-baseline.md");

    for marker in [
        "FeedbackCandidate",
        "feedback.submit",
        "feedback.review",
        "feedback.candidate_created",
        "feedback.candidate_reviewed",
        "feedback_candidates",
        "feedback_evidence_required",
        "feedback_quality_gate_required",
        "feedback_independent_reviewer_required",
        "feedback_candidate_exists",
        "feedback_already_reviewed",
        "evidence_refs",
        "reviewer_session_id",
        "promotion",
        "candidate_only",
        "authority_changes_applied",
        "commit_platform",
        "idempotency_key",
        "expected_revision",
        "policy",
        "grant",
        "GoldenTrace",
        "trace.replay",
        "quality.feedback",
        "quality.promote",
        "quality.rollback",
    ] {
        assert!(
            platform.contains(marker)
                || versioning.contains(marker)
                || quality.contains(marker)
                || domain.contains(marker)
                || baseline.contains(marker),
            "feedback marker missing: {marker}"
        );
    }

    assert!(platform.contains("promotion\":\"candidate_only\""));
    assert!(platform.contains("authority_changes_applied\":false"));
    assert!(platform.contains("context.session_id != candidate.session_id"));
    assert!(platform.contains("self.evidence_exists(context, &arguments)"));
    assert!(platform.contains("feedback.candidate_created"));
    assert!(platform.contains("feedback.candidate_reviewed"));
    assert!(!platform.contains("self.policy"));
    assert!(!platform.contains("CapabilityBroker"));
    assert!(!platform.contains("ModelClient"));
}
