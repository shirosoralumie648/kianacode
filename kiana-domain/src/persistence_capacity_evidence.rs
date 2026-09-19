//! Persistence capacity evidence handoff.

use crate::{json_digest, PersistenceCapacityStatus, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const PERSISTENCE_CAPACITY_EVIDENCE_SCHEMA: &str = "kiana.persistence-capacity-evidence.v1";
pub const PERSISTENCE_CAPACITY_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceCapacityProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Physical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceCapacityEvidenceStatus {
    Fixture,
    Verified,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceCapacityEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub report_digest: String,
    pub baseline_digest: String,
    pub report_status: PersistenceCapacityStatus,
    pub proof_level: PersistenceCapacityProofLevel,
    pub status: PersistenceCapacityEvidenceStatus,
    pub benchmark_receipt_digest: Option<String>,
    pub resource_receipt_digest: Option<String>,
    pub representative_workload: bool,
    pub facts_preserved: bool,
    pub degradation_acknowledged: bool,
    pub reviewer: String,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl PersistenceCapacityEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        report_digest: impl Into<String>,
        baseline_digest: impl Into<String>,
        report_status: PersistenceCapacityStatus,
        proof_level: PersistenceCapacityProofLevel,
        status: PersistenceCapacityEvidenceStatus,
        benchmark_receipt_digest: Option<String>,
        resource_receipt_digest: Option<String>,
        representative_workload: bool,
        facts_preserved: bool,
        degradation_acknowledged: bool,
        reviewer: impl Into<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: PERSISTENCE_CAPACITY_EVIDENCE_SCHEMA.to_owned(),
            version: PERSISTENCE_CAPACITY_EVIDENCE_VERSION,
            report_digest: report_digest.into(),
            baseline_digest: baseline_digest.into(),
            report_status,
            proof_level,
            status,
            benchmark_receipt_digest,
            resource_receipt_digest,
            representative_workload,
            facts_preserved,
            degradation_acknowledged,
            reviewer: reviewer.into(),
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PERSISTENCE_CAPACITY_EVIDENCE_SCHEMA
            || self.version != PERSISTENCE_CAPACITY_EVIDENCE_VERSION
        {
            return Err("persistence_capacity_evidence_header_invalid".to_owned());
        }
        digest(&self.report_digest, "persistence_capacity_report_digest")?;
        digest(
            &self.baseline_digest,
            "persistence_capacity_baseline_digest",
        )?;
        for (value, field) in [
            (
                &self.benchmark_receipt_digest,
                "persistence_capacity_benchmark_receipt_digest",
            ),
            (
                &self.resource_receipt_digest,
                "persistence_capacity_resource_receipt_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        bounded(&self.reviewer, "persistence_capacity_reviewer", 256)?;
        for limitation in &self.limitations {
            bounded(limitation, "persistence_capacity_limitation", 256)?;
        }
        if self.status == PersistenceCapacityEvidenceStatus::Verified {
            if self.report_status != PersistenceCapacityStatus::Ready
                || self.proof_level == PersistenceCapacityProofLevel::Source
                || self.benchmark_receipt_digest.is_none()
                || self.resource_receipt_digest.is_none()
                || !self.representative_workload
                || !self.facts_preserved
                || !self.degradation_acknowledged
            {
                return Err("persistence_capacity_verified_evidence_incomplete".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("persistence_capacity_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("persistence_capacity_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "report_digest": self.report_digest,
            "baseline_digest": self.baseline_digest,
            "report_status": self.report_status,
            "proof_level": self.proof_level,
            "status": self.status,
            "benchmark_receipt_digest": self.benchmark_receipt_digest,
            "resource_receipt_digest": self.resource_receipt_digest,
            "representative_workload": self.representative_workload,
            "facts_preserved": self.facts_preserved,
            "degradation_acknowledged": self.degradation_acknowledged,
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
