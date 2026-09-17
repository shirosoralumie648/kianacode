#[test]
fn storage_schema_registry_is_canonical_and_upcast_is_explicit() {
    let schema = include_str!("../../kiana-domain/src/storage_schema.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    for marker in [
        "StorageSchemaRegistry",
        "validate_schema_registry",
        "canonical_storage_bytes",
        "canonical_storage_digest",
        "upcast_storage_value",
        "upcast_memory_record",
        "storage_schema_unknown_major",
        "storage_migration_non_migratable_field",
        "storage_noncanonical_number",
        "SCHEMA_MIGRATIONS",
    ] {
        assert!(schema.contains(marker), "schema marker missing: {marker}");
    }
    assert!(contracts.contains("kiana.storage-schema-registry.v1"));
    assert!(!schema.contains("tokio::spawn"));
    assert!(!schema.contains("CapabilityBroker"));
}
