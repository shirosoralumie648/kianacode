//! DEP-19 source guard: the backup manifest is a verifiable, redacted, effect-free contract.

#[test]
fn dep19_backup_manifest_binds_cursors_epochs_and_refuses_paths() {
    let domain = include_str!("../../kiana-domain/src/backup_manifest.rs");
    for marker in [
        "BackupManifest",
        "BackupChunk",
        "BackupChunkKind",
        "BackupIntegrity",
        "backup_manifest_header_invalid",
        // The missing-chunk denial is built as `format!("backup_manifest_missing_{}",
        // required.as_str())`, so assert the prefix together with the two kinds it interpolates.
        "backup_manifest_missing_{}",
        "EventLog",
        "MigrationRegistry",
        "backup_manifest_integrity_",
        "backup_chunk_reference_invalid",
        "export_redacted",
        "validate_against_revision",
    ] {
        assert!(domain.contains(marker), "DEP-19 marker missing: {marker}");
    }
    // The manifest must never carry a filesystem location or read/write bytes.
    for forbidden in [
        "std::fs",
        "std::process",
        "PathBuf",
        "read_to_string",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::",
    ] {
        assert!(
            !domain.contains(forbidden),
            "DEP-19 backup manifest crossed effect boundary: {forbidden}"
        );
    }
}

#[test]
fn dep19_whole_manifest_is_unusable_when_any_chunk_is_torn_or_unknown() {
    let domain = include_str!("../../kiana-domain/src/backup_manifest.rs");
    // A torn or unknown chunk must fail the whole manifest, never degrade to a partial restore.
    assert!(
        domain.contains("fn restorable(&self) -> Result<(), String>"),
        "DEP-19 restorable gate missing"
    );
    assert!(
        domain.contains("backup_manifest_integrity_"),
        "DEP-19 must map integrity to a stable denial code"
    );
    // Alert-free: no side effect escapes through the manifest.
    assert!(!domain.contains("fn restore"), "DEP-19 must not restore");
    assert!(
        !domain.contains("fn create"),
        "DEP-19 must not create a backup"
    );
}
