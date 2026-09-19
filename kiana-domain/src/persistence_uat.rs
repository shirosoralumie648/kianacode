//! Persistence/data-layer end-to-end UAT evidence contract.
//!
//! The matrix describes the backup, restore, upgrade, restart and governance-delete boundaries
//! across the CLI, Web and Workbench entrypoints. It is deliberately a supplied-evidence
//! contract: it cannot write a backup, restore a root, run a migration or delete governed data.

use crate::{json_digest, SchemaVersion, UatEntrypoint, UatOutcome};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PERSISTENCE_UAT_SCHEMA: &str = "kiana.persistence-uat-matrix.v1";
pub const PERSISTENCE_UAT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_PERSISTENCE_UAT_CASES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceUatStage {
    Backup,
    Restore,
    Upgrade,
    Restart,
    GovernanceDelete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceUatCase {
    pub entrypoint: UatEntrypoint,
    pub stage: PersistenceUatStage,
    pub outcome: UatOutcome,
    pub storage_root_digest: String,
    pub event_store_digest: String,
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub receipt_digest: Option<String>,
    pub backup_manifest_verified: bool,
    pub quarantine_verified: bool,
    pub auth_re_admitted: bool,
    pub projection_receipt_parity: bool,
    pub old_root_retained: bool,
    pub migration_verified: bool,
    pub journal_replayed: bool,
    pub legal_hold: bool,
    pub deletion_authorized: bool,
    pub reconcile_required: bool,
    pub retry_permitted: bool,
    pub case_digest: String,
}

impl PersistenceUatCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        entrypoint: UatEntrypoint,
        stage: PersistenceUatStage,
        outcome: UatOutcome,
        storage_root_digest: impl Into<String>,
        event_store_digest: impl Into<String>,
        source_cursor: u64,
        projection_cursor: u64,
        receipt_digest: Option<String>,
        backup_manifest_verified: bool,
        quarantine_verified: bool,
        auth_re_admitted: bool,
        projection_receipt_parity: bool,
        old_root_retained: bool,
        migration_verified: bool,
        journal_replayed: bool,
        legal_hold: bool,
        deletion_authorized: bool,
        reconcile_required: bool,
        retry_permitted: bool,
    ) -> Result<Self, String> {
        let mut case = Self {
            entrypoint,
            stage,
            outcome,
            storage_root_digest: storage_root_digest.into(),
            event_store_digest: event_store_digest.into(),
            source_cursor,
            projection_cursor,
            receipt_digest,
            backup_manifest_verified,
            quarantine_verified,
            auth_re_admitted,
            projection_receipt_parity,
            old_root_retained,
            migration_verified,
            journal_replayed,
            legal_hold,
            deletion_authorized,
            reconcile_required,
            retry_permitted,
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.entrypoint == UatEntrypoint::Desktop {
            return Err("persistence_uat_desktop_not_in_scope".to_owned());
        }
        validate_digest(
            &self.storage_root_digest,
            "persistence_uat_storage_root_digest",
        )?;
        validate_digest(
            &self.event_store_digest,
            "persistence_uat_event_store_digest",
        )?;
        validate_digest(&self.case_digest, "persistence_uat_case_digest")?;
        if self.source_cursor == 0 || self.projection_cursor > self.source_cursor {
            return Err("persistence_uat_cursor_invalid".to_owned());
        }
        if let Some(receipt) = &self.receipt_digest {
            validate_digest(receipt, "persistence_uat_receipt_digest")?;
        }
        match self.outcome {
            UatOutcome::Denied => {
                if self.receipt_digest.is_some() || self.reconcile_required || self.retry_permitted
                {
                    return Err("persistence_uat_denied_evidence_invalid".to_owned());
                }
            }
            UatOutcome::ResultUnknown => {
                if self.receipt_digest.is_some() || !self.reconcile_required || self.retry_permitted
                {
                    return Err("persistence_uat_unknown_retry_forbidden".to_owned());
                }
            }
            UatOutcome::Succeeded | UatOutcome::RestartRecovered | UatOutcome::Replayed => {
                if self.receipt_digest.is_none() || self.reconcile_required {
                    return Err("persistence_uat_receipt_missing".to_owned());
                }
                match self.stage {
                    PersistenceUatStage::Backup
                        if !self.backup_manifest_verified
                            || !self.auth_re_admitted
                            || self.projection_cursor != self.source_cursor =>
                    {
                        return Err("persistence_uat_backup_gate_failed".to_owned())
                    }
                    PersistenceUatStage::Restore
                        if !self.quarantine_verified
                            || !self.auth_re_admitted
                            || !self.projection_receipt_parity
                            || !self.old_root_retained
                            || self.projection_cursor != self.source_cursor =>
                    {
                        return Err("persistence_uat_restore_gate_failed".to_owned())
                    }
                    PersistenceUatStage::Upgrade
                        if !self.migration_verified || !self.auth_re_admitted =>
                    {
                        return Err("persistence_uat_upgrade_gate_failed".to_owned())
                    }
                    PersistenceUatStage::Restart
                        if !self.journal_replayed
                            || !self.projection_receipt_parity
                            || !self.auth_re_admitted =>
                    {
                        return Err("persistence_uat_restart_gate_failed".to_owned())
                    }
                    PersistenceUatStage::GovernanceDelete
                        if self.legal_hold
                            || !self.deletion_authorized
                            || !self.auth_re_admitted =>
                    {
                        return Err("persistence_uat_delete_gate_failed".to_owned())
                    }
                    _ => {}
                }
            }
        }
        if self.case_digest != self.digest() {
            return Err("persistence_uat_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "entrypoint": self.entrypoint,
            "stage": self.stage,
            "outcome": self.outcome,
            "storage_root_digest": self.storage_root_digest,
            "event_store_digest": self.event_store_digest,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "receipt_digest": self.receipt_digest,
            "backup_manifest_verified": self.backup_manifest_verified,
            "quarantine_verified": self.quarantine_verified,
            "auth_re_admitted": self.auth_re_admitted,
            "projection_receipt_parity": self.projection_receipt_parity,
            "old_root_retained": self.old_root_retained,
            "migration_verified": self.migration_verified,
            "journal_replayed": self.journal_replayed,
            "legal_hold": self.legal_hold,
            "deletion_authorized": self.deletion_authorized,
            "reconcile_required": self.reconcile_required,
            "retry_permitted": self.retry_permitted,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceUatMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub storage_root_digest: String,
    pub event_store_digest: String,
    pub daemon_spine_digest: String,
    pub cases: Vec<PersistenceUatCase>,
    pub matrix_digest: String,
}

impl PersistenceUatMatrix {
    pub fn new(
        storage_root_digest: impl Into<String>,
        event_store_digest: impl Into<String>,
        daemon_spine_digest: impl Into<String>,
        cases: Vec<PersistenceUatCase>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: PERSISTENCE_UAT_SCHEMA.to_owned(),
            version: PERSISTENCE_UAT_VERSION,
            storage_root_digest: storage_root_digest.into(),
            event_store_digest: event_store_digest.into(),
            daemon_spine_digest: daemon_spine_digest.into(),
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PERSISTENCE_UAT_SCHEMA || self.version != PERSISTENCE_UAT_VERSION {
            return Err("persistence_uat_matrix_header_invalid".to_owned());
        }
        for (value, field) in [
            (
                &self.storage_root_digest,
                "persistence_uat_matrix_root_digest",
            ),
            (
                &self.event_store_digest,
                "persistence_uat_matrix_store_digest",
            ),
            (
                &self.daemon_spine_digest,
                "persistence_uat_matrix_spine_digest",
            ),
            (&self.matrix_digest, "persistence_uat_matrix_digest"),
        ] {
            validate_digest(value, field)?;
        }
        if self.cases.is_empty() || self.cases.len() > MAX_PERSISTENCE_UAT_CASES {
            return Err("persistence_uat_case_count_invalid".to_owned());
        }
        let mut keys = BTreeSet::new();
        let mut has_replayed = false;
        let mut has_unknown = false;
        for case in &self.cases {
            case.validate()?;
            if case.storage_root_digest != self.storage_root_digest
                || case.event_store_digest != self.event_store_digest
            {
                return Err("persistence_uat_storage_identity_drift".to_owned());
            }
            if !keys.insert((case.entrypoint, case.stage, case.outcome)) {
                return Err("persistence_uat_case_duplicate".to_owned());
            }
            has_replayed |= case.outcome == UatOutcome::Replayed;
            has_unknown |= case.outcome == UatOutcome::ResultUnknown;
        }
        for entrypoint in [
            UatEntrypoint::Cli,
            UatEntrypoint::Web,
            UatEntrypoint::Workbench,
        ] {
            for stage in [
                PersistenceUatStage::Backup,
                PersistenceUatStage::Restore,
                PersistenceUatStage::Upgrade,
                PersistenceUatStage::Restart,
                PersistenceUatStage::GovernanceDelete,
            ] {
                if !keys.contains(&(entrypoint, stage, UatOutcome::Denied)) {
                    return Err("persistence_uat_denied_coverage_missing".to_owned());
                }
                if !keys.contains(&(entrypoint, stage, UatOutcome::Succeeded)) {
                    return Err("persistence_uat_success_coverage_missing".to_owned());
                }
            }
        }
        if !has_replayed {
            return Err("persistence_uat_replay_coverage_missing".to_owned());
        }
        if !has_unknown {
            return Err("persistence_uat_unknown_coverage_missing".to_owned());
        }
        if self.matrix_digest != self.digest() {
            return Err("persistence_uat_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "storage_root_digest": self.storage_root_digest,
            "event_store_digest": self.event_store_digest,
            "daemon_spine_digest": self.daemon_spine_digest,
            "cases": self.cases,
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
