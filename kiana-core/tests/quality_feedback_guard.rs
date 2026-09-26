#[test]
fn quality_feedback_uses_trusted_context_and_has_no_mutation_path() {
    let domain = include_str!("../../kiana-domain/src/quality_feedback.rs");
    let core = include_str!("../src/quality_feedback.rs");
    for marker in [
        "QualityCanonicalTarget",
        "QualityFeedbackSubmission",
        "QualityFeedbackServerContext",
        "QualityFeedbackProvenance",
        "QualityFeedbackPrivacyScope",
        "target_privacy_class",
        "project_trusted",
        "actor_id",
        "source_event_digest",
        "QUALITY_FEEDBACK_COMMAND",
        "canonical_bytes",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "EQ-45 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ModelClient",
        "std::process::Command",
        "policy.update",
        "receipt.update",
        "grant.update",
        "route.switch",
        "append_event",
        "EventStore",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "EQ-45 mutation/effect marker present: {forbidden}"
        );
    }
}
