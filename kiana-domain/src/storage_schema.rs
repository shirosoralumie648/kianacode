//! Storage schema registry, canonical serialization and explicit upcast boundary.
//!
//! Unknown schema majors and fields are never silently preserved as active data. An upcaster must
//! be named in `SCHEMA_MIGRATIONS`; otherwise the caller receives a typed failure.
use crate::{
    canonical_journal_bytes, json_digest, MemoryRecord, SchemaVersion, MEMORY_RECORD_SCHEMA,
    MEMORY_RECORD_SCHEMA_V2, SCHEMA_CONTRACTS, SCHEMA_MIGRATIONS,
};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use std::collections::BTreeSet;

pub const STORAGE_SCHEMA_REGISTRY_SCHEMA: &str = "kiana.storage-schema-registry.v1";
pub const STORAGE_SCHEMA_REGISTRY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageSchemaRegistry {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_revision: u64,
    pub contracts_digest: String,
    pub generated_at_unix_ms: u64,
    pub registry_digest: String,
}

impl StorageSchemaRegistry {
    pub fn current(generated_at_unix_ms: u64) -> Result<Self, String> {
        validate_schema_registry()?;
        let mut registry = Self {
            schema: STORAGE_SCHEMA_REGISTRY_SCHEMA.to_owned(),
            version: STORAGE_SCHEMA_REGISTRY_VERSION,
            registry_revision: 1,
            contracts_digest: schema_contracts_digest(),
            generated_at_unix_ms,
            registry_digest: String::new(),
        };
        registry.registry_digest = registry.digest();
        registry.validate()?;
        Ok(registry)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_SCHEMA_REGISTRY_SCHEMA
            || self.version != STORAGE_SCHEMA_REGISTRY_VERSION
            || self.registry_revision == 0
        {
            return Err("storage_schema_registry_header_invalid".to_owned());
        }
        validate_schema_registry()?;
        if self.contracts_digest != schema_contracts_digest() {
            return Err("storage_schema_registry_digest_stale".to_owned());
        }
        validate_digest(&self.registry_digest, "storage_schema_registry_digest")?;
        if self.registry_digest != self.digest() {
            return Err("storage_schema_registry_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "registry_revision": self.registry_revision,
            "contracts_digest": self.contracts_digest,
            "generated_at_unix_ms": self.generated_at_unix_ms,
        }))
    }
}

pub fn validate_schema_registry() -> Result<(), String> {
    let mut names = BTreeSet::new();
    for contract in SCHEMA_CONTRACTS {
        if contract.name.trim().is_empty() || !names.insert(contract.name) {
            return Err("storage_schema_registry_duplicate".to_owned());
        }
        if contract.version.major == 0 {
            return Err("storage_schema_registry_version_invalid".to_owned());
        }
        if contract.layer == crate::SchemaLayer::Domain && contract.allow_unknown_fields {
            return Err("storage_schema_registry_domain_unknown_fields".to_owned());
        }
    }
    Ok(())
}

pub fn schema_contracts_digest() -> String {
    let entries = SCHEMA_CONTRACTS
        .iter()
        .map(|contract| {
            serde_json::json!({
                "name": contract.name,
                "version": contract.version,
                "layer": format!("{:?}", contract.layer),
                "owner_crate": contract.owner_crate,
                "compatibility": format!("{:?}", contract.compatibility),
                "allow_unknown_fields": contract.allow_unknown_fields,
            })
        })
        .collect::<Vec<_>>();
    json_digest(&serde_json::json!(entries))
}

/// Canonical storage bytes with deterministic object-key order and conservative number policy.
pub fn canonical_storage_bytes(value: &Value) -> Result<Vec<u8>, String> {
    validate_canonical_value(value, 0)?;
    canonical_journal_bytes(value)
}

pub fn canonical_storage_digest(value: &Value) -> Result<String, String> {
    let bytes = canonical_storage_bytes(value)?;
    Ok(format!("sha256:{}", crate::journal_sha256(&bytes)))
}

