//! DEP-38 source guard for rollout lifecycle, retirement and post-deploy verification.

#[test]
fn rollout_lifecycle_keeps_pause_promote_rollback_and_retention_gated() {
    let source = include_str!("../../kiana-domain/src/rollout_lifecycle.rs");
    let evidence = include_str!("../../kiana-domain/src/rollout_lifecycle_evidence.rs");
    let baseline = include_str!("../../docs/roadmap/dep38-rollout-lifecycle-baseline.md");
    for marker in [
        "RolloutLifecyclePhase",
        "Paused",
        "Draining",
        "Promoted",
        "RolledBack",
        "Retired",
        "RolloutHealthWindow",
        "PostDeployVerification",
        "OldRevisionRetention",
        "old_root_retained",
        "deletion_eligible",
        "pub fn pause",
        "pub fn resume",
        "pub fn promote",
        "pub fn rollback",
        "retire_old_revision",
        "rollout_post_deploy_verification_failed",
        "rollout_retention_window_open",
        "rollout_old_revision_still_active",
        "RevisionDrain",
        "RevisionDrainStatus::Retired",
        "RolloutLifecycleEvidence",
        "rollout_lifecycle_target_cannot_verify",
        "rollout_lifecycle_unknown_cannot_verify",
        "deletion_eligible",
        "health_window_digest",
    ] {
        assert!(
            source.contains(marker) || evidence.contains(marker),
            "DEP-38 source marker missing: {marker}"
        );
    }
    for marker in [
        "pause/resume/promote/rollback",
        "old revision retirement",
        "retention",
        "post-deploy verification",
        "health window",
        "partial",
        "durable",
        "live",
        "deletion",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-38 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
