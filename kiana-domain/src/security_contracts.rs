//! Security identity and versioned envelope contracts.
//!
//! This module is deliberately a domain-only boundary.  It exposes stable references and a
//! security view of the canonical [`crate::SCHEMA_CONTRACTS`] registry, but it does not make an
//! authorization decision, read a secret store, or execute an effect.  A registry snapshot is a
//! chained, immutable value: revisions and epochs may advance, while sequence and parent digest
//! links prevent stale or replayed snapshots from becoming current again.

use crate::{
    canonical_journal_bytes, check_schema_compatibility, json_digest, schema_contract,
    schema_contracts_digest, AuditId, EvidenceRefId, GrantId, OperationId, SchemaContract,
    SchemaVersion, SecretRefId, SecurityContextId, SecurityDecisionId, SecurityEventId,
    SecurityPolicyId, SecurityRegistryId, SCHEMA_CONTRACTS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const SECURITY_SCHEMA_REGISTRY_SCHEMA: &str = "kiana.security-schema-registry.v1";
pub const SECURITY_OBJECT_SCHEMA: &str = "kiana.security-object.v1";
pub const SECURITY_SCHEMA_REGISTRY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const SECURITY_OBJECT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SECURITY_SCHEMA_ENTRIES: usize = 512;
pub const MAX_SECURITY_OBJECT_BYTES: usize = 64 * 1024;
pub const MAX_SECURITY_PAYLOAD_DEPTH: usize = 32;

/// Security objects use a transport identity independent from the typed IDs in their payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityObjectKind {
    Context,
    Policy,
    Grant,
    Decision,
    SecretReference,
    Audit,
    Evidence,
}

impl SecurityObjectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Policy => "policy",
            Self::Grant => "grant",
            Self::Decision => "decision",
            Self::SecretReference => "secret_reference",
            Self::Audit => "audit",
            Self::Evidence => "evidence",
        }
    }
}

/// One entry in the security view of the canonical domain/wire/runtime schema registry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecuritySchemaEntry {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_id: SecurityRegistryId,
    pub owner_crate: String,
    pub contract_digest: String,
}

impl SecuritySchemaEntry {
    fn from_contract(contract: &SchemaContract) -> Self {
        Self {
            schema: contract.name.to_owned(),
            version: contract.version,
            registry_id: SecurityRegistryId::from_uuid(stable_uuid(
                "security-schema-entry",
                contract.name,
            )),
            owner_crate: contract.owner_crate.to_owned(),
            contract_digest: contract_digest(contract),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema.trim().is_empty()
            || self.schema.len() > 256
            || self.schema.contains('\0')
            || self.registry_id.as_uuid().is_nil()
            || self.owner_crate.trim().is_empty()
            || self.owner_crate.len() > 128
        {
            return Err("security_schema_entry_invalid".to_owned());
        }
        validate_digest(&self.contract_digest, "security_schema_entry_digest")?;
        let contract = schema_contract(&self.schema)
            .ok_or_else(|| "security_schema_entry_unregistered".to_owned())?;
        if self.version != contract.version
            || self.owner_crate != contract.owner_crate
            || self.contract_digest != contract_digest(contract)
        {
            return Err("security_schema_entry_drift".to_owned());
        }
        Ok(())
    }
}

/// A monotonic, digest-linked snapshot of the single canonical schema registry.
///
/// `SecuritySchemaRegistry` is a read-only security projection over `SCHEMA_CONTRACTS`; it is
/// not a second source of schema truth.  A successor must link to the previous digest and advance
/// the registry revision and source sequence.  Authority and data epochs may stay equal, but can
/// never decrease.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecuritySchemaRegistry {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_id: SecurityRegistryId,
    pub registry_revision: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub source_sequence: u64,
    pub contracts_digest: String,
    #[serde(default)]
    pub parent_digest: Option<String>,
    pub entries: Vec<SecuritySchemaEntry>,
    pub registry_digest: String,
}

