//! PD-33 source guard for persistence lifecycle UAT and recovery boundaries.

#[test]
fn persistence_uat_keeps_restore_upgrade_restart_and_delete_gates_explicit() {
    let source = include_str!("../../kiana-domain/src/persistence_uat.rs");
    let checkpoint = include_str!("../../kiana-core/src/workspace_checkpoints.rs");
    let baseline = include_str!("../../docs/roadmap/pd33-persistence-uat-baseline.md");
    for marker in [
        "PersistenceUatStage",
        "Backup",
        "Restore",
        "Upgrade",
        "Restart",
        "GovernanceDelete",
        "storage_root_digest",
        "event_store_digest",
        "source_cursor",
        "projection_cursor",
        "backup_manifest_verified",
        "quarantine_verified",
        "auth_re_admitted",
        "projection_receipt_parity",
        "old_root_retained",
        "migration_verified",
        "journal_replayed",
        "legal_hold",
        "deletion_authorized",
        "persistence_uat_unknown_retry_forbidden",
        "persistence_uat_restore_gate_failed",
        "persistence_uat_delete_gate_failed",
    ] {
        assert!(
            source.contains(marker),
            "PD-33 source marker missing: {marker}"
        );
    }
    for marker in [
        "prepare_checkpoint_restore",
        "finish_checkpoint_restore",
        "workspace.restore_requested",
        "workspace.restored",
        "result_unknown:checkpoint_restore_result_invalid",
    ] {
        assert!(
            checkpoint.contains(marker),
            "PD-33 checkpoint marker missing: {marker}"
        );
    }
    for marker in [
        "backup",
        "restore",
        "upgrade",
        "restart",
        "governance delete",
        "auth re-admission",
        "Receipt",
        "old root",
        "legal hold",
        "result_unknown",
        "fake",
        "partial",
        "durable",
    ] {
        assert!(
            baseline.contains(marker),
            "PD-33 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
