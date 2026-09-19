//! DEP-32 source guard for rollback decision gates.

#[test]
fn rollback_gate_requires_backup_fence_and_reconciliation() {
    let source = include_str!("../../kiana-domain/src/migration_rollback.rs");
    let baseline = include_str!("../../docs/roadmap/dep32-migration-rollback-baseline.md");
    for marker in [
        "MigrationRollbackKind",
        "Binary",
        "Data",
        "EffectReconciliation",
        "MigrationRollbackAction",
        "MigrationRollbackFacts",
        "MigrationRollbackReceipt",
        "verified_backup",
        "active_writer_count",
        "unknown_effect_count",
        "old_revision_compatible",
        "new_revision_fenced",
        "old_root_retained",
        "restore_verified",
        "external_effect_reconciled",
        "rollback_backup_unverified",
        "rollback_writer_active",
        "rollback_unknown_effects",
        "rollback_binary_incompatible",
        "rollback_restore_unverified",
        "rollback_effect_unreconciled",
    ] {
        assert!(
            source.contains(marker),
            "DEP-32 source marker missing: {marker}"
        );
    }
    for marker in [
        "destructive migration",
        "verified backup",
        "writer",
        "unknown effect",
        "old revision",
        "restore",
        "rollback receipt",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-32 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("std::fs"));
}