impl SecuritySchemaRegistry {
    /// Generate the genesis snapshot from the canonical registry.
    pub fn current() -> Result<Self, String> {
        Self::build(
            SecurityRegistryId::from_uuid(stable_uuid(
                "security-schema-registry",
                SECURITY_SCHEMA_REGISTRY_SCHEMA,
            )),
            1,
            1,
            1,
            1,
            None,
        )
    }

    /// Generate a chained successor.  Callers supply server-owned epoch/sequence observations;
    /// this value object only verifies monotonicity and never allocates authority.
    pub fn successor(
        previous: &Self,
        authority_epoch: u64,
        data_epoch: u64,
        source_sequence: u64,
    ) -> Result<Self, String> {
        previous.validate()?;
        let revision = previous
            .registry_revision
            .checked_add(1)
            .ok_or_else(|| "security_schema_registry_revision_exhausted".to_owned())?;
        if authority_epoch < previous.authority_epoch {
            return Err("security_schema_registry_authority_epoch_rollback".to_owned());
        }
        if data_epoch < previous.data_epoch {
            return Err("security_schema_registry_data_epoch_rollback".to_owned());
        }
        if source_sequence <= previous.source_sequence {
            return Err("security_schema_registry_sequence_rollback".to_owned());
        }
        Self::build(
            previous.registry_id,
            revision,
            authority_epoch,
            data_epoch,
            source_sequence,
            Some(previous.registry_digest.clone()),
        )
    }

    fn build(
        registry_id: SecurityRegistryId,
        registry_revision: u64,
        authority_epoch: u64,
        data_epoch: u64,
        source_sequence: u64,
        parent_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut entries = SCHEMA_CONTRACTS
            .iter()
            .map(SecuritySchemaEntry::from_contract)
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.schema.cmp(&right.schema));
        let mut registry = Self {
            schema: SECURITY_SCHEMA_REGISTRY_SCHEMA.to_owned(),
            version: SECURITY_SCHEMA_REGISTRY_VERSION,
            registry_id,
            registry_revision,
            authority_epoch,
            data_epoch,
            source_sequence,
            contracts_digest: schema_contracts_digest(),
            parent_digest,
            entries,
            registry_digest: String::new(),
        };
        registry.registry_digest = registry.digest();
        registry.validate()?;
        Ok(registry)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let registry: Self = serde_json::from_value(value.clone())
            .map_err(|_| "security_schema_registry_decode_failed".to_owned())?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "security_schema_registry_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_SCHEMA_REGISTRY_SCHEMA
            || !self
                .version
                .is_compatible_with(&SECURITY_SCHEMA_REGISTRY_VERSION)
            || self.registry_id.as_uuid().is_nil()
            || self.registry_revision == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.source_sequence == 0
            || self.entries.is_empty()
            || self.entries.len() > MAX_SECURITY_SCHEMA_ENTRIES
        {
            return Err("security_schema_registry_header_invalid".to_owned());
        }
        check_schema_compatibility(&self.schema, &self.version)
            .map_err(|_| "security_schema_registry_unknown_major".to_owned())?;
        if self.contracts_digest != schema_contracts_digest() {
            return Err("security_schema_registry_contracts_digest_stale".to_owned());
        }
        validate_digest(
            &self.contracts_digest,
            "security_schema_registry_contracts_digest",
        )?;
        let mut schemas = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !schemas.insert(entry.schema.as_str()) {
                return Err("security_schema_registry_duplicate_schema".to_owned());
            }
            if !ids.insert(entry.registry_id.as_uuid()) {
                return Err("security_schema_registry_duplicate_id".to_owned());
            }
        }
        if self
            .entries
            .windows(2)
            .any(|pair| pair[0].schema >= pair[1].schema)
        {
            return Err("security_schema_registry_entries_noncanonical".to_owned());
        }
        if self.registry_revision == 1 || self.source_sequence == 1 {
            if self.registry_revision != 1
                || self.source_sequence != 1
                || self.parent_digest.is_some()
            {
                return Err("security_schema_registry_genesis_invalid".to_owned());
            }
        } else {
            let parent = self
                .parent_digest
                .as_deref()
                .ok_or_else(|| "security_schema_registry_parent_required".to_owned())?;
            validate_digest(parent, "security_schema_registry_parent_digest")?;
            if parent == self.registry_digest {
                return Err("security_schema_registry_parent_self".to_owned());
            }
        }
        validate_digest(&self.registry_digest, "security_schema_registry_digest")?;
        if self.registry_digest != self.digest() {
            return Err("security_schema_registry_digest_mismatch".to_owned());
        }
        self.reject_serialized_secret_fields()
    }

    /// Validate a candidate against an already accepted snapshot.
    pub fn validate_successor(&self, previous: &Self) -> Result<(), String> {
        previous.validate()?;
        self.validate()?;
        if self.registry_id != previous.registry_id {
            return Err("security_schema_registry_identity_changed".to_owned());
        }
        if self.registry_revision <= previous.registry_revision {
            return Err("security_schema_registry_revision_rollback".to_owned());
        }
        if self.authority_epoch < previous.authority_epoch {
            return Err("security_schema_registry_authority_epoch_rollback".to_owned());
        }
        if self.data_epoch < previous.data_epoch {
            return Err("security_schema_registry_data_epoch_rollback".to_owned());
        }
        if self.source_sequence <= previous.source_sequence {
            return Err("security_schema_registry_sequence_rollback".to_owned());
        }
        if self.parent_digest.as_deref() != Some(previous.registry_digest.as_str()) {
            return Err("security_schema_registry_digest_rollback".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "registry_id": self.registry_id,
            "registry_revision": self.registry_revision,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "source_sequence": self.source_sequence,
            "contracts_digest": self.contracts_digest,
            "parent_digest": self.parent_digest,
            "entries": self.entries,
        }))
    }

    fn reject_serialized_secret_fields(&self) -> Result<(), String> {
        let value = serde_json::to_value(self)
            .map_err(|_| "security_schema_registry_encode_failed".to_owned())?;
        if contains_raw_secret(&value, 0) {
            return Err("security_schema_registry_secret_field".to_owned());
        }
        Ok(())
    }
}

