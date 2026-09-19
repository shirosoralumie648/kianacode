//! Persistence UAT evidence handoff.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const PERSISTENCE_UAT_EVIDENCE_SCHEMA: &str = "kiana.persistence-uat-evidence.v1";
pub const PERSISTENCE_UAT_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_RECEIPTS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceUatProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceUatEvidenceStatus {
    Fixture,
    Verified,
    Unknown,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceUatEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub matrix_digest: String,
    pub source_snapshot_digest: String,
    pub proof_level: PersistenceUatProofLevel,
    pub status: PersistenceUatEvidenceStatus,
    pub receipt_digests: Vec<String>,
    pub backup_restore_reconciled: bool,
    pub restart_replayed: bool,
    pub deletion_reviewed: bool,
    pub reviewer: String,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl PersistenceUatEvidence {
    pub fn new(
        matrix_digest: impl Into<String>,
        source_snapshot_digest: impl Into<String>,
        proof_level: PersistenceUatProofLevel,
        status: PersistenceUatEvidenceStatus,
        mut receipt_digests: Vec<String>,
        backup_restore_reconciled: bool,
        restart_replayed: bool,
        deletion_reviewed: bool,
        reviewer: impl Into<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        receipt_digests.sort();
        receipt_digests.dedup();
        let mut evidence = Self {
            schema: PERSISTENCE_UAT_EVIDENCE_SCHEMA.to_owned(),
            version: PERSISTENCE_UAT_EVIDENCE_VERSION,
            matrix_digest: matrix_digest.into(),
            source_snapshot_digest: source_snapshot_digest.into(),
            proof_level,
            status,
            receipt_digests,
            backup_restore_reconciled,
            restart_replayed,
            deletion_reviewed,
            reviewer: reviewer.into(),
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PERSISTENCE_UAT_EVIDENCE_SCHEMA
            || self.version != PERSISTENCE_UAT_EVIDENCE_VERSION
        {
            return Err("persistence_uat_evidence_header_invalid".to_owned());
        }
        digest(
            &self.matrix_digest,
            "persistence_uat_evidence_matrix_digest",
        )?;
        digest(
            &self.source_snapshot_digest,
            "persistence_uat_evidence_source_snapshot_digest",
        )?;
        bounded(&self.reviewer, "persistence_uat_evidence_reviewer", 256)?;
        if self.receipt_digests.len() > MAX_RECEIPTS {
            return Err("persistence_uat_evidence_receipt_limit".to_owned());
        }
        for receipt in &self.receipt_digests {
            digest(receipt, "persistence_uat_evidence_receipt_digest")?;
        }
        if self.limitations.len() > 16 {
            return Err("persistence_uat_evidence_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "persistence_uat_evidence_limitation", 256)?;
        }
        if self.status == PersistenceUatEvidenceStatus::Verified {
            if self.proof_level == PersistenceUatProofLevel::Source
                || self.receipt_digests.is_empty()
                || !self.backup_restore_reconciled
                || !self.restart_replayed
                || !self.deletion_reviewed
            {
                return Err("persistence_uat_verified_evidence_incomplete".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("persistence_uat_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("persistence_uat_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "matrix_digest": self.matrix_digest,
            "source_snapshot_digest": self.source_snapshot_digest,
            "proof_level": self.proof_level,
            "status": self.status,
            "receipt_digests": self.receipt_digests,
            "backup_restore_reconciled": self.backup_restore_reconciled,
            "restart_replayed": self.restart_replayed,
            "deletion_reviewed": self.deletion_reviewed,
            "reviewer": self.reviewer,
            "limitations": self.limitations,
        }))
    }
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
