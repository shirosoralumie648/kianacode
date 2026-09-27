//! DEP-21 source guard: the backup chain is planned, never performed.

#[test]
fn dep21_backup_chain_refuses_a_parentless_incremental_and_a_parentful_full() {
    let domain = include_str!("../../kiana-domain/src/backup_lifecycle.rs");
    for marker in [
        "BackupChain",
        "BackupChainEntry",
        "BackupLegalHold",
        "BackupDeletionPlan",
        "BackupLink",
        "BackupState",
        "DeletionMode",
        "backup_chain_full_with_parent",
        "backup_chain_incremental_without_parent",
        "backup_chain_incremental_covers_nothing",
        "backup_chain_parent_cursor_mismatch",
        "backup_chain_parent_after_child",
        "backup_required_by_live_child",
        "backup_chain_encryption_key_missing",
        "backup_deletion_plan_dry_run_not_empty",
    ] {
        assert!(domain.contains(marker), "DEP-21 marker missing: {marker}");
    }
    // Retention is planned here. The module must not delete, archive, encrypt or key anything.
    for forbidden in [
        "std::fs",
        "std::process",
        "remove_dir",
        "remove_file",
        "tokio::",
        "EventStorePort",
        "ArtifactStorePort",
        "encrypt(",
        "decrypt(",
    ] {
        assert!(
            !domain.contains(forbidden),
            "DEP-21 backup lifecycle crossed effect boundary: {forbidden}"
        );
    }
}

#[test]
fn dep21_a_dry_run_can_never_carry_a_removal_list() {
    let domain = include_str!("../../kiana-domain/src/backup_lifecycle.rs");
    assert!(
        domain.contains("self.mode == DeletionMode::DryRun && !self.deletable.is_empty()"),
        "DEP-21 must reject a dry run that carries a removal list"
    );
    // An encryption key travels as an opaque reference; an inline key must never appear.
    assert!(
        domain.contains("encryption_key_ref: Option<SecretRefId>"),
        "DEP-21 must carry a key reference, not key material"
    );
    assert!(
        !domain.contains("String = \"-----BEGIN"),
        "DEP-21 must never carry inline key material"
    );
}