/// Upcast a payload only when a named migration exists. The returned value is still a payload;
/// callers must validate the destination DTO before making it active.
pub fn upcast_storage_value(value: Value, expected_schema: &str) -> Result<Value, String> {
    let schema = value
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| "storage_schema_required".to_owned())?;
    if crate::schema_contract(expected_schema).is_none() {
        return Err("storage_schema_expected_unknown".to_owned());
    }
    if schema == expected_schema {
        return Ok(value);
    }
    if expected_schema == MEMORY_RECORD_SCHEMA_V2 && schema == MEMORY_RECORD_SCHEMA {
        return upcast_memory_record(value);
    }
    if schema_major(schema).is_none()
        || schema_major(expected_schema).is_none()
        || schema_major(schema) != schema_major(expected_schema)
    {
        return Err("storage_schema_unknown_major".to_owned());
    }
    if SCHEMA_MIGRATIONS
        .iter()
        .any(|migration| migration.from == schema && migration.to == expected_schema)
    {
        return Err("storage_schema_migration_unimplemented".to_owned());
    }
    Err("storage_schema_migration_missing".to_owned())
}

fn upcast_memory_record(value: Value) -> Result<Value, String> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| "memory_record_object_required".to_owned())?;
    const LEGACY_FIELDS: &[&str] = &[
        "project_root",
        "schema",
        "id",
        "layer",
        "collection",
        "text",
        "source",
        "role_id",
        "department_id",
        "session_id",
        "created_at_ms",
        "kind",
        "evidence",
        "content_hash",
        "supersedes",
        "origin",
        "admission_state",
        "state",
        "classification",
        "purpose",
        "sensitivity",
        "validity",
        "retention",
        "dependencies",
        "import_mode",
        "revision",
        "last_mutation_key",
        "reviewed_by",
        "review_reason",
        "reviewed_at_ms",
    ];
    if object
        .keys()
        .any(|key| !LEGACY_FIELDS.contains(&key.as_str()))
    {
        return Err("storage_migration_non_migratable_field".to_owned());
    }
    object.insert(
        "schema".to_owned(),
        Value::String(MEMORY_RECORD_SCHEMA_V2.to_owned()),
    );
    object
        .entry("kind".to_owned())
        .or_insert_with(|| Value::String("legacy".to_owned()));
    object
        .entry("import_mode".to_owned())
        .or_insert_with(|| Value::String("legacy_import".to_owned()));
    object
        .entry("revision".to_owned())
        .or_insert_with(|| Value::Number(Number::from(1u64)));
    let record = MemoryRecord::legacy_import(Value::Object(object))
        .map_err(|error| format!("storage_migration_destination_invalid:{error}"))?;
    serde_json::to_value(record).map_err(|_| "storage_migration_encode_failed".to_owned())
}

fn schema_major(schema: &str) -> Option<u32> {
    schema
        .rsplit_once(".v")
        .and_then(|(_, major)| major.parse::<u32>().ok())
}

fn validate_canonical_value(value: &Value, depth: usize) -> Result<(), String> {
    if depth > 128 {
        return Err("storage_canonical_depth_exceeded".to_owned());
    }
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
        Value::Number(number) => {
            if number.is_f64() {
                let text = number.to_string();
                if text == "-0.0" || text == "-0" || text.contains("e+") {
                    return Err("storage_noncanonical_number".to_owned());
                }
                if number.as_f64().is_none_or(|value| !value.is_finite()) {
                    return Err("storage_nonfinite_number".to_owned());
                }
            }
            Ok(())
        }
        Value::Array(values) => values
            .iter()
            .try_for_each(|value| validate_canonical_value(value, depth + 1)),
        Value::Object(object) => object.iter().try_for_each(|(key, value)| {
            if key.contains('\0') {
                return Err("storage_canonical_key_invalid".to_owned());
            }
            validate_canonical_value(value, depth + 1)
        }),
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