/// Versioned security payload envelope.  Payloads are intentionally untyped here so later SC
/// steps can add their domain objects without adding a second wire envelope.  The payload is still
/// an allow-list boundary: object-only, bounded, canonical and recursively secret-safe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityObjectEnvelope {
    pub schema: String,
    pub version: SchemaVersion,
    pub object_id: SecurityEventId,
    pub object_kind: SecurityObjectKind,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub sequence: u64,
    #[serde(default)]
    pub previous_digest: Option<String>,
    pub payload_digest: String,
    pub payload: Value,
    pub envelope_digest: String,
}

impl SecurityObjectEnvelope {
    pub fn new(
        object_id: SecurityEventId,
        object_kind: SecurityObjectKind,
        authority_epoch: u64,
        data_epoch: u64,
        payload: Value,
    ) -> Result<Self, String> {
        Self::build(
            object_id,
            object_kind,
            authority_epoch,
            data_epoch,
            1,
            None,
            payload,
        )
    }

    pub fn successor(
        previous: &Self,
        authority_epoch: u64,
        data_epoch: u64,
        payload: Value,
    ) -> Result<Self, String> {
        previous.validate()?;
        if authority_epoch < previous.authority_epoch {
            return Err("security_object_authority_epoch_rollback".to_owned());
        }
        if data_epoch < previous.data_epoch {
            return Err("security_object_data_epoch_rollback".to_owned());
        }
        let sequence = previous
            .sequence
            .checked_add(1)
            .ok_or_else(|| "security_object_sequence_exhausted".to_owned())?;
        Self::build(
            previous.object_id,
            previous.object_kind,
            authority_epoch,
            data_epoch,
            sequence,
            Some(previous.envelope_digest.clone()),
            payload,
        )
    }

