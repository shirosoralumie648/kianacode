//! DEP-30 source guard for migration runner fencing and quarantine.

#[test]
fn migration_runner_has_fence_resume_event_and_quarantine_boundaries() {
    let source = include_str!("../../kiana-domain/src/migration_runner.rs");
    let baseline = include_str!("../../docs/roadmap/dep30-migration-runner-baseline.md");
    for marker in [
        "MigrationRunnerState",
        "MigrationRunnerStatus",
        "MigrationRunnerEventKind",
        "Started",
        "Step",
        "Blocked",
        "Completed",
        "MigrationResumeToken",
        "fence_token",
        "lease_expires_at_unix_ms",
        "migration_runner_concurrent",
        "migration_runner_stale_fence",
        "migration_lease_expired",
        "migration_resume_token_mismatch",
        "migration_registry_checksum_drift",
        "migration_failure_quarantined",
        "MigrationCheckpoint",
    ] {
        assert!(
            source.contains(marker),
            "DEP-30 source marker missing: {marker}"
        );
    }
    for marker in [
        "second runner",
        "lease 过期",
        "checksum 改变",
        "resume token",
        "quarantine",
        "MigrationStarted",
        "MigrationStep",
        "MigrationBlocked",
        "MigrationCompleted",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-30 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("std::fs"));
}
