#[test]
fn quality_foundation_is_domain_owned_and_has_no_promotion_side_effect() {
    let quality = include_str!("../../kiana-domain/src/quality.rs");
    let ids = include_str!("../../kiana-domain/src/ids.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    for marker in [
        "pub enum QualityArtifactStatus",
        "pub struct QualityArtifact",
        "pub struct QualityStateTransition",
        "QUALITY_ARTIFACT_SCHEMA",
        "QUALITY_TRANSITION_SCHEMA",
        "canonical_quality_bytes",
        "deny_unknown_fields",
        "quality_transition_invalid",
        "quality_artifact_digest_mismatch",
    ] {
        assert!(quality.contains(marker), "quality marker missing: {marker}");
    }
    for marker in [
        "uuid_id!(QualityArtifactId)",
        "uuid_id!(QualityTransitionId)",
        "uuid_id!(EvalDatasetId)",
        "uuid_id!(EvalSuiteId)",
        "uuid_id!(EvalCaseId)",
        "uuid_id!(GoldenTraceId)",
        "uuid_id!(EvalExperimentId)",
        "uuid_id!(EvalResultId)",
        "uuid_id!(QualityCandidateId)",
        "uuid_id!(QualityGateId)",
        "uuid_id!(QualityGateDecisionId)",
        "uuid_id!(FeedbackId)",
        "uuid_id!(DriftAlertId)",
    ] {
        assert!(ids.contains(marker), "quality ID marker missing: {marker}");
    }
    assert!(contracts.contains("kiana.quality-artifact.v1"));
    assert!(contracts.contains("kiana.quality-transition.v1"));
    assert!(!quality.contains("QualityGateDecision::promote"));
    assert!(!quality.contains("CapabilityBroker"));
}
