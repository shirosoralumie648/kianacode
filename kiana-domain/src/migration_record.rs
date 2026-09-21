//! MigrationRecord binding ordered registry, preflight, verified backup and runner outcome.
//!
//! This is a pure durable-record contract. It never acquires a lock or applies an upcaster.

use crate::{json_digest, MigrationRegistry};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const MIGRATION_RECORD_SCHEMA: &str = "kiana.migration-record.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationRecordStatus {
    Planned,
    Running,
    Applied,
    Failed,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRecord {
    pub schema: String,
    pub migration_id: String,
    pub registry_digest: String,
    pub preflight_digest: String,
    pub backup_snapshot_id: String,
    pub owner: String,
    pub from_format_version: u32,
    pub to_format_version: u32,
    pub status: MigrationRecordStatus,
    pub attempt: u32,
    pub failure_reason: Option<String>,
    pub record_digest: String,
}

impl MigrationRecord {
    pub fn planned(
        registry: &MigrationRegistry,
        migration_id: impl Into<String>,
        preflight_digest: impl Into<String>,
        backup_snapshot_id: impl Into<String>,
        owner: impl Into<String>,
    ) -> Result<Self, String> {
        registry.validate()?;
        let first = registry
            .ordered_steps()
            .first()
            .ok_or_else(|| "migration_registry_header_invalid".to_owned())?;
        let last = registry
            .ordered_steps()
            .last()
            .ok_or_else(|| "migration_registry_header_invalid".to_owned())?;
        let mut record = Self {
            schema: MIGRATION_RECORD_SCHEMA.to_owned(),
            migration_id: migration_id.into(),
            registry_digest: registry.registry_digest.clone(),
            preflight_digest: preflight_digest.into(),
            backup_snapshot_id: backup_snapshot_id.into(),
            owner: owner.into(),
            from_format_version: first.from_format_version,
            to_format_version: last.to_format_version,
            status: MigrationRecordStatus::Planned,
            attempt: 0,
            failure_reason: None,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate_against(registry)?;
        Ok(record)
    }

    pub fn transition(
        &self,
        registry: &MigrationRegistry,
        status: MigrationRecordStatus,
        reason: Option<String>,
    ) -> Result<Self, String> {
        self.validate_against(registry)?;
        let allowed = matches!(
            (self.status, status),
            (
                MigrationRecordStatus::Planned,
                MigrationRecordStatus::Running
            ) | (
                MigrationRecordStatus::Running,
                MigrationRecordStatus::Applied
            ) | (
                MigrationRecordStatus::Running,
                MigrationRecordStatus::Failed
            ) | (
                MigrationRecordStatus::Running,
                MigrationRecordStatus::Quarantined
            ) | (
                MigrationRecordStatus::Failed,
                MigrationRecordStatus::Running
            )
        );
        if !allowed {
            return Err("migration_record_transition_invalid".to_owned());
        }
        if status == MigrationRecordStatus::Applied && reason.is_some() {
            return Err("migration_record_applied_reason_forbidden".to_owned());
        }
        if matches!(
            status,
            MigrationRecordStatus::Failed | MigrationRecordStatus::Quarantined
        ) && reason.as_deref().is_none_or(str::is_empty)
        {
            return Err("migration_record_failure_reason_required".to_owned());
        }
        let mut next = self.clone();
        next.status = status;
        next.failure_reason = reason;
        next.attempt = if status == MigrationRecordStatus::Running {
            next.attempt.saturating_add(1)
        } else {
            next.attempt
        };
        next.record_digest = next.digest();
        next.validate_against(registry)?;
        Ok(next)
    }

    pub fn validate_against(&self, registry: &MigrationRegistry) -> Result<(), String> {
        registry.validate()?;
        if self.schema != MIGRATION_RECORD_SCHEMA
            || self.registry_digest != registry.registry_digest
            || self.from_format_version
                != registry
                    .ordered_steps()
                    .first()
                    .map(|step| step.from_format_version)
                    .unwrap_or(0)
            || self.to_format_version
                != registry
                    .ordered_steps()
                    .last()
                    .map(|step| step.to_format_version)
                    .unwrap_or(0)
            || self.attempt > 128
        {
            return Err("migration_record_binding_invalid".to_owned());
        }
        for (value, field) in [
            (&self.migration_id, "migration_record_id"),
            (&self.preflight_digest, "migration_record_preflight_digest"),
            (&self.backup_snapshot_id, "migration_record_backup_snapshot"),
            (&self.owner, "migration_record_owner"),
        ] {
            if value.trim().is_empty() || value.len() > 512 || value.contains('\0') {
                return Err(format!("{field}_invalid"));
            }
        }
        for (value, field) in [
            (&self.registry_digest, "migration_record_registry_digest"),
            (&self.preflight_digest, "migration_record_preflight_digest"),
        ] {
            let Some(hex) = value.strip_prefix("sha256:") else {
                return Err(format!("{field}_invalid"));
            };
            if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.status == MigrationRecordStatus::Planned && self.attempt != 0 {
            return Err("migration_record_planned_attempt_invalid".to_owned());
        }
        if self.status == MigrationRecordStatus::Applied && self.failure_reason.is_some() {
            return Err("migration_record_applied_failure_forbidden".to_owned());
        }
        if matches!(
            self.status,
            MigrationRecordStatus::Failed | MigrationRecordStatus::Quarantined
        ) && self.failure_reason.as_deref().is_none_or(str::is_empty)
        {
            return Err("migration_record_failure_reason_required".to_owned());
        }
        let Some(hex) = self.record_digest.strip_prefix("sha256:") else {
            return Err("migration_record_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("migration_record_digest_invalid".to_owned());
        }
        if self.record_digest != self.digest() {
            return Err("migration_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "migration_id": self.migration_id,
            "registry_digest": self.registry_digest,
            "preflight_digest": self.preflight_digest,
            "backup_snapshot_id": self.backup_snapshot_id,
            "owner": self.owner,
            "from_format_version": self.from_format_version,
            "to_format_version": self.to_format_version,
            "status": self.status,
            "attempt": self.attempt,
            "failure_reason": self.failure_reason,
        }))
    }
}
