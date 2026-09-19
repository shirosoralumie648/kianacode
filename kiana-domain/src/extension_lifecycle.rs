//! Lifecycle, sandbox and permit-only callback contracts for extensions.
//!
//! These values describe a daemon-owned lifecycle transition. They do not load a package, run a
//! hook or grant a capability. A callback permit is bound to the enabled package/manifest and a
//! bounded sandbox profile; revoked, uninstalled or stale packages cannot mint one.

use crate::{json_digest, ExtensionEffect, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