    fn build(
        object_id: SecurityEventId,
        object_kind: SecurityObjectKind,
        authority_epoch: u64,
        data_epoch: u64,
        sequence: u64,
        previous_digest: Option<String>,
        payload: Value,
    ) -> Result<Self, String> {
        let payload_digest = json_digest(&payload);
        let mut envelope = Self {
            schema: SECURITY_OBJECT_SCHEMA.to_owned(),
            version: SECURITY_OBJECT_VERSION,
            object_id,
            object_kind,
            authority_epoch,
            data_epoch,
            sequence,
            previous_digest,
            payload_digest,
            payload,
            envelope_digest: String::new(),
        };
        envelope.envelope_digest = envelope.digest();
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let envelope: Self = serde_json::from_value(value.clone())
            .map_err(|_| "security_object_decode_failed".to_owned())?;
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "security_object_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_OBJECT_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_OBJECT_VERSION)
            || self.object_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.sequence == 0
            || !self.payload.is_object()
        {
            return Err("security_object_header_invalid".to_owned());
        }
        check_schema_compatibility(&self.schema, &self.version)
            .map_err(|_| "security_object_unknown_major".to_owned())?;
        if contains_raw_secret(&self.payload, 0) {
            return Err("security_object_secret_field".to_owned());
        }
        let bytes = canonical_journal_bytes(&self.payload)
            .map_err(|_| "security_object_payload_not_canonical".to_owned())?;
        if bytes.len() > MAX_SECURITY_OBJECT_BYTES {
            return Err("security_object_payload_too_large".to_owned());
        }
        if self.payload_digest != json_digest(&self.payload) {
            return Err("security_object_payload_digest_mismatch".to_owned());
        }
        validate_digest(&self.payload_digest, "security_object_payload_digest")?;
        if self.sequence == 1 {
            if self.previous_digest.is_some() {
                return Err("security_object_genesis_parent_unexpected".to_owned());
            }
        } else {
            let previous = self
                .previous_digest
                .as_deref()
                .ok_or_else(|| "security_object_parent_required".to_owned())?;
            validate_digest(previous, "security_object_previous_digest")?;
            if previous == self.envelope_digest {
                return Err("security_object_parent_self".to_owned());
            }
        }
        validate_digest(&self.envelope_digest, "security_object_digest")?;
        if self.envelope_digest != self.digest() {
            return Err("security_object_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_successor(&self, previous: &Self) -> Result<(), String> {
        previous.validate()?;
        self.validate()?;
        if self.object_id != previous.object_id || self.object_kind != previous.object_kind {
            return Err("security_object_identity_changed".to_owned());
        }
        if self.sequence <= previous.sequence {
            return Err("security_object_sequence_rollback".to_owned());
        }
        if self.authority_epoch < previous.authority_epoch {
            return Err("security_object_authority_epoch_rollback".to_owned());
        }
        if self.data_epoch < previous.data_epoch {
            return Err("security_object_data_epoch_rollback".to_owned());
        }
        if self.previous_digest.as_deref() != Some(previous.envelope_digest.as_str()) {
            return Err("security_object_digest_rollback".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "object_id": self.object_id,
            "object_kind": self.object_kind,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "sequence": self.sequence,
            "previous_digest": self.previous_digest,
            "payload_digest": self.payload_digest,
            "payload": self.payload,
        }))
    }
}

/// Explicitly named alias for callers that model the envelope as an event.
pub type SecurityEventEnvelope = SecurityObjectEnvelope;

/// Parse only the current security envelope.  There is no implicit migration from an unknown or
/// older major: a future upcaster must be explicitly registered by a later roadmap step.
pub fn parse_security_object(value: &Value) -> Result<SecurityObjectEnvelope, String> {
    SecurityObjectEnvelope::from_json(value)
}

/// Upcast boundary for security objects.  No migrations are registered in SC-02, so a current
/// payload parses and every other schema fails closed with a stable class of error.
pub fn upcast_security_object(
    value: Value,
    expected_schema: &str,
) -> Result<SecurityObjectEnvelope, String> {
    if schema_contract(expected_schema).is_none() {
        return Err("security_schema_expected_unknown".to_owned());
    }
    let incoming = value
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| "security_schema_required".to_owned())?;
    if incoming != expected_schema {
        if schema_major(incoming) != schema_major(expected_schema) {
            return Err("security_schema_unknown_major".to_owned());
        }
        return Err("security_schema_migration_missing".to_owned());
    }
    if expected_schema != SECURITY_OBJECT_SCHEMA {
        return Err("security_schema_object_expected".to_owned());
    }
    parse_security_object(&value)
}

