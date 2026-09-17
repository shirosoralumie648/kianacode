use kiana_ports::{
    ArtifactStorePort, BackupStorePort, Clock, MigrationRunnerPort, ProjectionStorePort,
    RetentionStorePort,
};

struct CompileOnlyStoragePorts;
impl ProjectionStorePort for CompileOnlyStoragePorts {}
impl ArtifactStorePort for CompileOnlyStoragePorts {}
impl BackupStorePort for CompileOnlyStoragePorts {}
impl MigrationRunnerPort for CompileOnlyStoragePorts {}
impl RetentionStorePort for CompileOnlyStoragePorts {}
impl Clock for CompileOnlyStoragePorts {
    fn now_unix_ms(&self) -> u64 {
        0
    }
}

#[test]
fn storage_ports_are_typed_and_default_to_explicit_unsupported() {
    let _ = CompileOnlyStoragePorts;
    let source = include_str!("../src/lib.rs");
    for marker in [
        "pub trait ProjectionStorePort",
        "pub trait ArtifactStorePort",
        "pub trait BackupStorePort",
        "pub trait MigrationRunnerPort",
        "pub trait RetentionStorePort",
        "projection_store_unsupported",
        "artifact_store_unsupported",
        "backup_store_unsupported",
        "migration_runner_unsupported",
        "retention_store_unsupported",
        "expected_source_cursor",
        "expected_tombstone_revision",
    ] {
        assert!(
            source.contains(marker),
            "storage port marker missing: {marker}"
        );
    }
    assert!(!source.contains("kiana_daemon"));
    assert!(!source.contains("kiana_provider"));
    assert!(!source.contains("PathBuf"));
    assert!(!source.contains("reqwest"));
}
