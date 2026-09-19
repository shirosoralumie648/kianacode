#[test]
fn resume_cache_and_source_revocation_share_one_invalidation_contract() {
    let source = include_str!("../../kiana-domain/src/context_invalidation.rs");
    for marker in [
        "ContextInvalidationPlan",
        "ContextCacheBinding",
        "ContextResumeView",
        "ContextInvalidationTarget::Summary",
        "ContextInvalidationTarget::Selection",
        "ContextInvalidationTarget::Cache",
        "ContextInvalidationTarget::Checkpoint",
        "context_resume_binding_changed",
        "context_cache_invalidation_target_missing",
        "rebuild_after_restart",
    ] {
        assert!(
            source.contains(marker),
            "CM-21 source marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}
