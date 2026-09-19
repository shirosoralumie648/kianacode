//! Migration rollback decision and receipt contract.
//!
//! A rollback decision is a read-only gate. It never restores a root, kills a writer, retries an
//! external effect, or deletes the retained old root. Those effects require the later runner and
//! explicit approval/fence evidence.

use crate::{json_digest, MigrationRegistry, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const MIGRATION_ROLLBACK_SCHEMA: &str = "kiana.migration-rollback.v1";
pub const MIGRATION_ROLLBACK_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationRollbackKind {
    Binary,
    Data,
    EffectReconciliation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationRollbackAction {
    StartCompatibleOldRevision,
    RestoreVerifiedRoot,
    ReconcileExternalEffect,
    Deny,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationRollbackStatus {
    Allowed,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRollbackFacts {
    pub registry_digest: String,
    pub old_revision_digest: String,
    pub old_root_digest: String,
    pub current_revision_digest: String,
    pub verified_backup: bool,
    pub active_writer_count: u32,
    pub unknown_effect_count: u32,
    pub old_revision_compatible: bool,
    pub new_revision_fenced: bool,
    pub old_root_retained: bool,
    pub restore_verified: bool,
    pub external_effect_reconciled: bool,
}

impl MigrationRollbackFacts {
    pub fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (&self.registry_digest, "migration_rollback_registry_digest"),
            (
                &self.old_revision_digest,
                "migration_rollback_old_revision_digest",
            ),
            (&self.old_root_digest, "migration_rollback_old_root_digest"),
            (
                &self.current_revision_digest,
                "migration_rollback_current_revision_digest",
            ),
        ] {
            validate_digest(value, field)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::to_value(self).unwrap_or_default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRollbackReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_digest: String,
    pub kind: MigrationRollbackKind,
    pub status: MigrationRollbackStatus,
    pub action: MigrationRollbackAction,
    pub reason: String,
    pub remediation: String,
    pub old_root_retained: bool,
    pub fact_digest: String,
    pub receipt_digest: String,
}

impl MigrationRollbackReceipt {
    pub fn evaluate(
        registry: &MigrationRegistry,
        kind: MigrationRollbackKind,
        facts: &MigrationRollbackFacts,
    ) -> Result<Self, String> {
        registry.validate()?;
        facts.validate()?;
        if facts.registry_digest != registry.registry_digest {
            return Err("migration_rollback_registry_checksum_drift".to_owned());
        }
        let fact_digest = facts.digest();
        let decision = if !facts.old_root_retained {
            (
                MigrationRollbackStatus::Blocked,
                MigrationRollbackAction::Deny,
                "rollback_old_root_not_retained",
                "retain the old root before any destructive migration or rollback decision",
            )
        } else if !facts.verified_backup {
            (
                MigrationRollbackStatus::Blocked,
                MigrationRollbackAction::Deny,
                "rollback_backup_unverified",
                "create and verify a backup before rollback",
            )
        } else if facts.active_writer_count > 0 {
            (
                MigrationRollbackStatus::Blocked,
                MigrationRollbackAction::Deny,
                "rollback_writer_active",
                "fence and drain every writer before rollback",
            )
        } else if !facts.new_revision_fenced {
            (
                MigrationRollbackStatus::Blocked,
                MigrationRollbackAction::Deny,
                "rollback_new_revision_not_fenced",
                "fence the migrated revision before restoring or switching",
            )
        } else if facts.unknown_effect_count > 0 {
            (
                MigrationRollbackStatus::Blocked,
                MigrationRollbackAction::Deny,
                "rollback_unknown_effects",
                "reconcile every unknown external effect before rollback",
            )
        } else {
            match kind {
                MigrationRollbackKind::Binary if !facts.old_revision_compatible => (
                    MigrationRollbackStatus::Blocked,
                    MigrationRollbackAction::Deny,
                    "rollback_binary_incompatible",
                    "use a forward-compatible build or verified data restore",
                ),
                MigrationRollbackKind::Binary => (
                    MigrationRollbackStatus::Allowed,
                    MigrationRollbackAction::StartCompatibleOldRevision,
                    "rollback_binary_allowed",
                    "start the compatible old revision and re-run readiness gates",
                ),
                MigrationRollbackKind::Data if !facts.restore_verified => (
                    MigrationRollbackStatus::Blocked,
                    MigrationRollbackAction::Deny,
                    "rollback_restore_unverified",
                    "verify the restored root before activation",
                ),
                MigrationRollbackKind::Data => (
                    MigrationRollbackStatus::Allowed,
                    MigrationRollbackAction::RestoreVerifiedRoot,
                    "rollback_data_allowed",
                    "activate only the verified root with a new fence",
                ),
                MigrationRollbackKind::EffectReconciliation
                    if !facts.external_effect_reconciled =>
                {
                    (
                        MigrationRollbackStatus::Blocked,
                        MigrationRollbackAction::Deny,
                        "rollback_effect_unreconciled",
                        "query the external idempotency receipt before compensation",
                    )
                }
                MigrationRollbackKind::EffectReconciliation => (
                    MigrationRollbackStatus::Allowed,
                    MigrationRollbackAction::ReconcileExternalEffect,
                    "rollback_effect_reconciled",
                    "record reconcile, retry_without_effect, abandon or compensate explicitly",
                ),
            }
        };
        let mut receipt = Self {
            schema: MIGRATION_ROLLBACK_SCHEMA.to_owned(),
            version: MIGRATION_ROLLBACK_VERSION,
            registry_digest: facts.registry_digest.clone(),
            kind,
            status: decision.0,
            action: decision.1,
            reason: decision.2.to_owned(),
            remediation: decision.3.to_owned(),
            old_root_retained: facts.old_root_retained,
            fact_digest,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_ROLLBACK_SCHEMA
            || self.version != MIGRATION_ROLLBACK_VERSION
            || self.reason.trim().is_empty()
            || self.remediation.trim().is_empty()
            || self.status == MigrationRollbackStatus::Allowed
                && self.action == MigrationRollbackAction::Deny
        {
            return Err("migration_rollback_receipt_invalid".to_owned());
        }
        validate_digest(&self.registry_digest, "migration_rollback_registry_digest")?;
        validate_digest(&self.fact_digest, "migration_rollback_fact_digest")?;
        validate_digest(&self.receipt_digest, "migration_rollback_receipt_digest")?;
        if self.receipt_digest != self.digest() {
            return Err("migration_rollback_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "registry_digest": self.registry_digest,
            "kind": self.kind,
            "status": self.status,
            "action": self.action,
            "reason": self.reason,
            "remediation": self.remediation,
            "old_root_retained": self.old_root_retained,
            "fact_digest": self.fact_digest,
        }))
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