/// Upcast boundary for registry snapshots; SC-02 intentionally has no legacy registry migration.
pub fn upcast_security_registry(
    value: Value,
    expected_schema: &str,
) -> Result<SecuritySchemaRegistry, String> {
    if expected_schema != SECURITY_SCHEMA_REGISTRY_SCHEMA {
        return Err("security_schema_registry_expected".to_owned());
    }
    let incoming = value
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| "security_schema_required".to_owned())?;
    if incoming != expected_schema {
        if schema_major(incoming) != schema_major(expected_schema) {
            return Err("security_schema_unknown_major".to_owned());
        }
        return Err("security_schema_migration_missing".to_owned());
    }
    SecuritySchemaRegistry::from_json(&value)
}

fn contract_digest(contract: &SchemaContract) -> String {
    json_digest(&json!({
        "name": contract.name,
        "version": contract.version,
        "layer": format!("{:?}", contract.layer),
        "owner_crate": contract.owner_crate,
        "compatibility": format!("{:?}", contract.compatibility),
        "allow_unknown_fields": contract.allow_unknown_fields,
    }))
}

fn schema_major(schema: &str) -> Option<u32> {
    schema
        .rsplit_once(".v")
        .and_then(|(_, major)| major.parse::<u32>().ok())
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

fn stable_uuid(namespace: &str, key: &str) -> Uuid {
    let digest = Sha256::digest(format!("{namespace}\0{key}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn contains_raw_secret(value: &Value, depth: usize) -> bool {
    if depth > MAX_SECURITY_PAYLOAD_DEPTH {
        return true;
    }
    match value {
        Value::Object(object) => object.iter().any(|(key, child)| {
            let normalized = key.to_ascii_lowercase();
            let reference = normalized.ends_with("_ref")
                || normalized.ends_with("_id")
                || normalized.ends_with("_digest")
                || normalized.ends_with("_hash");
            let forbidden = matches!(
                normalized.as_str(),
                "secret"
                    | "secret_value"
                    | "token"
                    | "token_value"
                    | "password"
                    | "api_key"
                    | "access_token"
                    | "refresh_token"
                    | "authorization"
                    | "private_key"
                    | "credential"
                    | "credential_value"
            );
            (forbidden && !reference) || contains_raw_secret(child, depth + 1)
        }),
        Value::Array(values) => values
            .iter()
            .any(|child| contains_raw_secret(child, depth + 1)),
        Value::String(text) => {
            let normalized = text.to_ascii_lowercase();
            normalized.contains("bearer ")
                || normalized.starts_with("sk-")
                || normalized.contains("-----begin ")
        }
        _ => false,
    }
}

// Keep the imported ID names in this module's public API documentation and make accidental
// removal of a security ID visible to compilation-time consumers of the crate.
#[allow(dead_code)]
const SECURITY_TYPED_ID_NAMES: &[&str] = &[
    "SecurityRegistryId",
    "SecurityContextId",
    "SecurityPolicyId",
    "SecurityDecisionId",
    "SecurityEventId",
    "GrantId",
    "OperationId",
    "AuditId",
    "SecretRefId",
    "EvidenceRefId",
];

#[allow(dead_code)]
fn _typed_ids_for_future_security_steps(
    context: SecurityContextId,
    policy: SecurityPolicyId,
    decision: SecurityDecisionId,
    grant: GrantId,
    operation: OperationId,
    audit: AuditId,
    secret: SecretRefId,
    evidence: EvidenceRefId,
) -> (
    SecurityContextId,
    SecurityPolicyId,
    SecurityDecisionId,
    GrantId,
    OperationId,
    AuditId,
    SecretRefId,
    EvidenceRefId,
) {
    (
        context, policy, decision, grant, operation, audit, secret, evidence,
    )
}
