use kiana_domain::*;
use serde_json::json;

fn legacy_memory() -> serde_json::Value {
    json!({
        "schema": MEMORY_RECORD_SCHEMA,
        "id": "legacy-1",
        "layer": "project",
        "collection": "project",
        "text": "legacy note",
        "source": "fixture",
        "role_id": "builder",
        "department_id": "executing",
        "session_id": "session-1",
        "created_at_ms": 1
    })
}

#[test]
fn storage_schema_registry_is_unique_and_canonical() {
    validate_schema_registry().unwrap();
    let registry = StorageSchemaRegistry::current(100).unwrap();
    registry.validate().unwrap();
    let first = json!({"b":1,"a":2});
    let second = json!({"a":2,"b":1});
    assert_eq!(
        canonical_storage_bytes(&first).unwrap(),
        canonical_storage_bytes(&second).unwrap()
    );
    assert_eq!(
        canonical_storage_digest(&first).unwrap(),
        canonical_storage_digest(&second).unwrap()
    );
    assert_eq!(
        canonical_storage_bytes(&json!({"number":-0.0})).unwrap_err(),
        "storage_noncanonical_number"
    );
    let mut unknown = serde_json::to_value(&registry).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<StorageSchemaRegistry>(unknown).is_err());
}

#[test]
fn storage_upcaster_requires_named_migration_and_rejects_unknown_major_or_field() {
    let upgraded = upcast_storage_value(legacy_memory(), MEMORY_RECORD_SCHEMA_V2).unwrap();
    assert_eq!(upgraded["schema"], MEMORY_RECORD_SCHEMA_V2);
    assert_eq!(upgraded["import_mode"], "legacy_import");
    assert_eq!(upgraded["admission_state"], "candidate");
    assert_eq!(
        upcast_storage_value(
            json!({"schema":"kiana.memory-record.v9"}),
            MEMORY_RECORD_SCHEMA_V2
        )
        .unwrap_err(),
        "storage_schema_unknown_major"
    );
    let mut unknown = legacy_memory();
    unknown["non_migratable"] = json!(true);
    assert_eq!(
        upcast_storage_value(unknown, MEMORY_RECORD_SCHEMA_V2).unwrap_err(),
        "storage_migration_non_migratable_field"
    );
    assert_eq!(
        upcast_storage_value(
            json!({"schema":"kiana.storage-root.v1"}),
            "kiana.storage-root.v1"
        )
        .unwrap()
        .get("schema")
        .and_then(|value| value.as_str()),
        Some("kiana.storage-root.v1")
    );
    assert_eq!(
        upcast_storage_value(json!({"schema":"kiana.future.v1"}), "kiana.unknown.v1").unwrap_err(),
        "storage_schema_expected_unknown"
    );
}
