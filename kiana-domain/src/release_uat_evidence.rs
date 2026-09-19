//! Cross-entrypoint UAT evidence handoff.
//!
//! The bundle binds a ReleaseUatMatrix to its source/CI observation and proof ceiling. It does
//! not execute release operations or turn fake matrix rows into durable/live deployment proof.

use crate::{json_digest, SchemaVersion, UatProviderMode};
use serde::{Deserialize, Serialize};

pub const RELEASE_UAT_EVIDENCE_SCHEMA: &str = "kiana.release-uat-evidence.v1";
pub const RELEASE_UAT_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_RECEIPTS: usize = 512;
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseUatProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
    Physical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseUatEvidenceStatus {
    Fixture,
    Verified,
    Unknown,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseUatEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub matrix_digest: String,
    pub source_snapshot_digest: String,
    pub ci_run_ref: String,
    pub provider_mode: UatProviderMode,
    pub proof_level: ReleaseUatProofLevel,
    pub status: ReleaseUatEvidenceStatus,
    pub operator_approval_ref: Option<String>,
    pub receipt_digests: Vec<String>,
    pub unknown_reconciled: bool,
    pub reviewer: String,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl ReleaseUatEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        matrix_digest: impl Into<String>,
        source_snapshot_digest: impl Into<String>,
        ci_run_ref: impl Into<String>,
        provider_mode: UatProviderMode,
        proof_level: ReleaseUatProofLevel,
        status: ReleaseUatEvidenceStatus,
        operator_approval_ref: Option<String>,
        mut receipt_digests: Vec<String>,
        unknown_reconciled: bool,
        reviewer: impl Into<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        receipt_digests.sort();
        receipt_digests.dedup();
        let mut evidence = Self {
            schema: RELEASE_UAT_EVIDENCE_SCHEMA.to_owned(),
            version: RELEASE_UAT_EVIDENCE_VERSION,
            matrix_digest: matrix_digest.into(),
            source_snapshot_digest: source_snapshot_digest.into(),
            ci_run_ref: ci_run_ref.into(),
            provider_mode,
            proof_level,
            status,
            operator_approval_ref,
            receipt_digests,
            unknown_reconciled,
            reviewer: reviewer.into(),
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RELEASE_UAT_EVIDENCE_SCHEMA
            || self.version != RELEASE_UAT_EVIDENCE_VERSION
        {
            return Err("release_uat_evidence_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.ci_run_ref, "release_uat_ci_run_ref", 256),
            (&self.reviewer, "release_uat_reviewer", 256),
        ] {
            bounded(value, field, max)?;
        }
        digest(&self.matrix_digest, "release_uat_matrix_digest")?;
        digest(
            &self.source_snapshot_digest,
            "release_uat_source_snapshot_digest",
        )?;
        if self.receipt_digests.len() > MAX_RECEIPTS {
            return Err("release_uat_receipt_limit".to_owned());
        }
        for receipt in &self.receipt_digests {
            digest(receipt, "release_uat_receipt_digest")?;
        }
        if let Some(value) = &self.operator_approval_ref {
            bounded(value, "release_uat_operator_approval_ref", 256)?;
        }
        if self.limitations.len() > MAX_LIMITATIONS {
            return Err("release_uat_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "release_uat_limitation", 256)?;
        }
        if self.provider_mode == UatProviderMode::Fake
            && (self.proof_level == ReleaseUatProofLevel::Live
                || self.proof_level == ReleaseUatProofLevel::Physical)
        {
            return Err("release_uat_fake_cannot_claim_live".to_owned());
        }
        if self.provider_mode == UatProviderMode::LiveOptIn && self.operator_approval_ref.is_none()
        {
            return Err("release_uat_live_approval_missing".to_owned());
        }
        if self.status == ReleaseUatEvidenceStatus::Verified {
            if self.proof_level == ReleaseUatProofLevel::Source
                || self.receipt_digests.is_empty()
                || !self.unknown_reconciled
            {
                return Err("release_uat_verified_evidence_incomplete".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("release_uat_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("release_uat_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "matrix_digest": self.matrix_digest,
            "source_snapshot_digest": self.source_snapshot_digest,
            "ci_run_ref": self.ci_run_ref,
            "provider_mode": self.provider_mode,
            "proof_level": self.proof_level,
            "status": self.status,
            "operator_approval_ref": self.operator_approval_ref,
            "receipt_digests": self.receipt_digests,
            "unknown_reconciled": self.unknown_reconciled,
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
