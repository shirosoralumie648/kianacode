//! Versioned migration registry contract.
//!
//! This module defines the immutable input to a future migration runner. It does not apply a
//! migration, acquire a writer lock, or create a backup. Every step is forward-only and must
//! carry an owner, a verified-backup requirement, a source precondition and a bounded
//! compatibility window.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MIGRATION_REGISTRY_SCHEMA: &str = "kiana.migration-registry.v1";
pub const MIGRATION_REGISTRY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_MIGRATION_STEPS: usize = 128;
pub const MAX_MIGRATION_TEXT: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationReleaseBinding {
    pub release_manifest_digest: String,
    pub artifact_digest: String,
    pub signature_digest: String,
}

impl MigrationReleaseBinding {
    pub fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (
                &self.release_manifest_digest,
                "migration_release_manifest_digest",
            ),
            (&self.artifact_digest, "migration_artifact_digest"),
            (&self.signature_digest, "migration_signature_digest"),
        ] {
            validate_digest(value, field)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationCompatibilityWindow {
    pub min_reader_version: u32,
    pub max_reader_version: u32,
    pub expires_at_unix_ms: u64,
}

impl MigrationCompatibilityWindow {
    pub fn validate(&self) -> Result<(), String> {
        if self.min_reader_version > self.max_reader_version || self.expires_at_unix_ms == 0 {
            return Err("migration_compatibility_window_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPrecondition {
    pub source_schema_digest: String,
    pub expected_source_revision: u64,
    pub required_owner: String,
    pub requires_verified_backup: bool,
}

impl MigrationPrecondition {
    pub fn validate(&self) -> Result<(), String> {
        validate_digest(&self.source_schema_digest, "migration_source_schema_digest")?;
        if self.expected_source_revision == 0
            || !bounded(&self.required_owner, MAX_MIGRATION_TEXT)
            || !self.requires_verified_backup
        {
            return Err("migration_precondition_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationStep {
    pub step_id: String,
    pub ordinal: u32,
    pub from_format_version: u32,
    pub to_format_version: u32,
    pub checksum: String,
    pub owner: String,
    pub backup_required: bool,
    pub precondition: MigrationPrecondition,
    pub compatibility_window: MigrationCompatibilityWindow,
    pub upcaster: String,
}

impl MigrationStep {
    pub fn validate(&self) -> Result<(), String> {
        if !bounded(&self.step_id, MAX_MIGRATION_TEXT)
            || self.ordinal == 0
            || self.from_format_version >= self.to_format_version
            || !bounded(&self.owner, MAX_MIGRATION_TEXT)
            || !self.backup_required
            || !bounded(&self.upcaster, MAX_MIGRATION_TEXT)
        {
            return Err("migration_step_header_invalid".to_owned());
        }
        validate_digest(&self.checksum, "migration_step_checksum")?;
        self.precondition.validate()?;
        if self.precondition.required_owner != self.owner {
            return Err("migration_step_owner_precondition_mismatch".to_owned());
        }
        self.compatibility_window.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRegistry {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_revision: u64,
    pub release: MigrationReleaseBinding,
    pub steps: Vec<MigrationStep>,
    pub registry_digest: String,
}

impl MigrationRegistry {
    pub fn new(
        release: MigrationReleaseBinding,
        steps: Vec<MigrationStep>,
    ) -> Result<Self, String> {
        let mut registry = Self {
            schema: MIGRATION_REGISTRY_SCHEMA.to_owned(),
            version: MIGRATION_REGISTRY_VERSION,
            registry_revision: 1,
            release,
            steps,
            registry_digest: String::new(),
        };
        registry.registry_digest = registry.digest();
        registry.validate()?;
        Ok(registry)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_REGISTRY_SCHEMA
            || self.version != MIGRATION_REGISTRY_VERSION
            || self.registry_revision == 0
            || self.steps.is_empty()
            || self.steps.len() > MAX_MIGRATION_STEPS
        {
            return Err("migration_registry_header_invalid".to_owned());
        }
        self.release.validate()?;
        validate_digest(&self.registry_digest, "migration_registry_digest")?;
        if self.registry_digest != self.digest() {
            return Err("migration_registry_digest_mismatch".to_owned());
        }

        let mut ids = BTreeSet::new();
        let mut checksums = BTreeSet::new();
        let mut previous_to = None;
        for (index, step) in self.steps.iter().enumerate() {
            step.validate()?;
            if step.ordinal != (index as u32) + 1
                || !ids.insert(step.step_id.as_str())
                || !checksums.insert(step.checksum.as_str())
            {
                return Err("migration_registry_order_or_duplicate_invalid".to_owned());
            }
            if let Some(previous_to) = previous_to {
                if step.from_format_version != previous_to {
                    return Err("migration_registry_version_gap".to_owned());
                }
            }
            previous_to = Some(step.to_format_version);
        }
        Ok(())
    }

    pub fn validate_for_release(
        &self,
        release_manifest_digest: &str,
        artifact_digest: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.release.release_manifest_digest != release_manifest_digest {
            return Err("migration_release_manifest_drift".to_owned());
        }
        if self.release.artifact_digest != artifact_digest {
            return Err("migration_artifact_drift".to_owned());
        }
        Ok(())
    }

    pub fn ordered_steps(&self) -> &[MigrationStep] {
        &self.steps
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "registry_revision": self.registry_revision,
            "release": self.release,
            "steps": self.steps,
        }))
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
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
