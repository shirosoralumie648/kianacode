//! DEP-22 source guard: quarantine admits a restored root without touching the active one.

#[test]
fn dep22_restore_quarantine_refuses_the_active_root_and_regressed_epochs() {
    let domain = include_str!("../../kiana-domain/src/restore_quarantine.rs");
    for marker in [
        "RestoreQuarantine",
        "RestoreRoot",
        "RestoreScanReport",
        "RestoreScanStatus",
        "QuarantineStage",
        "RestoreMode",
        "restore_root_would_overwrite_active",
        "restore_projection_cursor_ahead_of_source",
        "restore_data_epoch_not_advanced",
        "restore_authority_epoch_not_advanced",
        "restore_migration_not_applied",
        "restore_migration_version_missing",
        "restore_artifact_missing",
        "restore_quarantine_stage_not_advancing",
        "restore_scan_report_rejected_but_ready",
    ] {
        assert!(domain.contains(marker), "DEP-22 marker missing: {marker}");
    }
    // The domain decides; it must not touch a filesystem, a lease or the active root.
    for forbidden in [
        "std::fs",
        "std::process",
        "PathBuf",
        "remove_dir",
        "rename(",
        "tokio::",
        "EventStorePort",
        "ArtifactStorePort",
        "activate(",
    ] {
        assert!(
            !domain.contains(forbidden),
            "DEP-22 restore quarantine crossed effect boundary: {forbidden}"
        );
    }
}

#[test]
fn dep22_only_a_ready_root_is_activation_eligible() {
    let domain = include_str!("../../kiana-domain/src/restore_quarantine.rs");
    // Verified is not Ready: a scanned-but-not-rebuilt root is a directory, not a restore.
    assert!(
        domain.contains("matches!(self, Self::Ready)"),
        "DEP-22 must restrict activation eligibility to the Ready stage alone"
    );
    // Eligibility requires both a complete scan and a Ready root, never either alone.
    assert!(
        domain.contains("self.status == RestoreScanStatus::Complete"),
        "DEP-22 must require a complete scan for activation eligibility"
    );
    assert!(
        domain.contains("root.stage == QuarantineStage::Ready"),
        "DEP-22 must require the root itself to be Ready"
    );
    // Roots are identified structurally, never by a path.
    assert!(
        !domain.contains("root: String") && !domain.contains("path: String"),
        "DEP-22 must identify a root by id, not by a path"
    );
}
