//! DEP-35 source guard for local rollout phase ordering and evidence gates.

#[test]
fn local_rollout_is_ordered_and_does_not_fake_ready_or_promote() {
    let source = include_str!("../../kiana-domain/src/local_rollout.rs");
    let baseline = include_str!("../../docs/roadmap/dep35-local-rollout-baseline.md");
    for marker in [
        "LocalRolloutMode",
        "ManagedLocal",
        "EmbeddedLocal",
        "LocalRolloutPhase",
        "Plan",
        "Preflight",
        "Backup",
        "Drain",
        "Replace",
        "Ready",
        "Promote",
        "LocalRolloutEvidence",
        "active_run_count",
        "active_writer_count",
        "old_revision_fenced",
        "replacement_started",
        "readiness_verified",
        "rollout_phase_order_invalid",
        "rollout_drain_not_safe",
        "rollout_promote_gate_blocked",
    ] {
        assert!(
            source.contains(marker),
            "DEP-35 source marker missing: {marker}"
        );
    }
    for marker in [
        "managed-local",
        "embedded-local",
        "plan",
        "preflight",
        "backup",
        "drain",
        "replace",
        "ready",
        "promote",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-35 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("std::fs"));
}
