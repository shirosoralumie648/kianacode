//! SC-42 source guard for recovery, replay, reconcile and retention rehearsal.

#[test]
fn sc42_rehearsal_keeps_old_lease_unknown_and_retention_guards() {
    let source = include_str!("../../kiana-domain/src/security_rehearsal.rs");
    let baseline = include_str!("../../docs/roadmap/sc42-security-rehearsal-baseline.md");
    let workflow = include_str!("../../.github/workflows/sc42-security-rehearsal.yml");
    let baseline_text = include_str!("../../docs/roadmap/sc42-security-rehearsal-baseline.md");
    for marker in [
        "SecurityRehearsalScenario",
        "Restart",
        "RestoreQuarantine",
        "Replay",
        "UnknownReconcile",
        "RetentionPrune",
        "old_lease_fenced",
        "quarantine_verified",
        "no_duplicate_effect",
        "unknown_reconciled",
        "retention_watermark_committed",
        "legal_hold_respected",
        "security_rehearsal_unknown_retry_forbidden",
        "security_rehearsal_fence_or_duplicate_guard_failed",
        "security_rehearsal_retention_guard_failed",
    ] {
        assert!(
            source.contains(marker),
            "SC-42 source marker missing: {marker}"
        );
    }
    for marker in [
        "package-lifecycle-smoke.sh",
        "oa28-live-handoff-preflight.sh",
        "oa26-durable-observability-gate.sh",
        "bash -n",
        "cargo test -p kiana-domain --test sc42_security_rehearsal",
        "result_unknown",
        "retention",
        "partial",
        "durable",
        "live",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-42 workflow marker missing: {marker}"
        );
    }
    for marker in [
        "restart",
        "quarantine",
        "restore",
        "replay",
        "duplicate effect",
        "Unknown",
        "reconcile",
        "retention watermark",
        "legal hold",
        "fake",
        "partial",
        "physical",
        "PersistenceUatEvidence",
        "RolloutLifecycleEvidence",
    ] {
        assert!(
            baseline.contains(marker) || baseline_text.contains(marker),
            "SC-42 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
