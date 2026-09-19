//! DEP-37 source guard for orchestrated rollout design and worker build routing.

#[test]
fn orchestrated_rollout_is_pinned_deadline_bound_and_target_explicit() {
    let source = include_str!("../../kiana-domain/src/orchestrated_rollout.rs");
    let baseline = include_str!("../../docs/roadmap/dep37-orchestrated-rollout-baseline.md");
    for marker in [
        "OrchestratedRolloutProfile",
        "Canary",
        "BlueGreen",
        "Rainbow",
        "ExecutionRevisionPin",
        "WorkerBuildRoute",
        "active_writer_revision_id",
        "writer_fence_digest",
        "progress_deadline_unix_ms",
        "rollout_progress_deadline_exceeded",
        "rollout_old_worker_not_fenced",
        "rollout_canary_gate_blocked",
        "OrchestratedRolloutAction",
        "Pause",
        "Resume",
        "Promote",
        "Rollback",
        "KubernetesTarget",
        "GenericOrchestratorTarget",
        "is_target",
    ] {
        assert!(
            source.contains(marker),
            "DEP-37 source marker missing: {marker}"
        );
    }
    for marker in [
        "canary",
        "blue-green",
        "rainbow",
        "worker build routing",
        "progress deadline",
        "single active writer",
        "pause/resume/promote/rollback",
        "target",
        "partial",
        "fake",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-37 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
