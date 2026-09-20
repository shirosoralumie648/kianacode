//! Lifecycle, sandbox and permit-only callback contracts for extensions.
//!
//! These values describe a daemon-owned lifecycle transition. They do not load a package, run a
//! hook or grant a capability. A callback permit is bound to the enabled package/manifest and a
//! bounded sandbox profile; revoked, uninstalled or stale packages cannot mint one.

use crate::{
    is_hex_bytes, json_digest, valid_extension_identifier, ExtensionEffect, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const EXTENSION_LIFECYCLE_SCHEMA: &str = "kiana.extension-lifecycle-record.v1";
pub const EXTENSION_CALLBACK_PERMIT_SCHEMA: &str = "kiana.extension-callback-permit.v1";
pub const EXTENSION_LIFECYCLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EXTENSION_CALLBACK_CAPABILITIES: usize = 64;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn hex_digest(value: &str, field: &str, bytes: usize) -> Result<(), String> {
    if value.len() != bytes * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn sha_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn read_only_capability(value: &str) -> bool {
    matches!(
        value,
        "memory.search" | "context.query" | "extension.inspect"
    )
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionLifecyclePhase {
    Inspected,
    Staged,
    Enabled,
    Disabled,
    Revoked,
    RolledBack,
    Uninstalled,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionSandboxProfile {
    ReadOnly,
    WorkspaceWrite,
    NetworkIsolated,
}

impl ExtensionSandboxProfile {
    pub const fn allows_network(self) -> bool {
        matches!(self, Self::NetworkIsolated)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionCallbackPermit {
    pub schema: String,
    pub version: SchemaVersion,
    pub extension_id: String,
    pub package_sha256: String,
    pub manifest_digest: String,
    pub operation: String,
    pub phase: ExtensionLifecyclePhase,
    pub effect: ExtensionEffect,
    pub sandbox: ExtensionSandboxProfile,
    pub capabilities: BTreeSet<String>,
    pub expires_at_ms: u64,
    pub permit_digest: String,
}

impl ExtensionCallbackPermit {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        extension_id: impl Into<String>,
        package_sha256: impl Into<String>,
        manifest_digest: impl Into<String>,
        operation: impl Into<String>,
        phase: ExtensionLifecyclePhase,
        effect: ExtensionEffect,
        sandbox: ExtensionSandboxProfile,
        capabilities: BTreeSet<String>,
        expires_at_ms: u64,
    ) -> Result<Self, String> {
        let mut permit = Self {
            schema: EXTENSION_CALLBACK_PERMIT_SCHEMA.to_owned(),
            version: EXTENSION_LIFECYCLE_VERSION,
            extension_id: extension_id.into(),
            package_sha256: package_sha256.into(),
            manifest_digest: manifest_digest.into(),
            operation: operation.into(),
            phase,
            effect,
            sandbox,
            capabilities,
            expires_at_ms,
            permit_digest: String::new(),
        };
        permit.permit_digest = permit.digest();
        permit.validate()?;
        Ok(permit)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_CALLBACK_PERMIT_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXTENSION_LIFECYCLE_VERSION)
            || self.phase != ExtensionLifecyclePhase::Enabled
        {
            return Err("extension_callback_permit_phase_invalid".to_owned());
        }
        required(&self.extension_id, "extension_callback_extension", 128)?;
        hex_digest(&self.package_sha256, "extension_callback_package", 32)?;
        sha_digest(&self.manifest_digest, "extension_callback_manifest")?;
        required(&self.operation, "extension_callback_operation", 256)?;
        if self.operation.chars().any(char::is_whitespace) || self.expires_at_ms == 0 {
            return Err("extension_callback_permit_boundary_invalid".to_owned());
        }
        if self.capabilities.len() > MAX_EXTENSION_CALLBACK_CAPABILITIES
            || self
                .capabilities
                .iter()
                .any(|capability| capability.trim().is_empty() || capability.len() > 256)
        {
            return Err("extension_callback_capability_limit".to_owned());
        }
        if self.effect == ExtensionEffect::ReadOnly
            && (self.sandbox != ExtensionSandboxProfile::ReadOnly
                || self
                    .capabilities
                    .iter()
                    .any(|capability| !read_only_capability(capability)))
        {
            return Err("extension_callback_read_only_write_denied".to_owned());
        }
        if !self.sandbox.allows_network() && self.operation.starts_with("network.") {
            return Err("extension_callback_network_denied".to_owned());
        }
        sha_digest(&self.permit_digest, "extension_callback_permit_digest")?;
        if self.permit_digest != self.digest() {
            return Err("extension_callback_permit_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "package_sha256": self.package_sha256,
            "manifest_digest": self.manifest_digest,
            "operation": self.operation,
            "phase": self.phase,
            "effect": self.effect,
            "sandbox": self.sandbox,
            "capabilities": self.capabilities,
            "expires_at_ms": self.expires_at_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionLifecycleRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub extension_id: String,
    pub package_sha256: String,
    pub from_phase: Option<ExtensionLifecyclePhase>,
    pub to_phase: ExtensionLifecyclePhase,
    pub actor_ref: String,
    pub registry_revision: u64,
    pub reason: String,
    pub callback_permit_digest: Option<String>,
    pub record_digest: String,
}

impl ExtensionLifecycleRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn transition(
        extension_id: impl Into<String>,
        package_sha256: impl Into<String>,
        from_phase: Option<ExtensionLifecyclePhase>,
        to_phase: ExtensionLifecyclePhase,
        actor_ref: impl Into<String>,
        registry_revision: u64,
        reason: impl Into<String>,
        callback_permit_digest: Option<String>,
    ) -> Result<Self, String> {
        let record = Self {
            schema: EXTENSION_LIFECYCLE_SCHEMA.to_owned(),
            version: EXTENSION_LIFECYCLE_VERSION,
            extension_id: extension_id.into(),
            package_sha256: package_sha256.into(),
            from_phase,
            to_phase,
            actor_ref: actor_ref.into(),
            registry_revision,
            reason: reason.into(),
            callback_permit_digest,
            record_digest: String::new(),
        };
        record.validate_with_digest()
    }

    fn validate_with_digest(mut self) -> Result<Self, String> {
        self.record_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_LIFECYCLE_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXTENSION_LIFECYCLE_VERSION)
        {
            return Err("extension_lifecycle_schema_invalid".to_owned());
        }
        required(&self.extension_id, "extension_lifecycle_extension", 128)?;
        hex_digest(&self.package_sha256, "extension_lifecycle_package", 32)?;
        required(&self.actor_ref, "extension_lifecycle_actor", 256)?;
        required(&self.reason, "extension_lifecycle_reason", 1_024)?;
        if self.registry_revision == 0 {
            return Err("extension_lifecycle_revision_invalid".to_owned());
        }
        if !valid_transition(self.from_phase, self.to_phase) {
            return Err("extension_lifecycle_transition_invalid".to_owned());
        }
        if let Some(digest) = &self.callback_permit_digest {
            sha_digest(digest, "extension_lifecycle_permit_digest")?;
        }
        sha_digest(&self.record_digest, "extension_lifecycle_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("extension_lifecycle_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "package_sha256": self.package_sha256,
            "from_phase": self.from_phase,
            "to_phase": self.to_phase,
            "actor_ref": self.actor_ref,
            "registry_revision": self.registry_revision,
            "reason": self.reason,
            "callback_permit_digest": self.callback_permit_digest,
        }))
    }
}

pub fn valid_transition(
    from_phase: Option<ExtensionLifecyclePhase>,
    to_phase: ExtensionLifecyclePhase,
) -> bool {
    match (from_phase, to_phase) {
        (None, ExtensionLifecyclePhase::Inspected | ExtensionLifecyclePhase::Staged) => true,
        (Some(ExtensionLifecyclePhase::Inspected), ExtensionLifecyclePhase::Staged) => true,
        (
            Some(ExtensionLifecyclePhase::Staged),
            ExtensionLifecyclePhase::Enabled
            | ExtensionLifecyclePhase::Disabled
            | ExtensionLifecyclePhase::Revoked
            | ExtensionLifecyclePhase::Uninstalled,
        ) => true,
        (
            Some(ExtensionLifecyclePhase::Enabled),
            ExtensionLifecyclePhase::Disabled
            | ExtensionLifecyclePhase::Revoked
            | ExtensionLifecyclePhase::RolledBack
            | ExtensionLifecyclePhase::Uninstalled,
        ) => true,
        (
            Some(ExtensionLifecyclePhase::Disabled),
            ExtensionLifecyclePhase::Enabled
            | ExtensionLifecyclePhase::Revoked
            | ExtensionLifecyclePhase::Uninstalled,
        ) => true,
        (
            Some(ExtensionLifecyclePhase::RolledBack),
            ExtensionLifecyclePhase::Enabled
            | ExtensionLifecyclePhase::Disabled
            | ExtensionLifecyclePhase::Revoked,
        ) => true,
        (Some(ExtensionLifecyclePhase::Revoked), ExtensionLifecyclePhase::Uninstalled) => true,
        _ => false,
    }
}

pub const EXTENSION_LIFECYCLE_MUTATION_SCHEMA: &str = "kiana.extension-lifecycle-mutation.v1";
pub const EXTENSION_CONFIG_SNAPSHOT_SCHEMA: &str = "kiana.extension-config-snapshot.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionLifecycleAction {
    Inspect,
    Stage,
    Install,
    Enable,
}

impl ExtensionLifecycleAction {
    pub const fn mutates_registry(self) -> bool {
        !matches!(self, Self::Inspect)
    }

    pub const fn requires_package(self) -> bool {
        !matches!(self, Self::Inspect)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionConfigScope {
    Host,
    User,
    Project,
    Run,
}

impl ExtensionConfigScope {
    const fn precedence(self) -> u8 {
        match self {
            Self::Host => 0,
            Self::User => 1,
            Self::Project => 2,
            Self::Run => 3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionConfigEntry {
    pub key: String,
    pub scope: ExtensionConfigScope,
    pub source_ref: String,
    pub value_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_handle: Option<String>,
}

impl ExtensionConfigEntry {
    fn validate(&self) -> Result<(), String> {
        required(&self.key, "extension_config_key", 256)?;
        required(&self.source_ref, "extension_config_source", 512)?;
        sha_digest(&self.value_digest, "extension_config_value_digest")?;
        if self
            .secret_handle
            .as_deref()
            .is_some_and(|handle| !valid_extension_identifier(handle) || handle.len() > 256)
        {
            return Err("extension_config_secret_handle_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionConfigSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub extension_id: String,
    pub registry_version: u64,
    pub entries: Vec<ExtensionConfigEntry>,
    pub snapshot_digest: String,
}

impl ExtensionConfigSnapshot {
    pub fn new(
        extension_id: impl Into<String>,
        registry_version: u64,
        mut entries: Vec<ExtensionConfigEntry>,
    ) -> Result<Self, String> {
        entries.sort_by(|left, right| (&left.key, left.scope).cmp(&(&right.key, right.scope)));
        let mut snapshot = Self {
            schema: EXTENSION_CONFIG_SNAPSHOT_SCHEMA.to_owned(),
            version: EXTENSION_LIFECYCLE_VERSION,
            extension_id: extension_id.into(),
            registry_version,
            entries,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_CONFIG_SNAPSHOT_SCHEMA
            || self.version != EXTENSION_LIFECYCLE_VERSION
            || !valid_extension_identifier(&self.extension_id)
            || self.registry_version == 0
            || self.entries.len() > 256
        {
            return Err("extension_config_snapshot_invalid".to_owned());
        }
        for entry in &self.entries {
            entry.validate()?;
        }
        if self
            .entries
            .windows(2)
            .any(|pair| pair[0].key == pair[1].key && pair[0].scope == pair[1].scope)
        {
            return Err("extension_config_duplicate_scope".to_owned());
        }
        sha_digest(&self.snapshot_digest, "extension_config_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("extension_config_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn effective_entries(&self) -> Result<Vec<ExtensionConfigEntry>, String> {
        self.validate()?;
        let mut selected = BTreeMap::<String, ExtensionConfigEntry>::new();
        for entry in &self.entries {
            let replace = selected
                .get(&entry.key)
                .is_none_or(|current| entry.scope.precedence() > current.scope.precedence());
            if replace {
                selected.insert(entry.key.clone(), entry.clone());
            }
        }
        Ok(selected.values().cloned().collect())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "registry_version": self.registry_version,
            "entries": self.entries,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionLifecycleMutation {
    pub schema: String,
    pub version: SchemaVersion,
    pub action: ExtensionLifecycleAction,
    pub extension_id: String,
    pub project_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
    pub expected_registry_version: u64,
    pub idempotency_key: String,
    pub actor_ref: String,
    pub reason: String,
    pub trust_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_snapshot_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_snapshot_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    pub mutation_digest: String,
}

impl ExtensionLifecycleMutation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action: ExtensionLifecycleAction,
        extension_id: impl Into<String>,
        project_root: impl Into<String>,
        package_sha256: Option<String>,
        expected_registry_version: u64,
        idempotency_key: impl Into<String>,
        actor_ref: impl Into<String>,
        reason: impl Into<String>,
        trust_revision: impl Into<String>,
        dependency_snapshot_digest: Option<String>,
        config_snapshot_digest: Option<String>,
        approval_id: Option<String>,
    ) -> Result<Self, String> {
        let mut mutation = Self {
            schema: EXTENSION_LIFECYCLE_MUTATION_SCHEMA.to_owned(),
            version: EXTENSION_LIFECYCLE_VERSION,
            action,
            extension_id: extension_id.into(),
            project_root: project_root.into(),
            package_sha256,
            expected_registry_version,
            idempotency_key: idempotency_key.into(),
            actor_ref: actor_ref.into(),
            reason: reason.into(),
            trust_revision: trust_revision.into(),
            dependency_snapshot_digest,
            config_snapshot_digest,
            approval_id,
            mutation_digest: String::new(),
        };
        mutation.mutation_digest = mutation.digest();
        mutation.validate()?;
        Ok(mutation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_LIFECYCLE_MUTATION_SCHEMA
            || self.version != EXTENSION_LIFECYCLE_VERSION
            || !valid_extension_identifier(&self.extension_id)
            || self.project_root.trim().is_empty()
            || self.project_root.len() > 4_096
            || self.project_root.contains('\0')
            || self.expected_registry_version == 0
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.len() > 128
            || self.idempotency_key.chars().any(char::is_control)
            || self.actor_ref.trim().is_empty()
            || self.actor_ref.len() > 256
            || self.reason.trim().is_empty()
            || self.reason.len() > 4_096
            || self.trust_revision.trim().is_empty()
            || self.trust_revision.len() > 256
        {
            return Err("extension_lifecycle_mutation_invalid".to_owned());
        }
        if self
            .package_sha256
            .as_deref()
            .is_some_and(|digest| !is_hex_bytes(digest, 32))
        {
            return Err("extension_lifecycle_package_invalid".to_owned());
        }
        for (digest, field) in [
            (
                self.dependency_snapshot_digest.as_deref(),
                "extension_lifecycle_dependency_snapshot",
            ),
            (
                self.config_snapshot_digest.as_deref(),
                "extension_lifecycle_config_snapshot",
            ),
        ] {
            if let Some(digest) = digest {
                sha_digest(digest, field)?;
            }
        }
        if self
            .approval_id
            .as_deref()
            .is_some_and(|approval| !valid_extension_identifier(approval))
        {
            return Err("extension_lifecycle_approval_invalid".to_owned());
        }
        if self.action.requires_package() && self.package_sha256.is_none() {
            return Err("extension_lifecycle_package_required".to_owned());
        }
        if self.action.mutates_registry() && self.dependency_snapshot_digest.is_none() {
            return Err("extension_lifecycle_dependency_snapshot_required".to_owned());
        }
        if matches!(self.action, ExtensionLifecycleAction::Enable)
            && (self.approval_id.is_none() || self.config_snapshot_digest.is_none())
        {
            return Err("extension_enable_approval_and_config_required".to_owned());
        }
        sha_digest(&self.mutation_digest, "extension_lifecycle_mutation_digest")?;
        if self.mutation_digest != self.digest() {
            return Err("extension_lifecycle_mutation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "action": self.action,
            "extension_id": self.extension_id,
            "project_root": self.project_root,
            "package_sha256": self.package_sha256,
            "expected_registry_version": self.expected_registry_version,
            "idempotency_key": self.idempotency_key,
            "actor_ref": self.actor_ref,
            "reason": self.reason,
            "trust_revision": self.trust_revision,
            "dependency_snapshot_digest": self.dependency_snapshot_digest,
            "config_snapshot_digest": self.config_snapshot_digest,
            "approval_id": self.approval_id,
        }))
    }
}
