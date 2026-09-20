//! Opaque extension secrets, isolated state and bounded migration contracts.
//!
//! These values are admission and receipt contracts only.  They never contain secret material,
//! read a store, run migration code or make a capability decision.  The daemon must revalidate
//! the handle, scope and migration preconditions at the concrete effect boundary.

use crate::{json_digest, valid_extension_identifier, SchemaVersion, SecretRef};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const EXTENSION_SECRET_BINDING_SCHEMA: &str = "kiana.extension-secret-binding.v1";
pub const EXTENSION_CONFIGURATION_SNAPSHOT_SCHEMA: &str =
    "kiana.extension-configuration-snapshot.v1";
pub const EXTENSION_STATE_SCOPE_SCHEMA: &str = "kiana.extension-state-scope.v1";
pub const EXTENSION_STATE_MIGRATION_PLAN_SCHEMA: &str = "kiana.extension-state-migration-plan.v1";
pub const EXTENSION_STATE_MIGRATION_RECEIPT_SCHEMA: &str =
    "kiana.extension-state-migration-receipt.v1";
pub const EXTENSION_STATE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EXTENSION_SECRET_BINDINGS: usize = 64;
pub const MAX_EXTENSION_NAMESPACE: usize = 256;
pub const MAX_EXTENSION_MIGRATION_BYTES: u64 = 4 * 1024 * 1024;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    if value
        .strip_prefix("sha256:")
        .is_none_or(|hex| hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn namespace(value: &str, field: &str) -> Result<(), String> {
    required(value, field, MAX_EXTENSION_NAMESPACE)?;
    if value.starts_with('/')
        || value.contains('\\')
        || value.contains("//")
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn contains_raw_secret(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let normalized = key.to_ascii_lowercase();
            let forbidden = matches!(
                normalized.as_str(),
                "secret"
                    | "token"
                    | "password"
                    | "api_key"
                    | "access_token"
                    | "refresh_token"
                    | "authorization"
            );
            let reference = normalized.ends_with("_ref")
                || normalized.ends_with("_env")
                || normalized.ends_with("_id");
            (forbidden && !reference) || contains_raw_secret(value)
        }),
        Value::Array(values) => values.iter().any(contains_raw_secret),
        Value::String(text) => {
            let normalized = text.to_ascii_lowercase();
            normalized.contains("bearer ") || normalized.starts_with("sk-")
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionSecretBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub extension_id: String,
    pub scope_digest: String,
    pub config_key: String,
    pub secret_ref: SecretRef,
    pub destination: String,
    pub expires_at_unix_ms: u64,
    pub binding_digest: String,
}

impl ExtensionSecretBinding {
    pub fn new(
        extension_id: impl Into<String>,
        scope_digest: impl Into<String>,
        config_key: impl Into<String>,
        secret_ref: SecretRef,
        destination: impl Into<String>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: EXTENSION_SECRET_BINDING_SCHEMA.to_owned(),
            version: EXTENSION_STATE_VERSION,
            extension_id: extension_id.into(),
            scope_digest: scope_digest.into(),
            config_key: config_key.into(),
            secret_ref,
            destination: destination.into(),
            expires_at_unix_ms,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_SECRET_BINDING_SCHEMA
            || self.version != EXTENSION_STATE_VERSION
            || !valid_extension_identifier(&self.extension_id)
            || self.expires_at_unix_ms == 0
        {
            return Err("extension_secret_binding_header_invalid".to_owned());
        }
        digest(&self.scope_digest, "extension_secret_scope_digest")?;
        required(&self.config_key, "extension_secret_config_key", 128)?;
        required(&self.destination, "extension_secret_destination", 256)?;
        if self.destination.contains(['\n', '\r']) {
            return Err("extension_secret_destination_invalid".to_owned());
        }
        self.secret_ref.validate()?;
        digest(&self.binding_digest, "extension_secret_binding_digest")?;
        if self.binding_digest != self.digest() {
            return Err("extension_secret_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Reauthorize the opaque handle at the concrete destination immediately before use.
    pub fn validate_at(
        &self,
        now_unix_ms: u64,
        extension_id: &str,
        config_key: &str,
        destination: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("extension_secret_binding_expired".to_owned());
        }
        if self.extension_id != extension_id {
            return Err("extension_secret_extension_mismatch".to_owned());
        }
        if self.config_key != config_key {
            return Err("extension_secret_config_key_mismatch".to_owned());
        }
        if self.destination != destination {
            return Err("extension_secret_destination_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "scope_digest": self.scope_digest,
            "config_key": self.config_key,
            "secret_ref": self.secret_ref,
            "destination": self.destination,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionConfigurationSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub extension_id: String,
    pub scope_digest: String,
    pub public_config: Value,
    #[serde(default)]
    pub secret_bindings: BTreeMap<String, ExtensionSecretBinding>,
    pub config_digest: String,
}

impl ExtensionConfigurationSnapshot {
    pub fn new(
        extension_id: impl Into<String>,
        scope_digest: impl Into<String>,
        public_config: Value,
        secret_bindings: BTreeMap<String, ExtensionSecretBinding>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: EXTENSION_CONFIGURATION_SNAPSHOT_SCHEMA.to_owned(),
            version: EXTENSION_STATE_VERSION,
            extension_id: extension_id.into(),
            scope_digest: scope_digest.into(),
            public_config,
            secret_bindings,
            config_digest: String::new(),
        };
        snapshot.config_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_CONFIGURATION_SNAPSHOT_SCHEMA
            || self.version != EXTENSION_STATE_VERSION
            || !valid_extension_identifier(&self.extension_id)
            || self.secret_bindings.len() > MAX_EXTENSION_SECRET_BINDINGS
        {
            return Err("extension_configuration_snapshot_header_invalid".to_owned());
        }
        digest(&self.scope_digest, "extension_configuration_scope_digest")?;
        if contains_raw_secret(&self.public_config) {
            return Err("extension_configuration_raw_secret_forbidden".to_owned());
        }
        for (key, binding) in &self.secret_bindings {
            required(key, "extension_secret_binding_key", 128)?;
            binding.validate()?;
            if key != &binding.config_key
                || binding.extension_id != self.extension_id
                || binding.scope_digest != self.scope_digest
            {
                return Err("extension_configuration_secret_binding_mismatch".to_owned());
            }
        }
        digest(&self.config_digest, "extension_configuration_digest")?;
        if self.config_digest != self.digest() {
            return Err("extension_configuration_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "scope_digest": self.scope_digest,
            "public_config": self.public_config,
            "secret_bindings": self.secret_bindings,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionStateScope {
    pub schema: String,
    pub version: SchemaVersion,
    pub publisher: String,
    pub extension_id: String,
    pub scope_digest: String,
    pub state_namespace: String,
    pub cache_namespace: String,
    pub cache_read_only: bool,
    pub scope_contract_digest: String,
}

impl ExtensionStateScope {
    pub fn new(
        publisher: impl Into<String>,
        extension_id: impl Into<String>,
        scope_digest: impl Into<String>,
        state_namespace: impl Into<String>,
        cache_namespace: impl Into<String>,
    ) -> Result<Self, String> {
        let mut scope = Self {
            schema: EXTENSION_STATE_SCOPE_SCHEMA.to_owned(),
            version: EXTENSION_STATE_VERSION,
            publisher: publisher.into(),
            extension_id: extension_id.into(),
            scope_digest: scope_digest.into(),
            state_namespace: state_namespace.into(),
            cache_namespace: cache_namespace.into(),
            cache_read_only: true,
            scope_contract_digest: String::new(),
        };
        scope.scope_contract_digest = scope.digest();
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_STATE_SCOPE_SCHEMA
            || self.version != EXTENSION_STATE_VERSION
            || !valid_extension_identifier(&self.publisher)
            || !valid_extension_identifier(&self.extension_id)
            || !self.cache_read_only
            || self.state_namespace == self.cache_namespace
        {
            return Err("extension_state_scope_header_invalid".to_owned());
        }
        digest(&self.scope_digest, "extension_state_scope_digest")?;
        namespace(&self.state_namespace, "extension_state_namespace")?;
        namespace(&self.cache_namespace, "extension_cache_namespace")?;
        let prefix = format!("{}/{}", self.publisher, self.extension_id);
        if !self.state_namespace.starts_with(&prefix)
            || !self.cache_namespace.starts_with(&prefix)
            || !self.state_namespace.ends_with("/state")
            || !self.cache_namespace.ends_with("/cache")
        {
            return Err("extension_state_scope_isolation_invalid".to_owned());
        }
        digest(
            &self.scope_contract_digest,
            "extension_state_scope_contract_digest",
        )?;
        if self.scope_contract_digest != self.digest() {
            return Err("extension_state_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "publisher": self.publisher,
            "extension_id": self.extension_id,
            "scope_digest": self.scope_digest,
            "state_namespace": self.state_namespace,
            "cache_namespace": self.cache_namespace,
            "cache_read_only": self.cache_read_only,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionStateMigrationStatus {
    Committed,
    RetainedOld,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionStateMigrationPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub extension_id: String,
    pub publisher: String,
    pub scope_digest: String,
    pub from_schema_digest: String,
    pub from_state_digest: String,
    pub to_schema_digest: String,
    pub to_state_digest: String,
    pub backup_id: String,
    pub max_bytes: u64,
    pub expected_generation: u64,
    pub next_generation: u64,
    pub plan_digest: String,
}

impl ExtensionStateMigrationPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        extension_id: impl Into<String>,
        publisher: impl Into<String>,
        scope_digest: impl Into<String>,
        from_schema_digest: impl Into<String>,
        from_state_digest: impl Into<String>,
        to_schema_digest: impl Into<String>,
        to_state_digest: impl Into<String>,
        backup_id: impl Into<String>,
        max_bytes: u64,
        expected_generation: u64,
        next_generation: u64,
    ) -> Result<Self, String> {
        let mut plan = Self {
            schema: EXTENSION_STATE_MIGRATION_PLAN_SCHEMA.to_owned(),
            version: EXTENSION_STATE_VERSION,
            extension_id: extension_id.into(),
            publisher: publisher.into(),
            scope_digest: scope_digest.into(),
            from_schema_digest: from_schema_digest.into(),
            from_state_digest: from_state_digest.into(),
            to_schema_digest: to_schema_digest.into(),
            to_state_digest: to_state_digest.into(),
            backup_id: backup_id.into(),
            max_bytes,
            expected_generation,
            next_generation,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_STATE_MIGRATION_PLAN_SCHEMA
            || self.version != EXTENSION_STATE_VERSION
            || !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.publisher)
            || self.max_bytes == 0
            || self.max_bytes > MAX_EXTENSION_MIGRATION_BYTES
            || self.expected_generation == 0
            || self.next_generation != self.expected_generation.checked_add(1).unwrap_or(0)
        {
            return Err("extension_state_migration_plan_header_invalid".to_owned());
        }
        digest(&self.scope_digest, "extension_migration_scope_digest")?;
        for (value, field) in [
            (
                &self.from_schema_digest,
                "extension_migration_from_schema_digest",
            ),
            (
                &self.from_state_digest,
                "extension_migration_from_state_digest",
            ),
            (
                &self.to_schema_digest,
                "extension_migration_to_schema_digest",
            ),
            (&self.to_state_digest, "extension_migration_to_state_digest"),
        ] {
            digest(value, field)?;
        }
        if self.from_schema_digest == self.to_schema_digest
            && self.from_state_digest == self.to_state_digest
        {
            return Err("extension_state_migration_noop".to_owned());
        }
        required(&self.backup_id, "extension_migration_backup_id", 256)?;
        digest(&self.plan_digest, "extension_migration_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("extension_state_migration_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "publisher": self.publisher,
            "scope_digest": self.scope_digest,
            "from_schema_digest": self.from_schema_digest,
            "from_state_digest": self.from_state_digest,
            "to_schema_digest": self.to_schema_digest,
            "to_state_digest": self.to_state_digest,
            "backup_id": self.backup_id,
            "max_bytes": self.max_bytes,
            "expected_generation": self.expected_generation,
            "next_generation": self.next_generation,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionStateMigrationReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub plan_digest: String,
    pub status: ExtensionStateMigrationStatus,
    pub old_state_retained: bool,
    pub backup_verified: bool,
    pub old_state_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_state_digest: Option<String>,
    pub committed_generation: u64,
    pub reason: String,
    pub receipt_digest: String,
}

impl ExtensionStateMigrationReceipt {
    pub fn committed(
        plan: &ExtensionStateMigrationPlan,
        new_state_digest: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(
            plan,
            ExtensionStateMigrationStatus::Committed,
            true,
            true,
            Some(new_state_digest.into()),
            plan.next_generation,
            "migration_cas_committed",
        )
    }

    pub fn retained_old(
        plan: &ExtensionStateMigrationPlan,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(
            plan,
            ExtensionStateMigrationStatus::RetainedOld,
            true,
            true,
            None,
            0,
            reason,
        )
    }

    pub fn unknown(
        plan: &ExtensionStateMigrationPlan,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(
            plan,
            ExtensionStateMigrationStatus::Unknown,
            true,
            true,
            None,
            0,
            reason,
        )
    }

    fn new(
        plan: &ExtensionStateMigrationPlan,
        status: ExtensionStateMigrationStatus,
        old_state_retained: bool,
        backup_verified: bool,
        new_state_digest: Option<String>,
        committed_generation: u64,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        plan.validate()?;
        let mut receipt = Self {
            schema: EXTENSION_STATE_MIGRATION_RECEIPT_SCHEMA.to_owned(),
            version: EXTENSION_STATE_VERSION,
            plan_digest: plan.plan_digest.clone(),
            status,
            old_state_retained,
            backup_verified,
            old_state_digest: plan.from_state_digest.clone(),
            new_state_digest,
            committed_generation,
            reason: reason.into(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_STATE_MIGRATION_RECEIPT_SCHEMA
            || self.version != EXTENSION_STATE_VERSION
            || !self.old_state_retained
            || !self.backup_verified
            || self.reason.trim().is_empty()
            || self.reason.len() > 2_048
        {
            return Err("extension_state_migration_receipt_header_invalid".to_owned());
        }
        digest(&self.plan_digest, "extension_migration_receipt_plan_digest")?;
        digest(
            &self.old_state_digest,
            "extension_migration_old_state_digest",
        )?;
        if let Some(new_state_digest) = &self.new_state_digest {
            digest(new_state_digest, "extension_migration_new_state_digest")?;
        }
        match self.status {
            ExtensionStateMigrationStatus::Committed
                if self.committed_generation == 0 || self.new_state_digest.is_none() =>
            {
                return Err("extension_state_migration_commit_incomplete".to_owned());
            }
            ExtensionStateMigrationStatus::RetainedOld | ExtensionStateMigrationStatus::Unknown
                if self.committed_generation != 0 || self.new_state_digest.is_some() =>
            {
                return Err("extension_state_migration_old_version_not_retained".to_owned());
            }
            _ => {}
        }
        digest(&self.receipt_digest, "extension_migration_receipt_digest")?;
        if self.receipt_digest != self.digest() {
            return Err("extension_state_migration_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "plan_digest": self.plan_digest,
            "status": self.status,
            "old_state_retained": self.old_state_retained,
            "backup_verified": self.backup_verified,
            "old_state_digest": self.old_state_digest,
            "new_state_digest": self.new_state_digest,
            "committed_generation": self.committed_generation,
            "reason": self.reason,
        }))
    }
}
