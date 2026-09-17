#[test]
fn storage_lifecycle_ports_have_no_implicit_fallback_or_execution_authority() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/storage_health.rs");
    for marker in [
        "ProjectionStorePort",
        "ArtifactStorePort",
        "BackupStorePort",
        "MigrationRunnerPort",
        "RetentionStorePort",
        "StorageHealth",
        "expected_source_cursor",
        "expected_revision",
        "append_tombstone",
    ] {
        assert!(ports.contains(marker), "port marker missing: {marker}");
    }
    assert!(domain.contains("StorageCapabilities"));
    for forbidden in ["kiana_daemon", "kiana_provider", "PathBuf", "reqwest"] {
        assert!(
            !ports.contains(forbidden),
            "forbidden storage port type: {forbidden}"
        );
    }
    assert!(!ports.contains("tokio::spawn"));
}
