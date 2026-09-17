#[test]
fn storage_health_and_error_taxonomy_are_typed_before_ports_or_surfaces() {
    let health = include_str!("../../kiana-domain/src/storage_health.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "StorageErrorClass",
        "StorageRetryDisposition",
        "StorageError",
        "StorageCapabilities",
        "StorageHealth",
        "StorageIntegrityIncident",
        "storage_error_retry_mismatch",
        "storage_health_digest_mismatch",
        "quarantine_required",
        "deny_unknown_fields",
    ] {
        assert!(
            health.contains(marker),
            "storage health marker missing: {marker}"
        );
    }
    for marker in [
        "storage_class",
        "into_storage_error",
        "StorageErrorClass::ResultUnknown",
        "StorageErrorClass::Corrupt",
    ] {
        assert!(
            ports.contains(marker),
            "port mapping marker missing: {marker}"
        );
    }
    assert!(protocol.contains("StorageHealth"));
    assert!(protocol.contains("STORAGE_HEALTH_SCHEMA"));
    assert!(!health.contains("tokio::spawn"));
    assert!(!health.contains("CapabilityBroker"));
}
