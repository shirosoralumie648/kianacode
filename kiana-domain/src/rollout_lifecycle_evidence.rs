//! Rollout lifecycle verification and retirement evidence boundary.
//!
//! This manifest is a projection of lifecycle facts. It never pauses traffic, retires a worker,
//! deletes an old root or turns a simulated health window into a live deployment proof.

use crate::{
    json_digest, OrchestratedBackend, OrchestratedRolloutAction, RolloutLifecyclePhase,
    SchemaVersion,
};
use serde::{Deserialize, Serialize};

pub const ROLLOUT_LIFECYCLE_EVIDENCE_SCHEMA: &str = "kiana.rollout-lifecycle-evidence.v1";
pub const ROLLOUT_LIFECYCLE_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutLifecycleProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutLifecycleEvidenceStatus {
    Simulated,
    Target,
    Verified,
    Unknown,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RolloutLifecycleEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub backend: OrchestratedBackend,
    pub phase: RolloutLifecyclePhase,
    pub action: OrchestratedRolloutAction,
    pub state_digest: String,
    pub plan_digest: String,
    pub health_window_digest: Option<String>,
    pub verification_digest: Option<String>,
    pub retention_digest: String,
    pub drain_digest: String,
    pub old_root_retained: bool,
    pub deletion_eligible: bool,
    pub active_run_count: u32,
    pub active_writer_count: u32,
    pub proof_level: RolloutLifecycleProofLevel,
    pub status: RolloutLifecycleEvidenceStatus,
    pub operator_approval_ref: Option<String>,
    pub result_unknown: bool,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl RolloutLifecycleEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: OrchestratedBackend,
        phase: RolloutLifecyclePhase,
        action: OrchestratedRolloutAction,
        state_digest: impl Into<String>,
        plan_digest: impl Into<String>,
        health_window_digest: Option<String>,
        verification_digest: Option<String>,
        retention_digest: impl Into<String>,
        drain_digest: impl Into<String>,
        old_root_retained: bool,
        deletion_eligible: bool,
        active_run_count: u32,
        active_writer_count: u32,
        proof_level: RolloutLifecycleProofLevel,
        status: RolloutLifecycleEvidenceStatus,
        operator_approval_ref: Option<String>,
        result_unknown: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: ROLLOUT_LIFECYCLE_EVIDENCE_SCHEMA.to_owned(),
            version: ROLLOUT_LIFECYCLE_EVIDENCE_VERSION,
            backend,
            phase,
            action,
            state_digest: state_digest.into(),
            plan_digest: plan_digest.into(),
            health_window_digest,
            verification_digest,
            retention_digest: retention_digest.into(),
            drain_digest: drain_digest.into(),
            old_root_retained,
            deletion_eligible,
            active_run_count,
            active_writer_count,
            proof_level,
            status,
            operator_approval_ref,
            result_unknown,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROLLOUT_LIFECYCLE_EVIDENCE_SCHEMA
            || self.version != ROLLOUT_LIFECYCLE_EVIDENCE_VERSION
        {
            return Err("rollout_lifecycle_evidence_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.state_digest, "rollout_lifecycle_state_digest"),
            (&self.plan_digest, "rollout_lifecycle_plan_digest"),
            (&self.retention_digest, "rollout_lifecycle_retention_digest"),
            (&self.drain_digest, "rollout_lifecycle_drain_digest"),
        ] {
            digest(value, field)?;
        }
        for (value, field) in [
            (
                &self.health_window_digest,
                "rollout_lifecycle_health_window_digest",
            ),
            (
                &self.verification_digest,
                "rollout_lifecycle_verification_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        if let Some(value) = &self.operator_approval_ref {
            bounded(value, "rollout_lifecycle_operator_approval_ref", 256)?;
        }
        if self.deletion_eligible
            && (!self.old_root_retained
                || self.phase != RolloutLifecyclePhase::Retired
                || self.active_run_count > 0
                || self.active_writer_count > 0)
        {
            return Err("rollout_lifecycle_deletion_gate_invalid".to_owned());
        }
        if self.backend.is_target() && self.status == RolloutLifecycleEvidenceStatus::Verified {
            return Err("rollout_lifecycle_target_cannot_verify".to_owned());
        }
        if self.result_unknown && self.status == RolloutLifecycleEvidenceStatus::Verified {
            return Err("rollout_lifecycle_unknown_cannot_verify".to_owned());
        }
        if self.limitations.len() > MAX_LIMITATIONS {
            return Err("rollout_lifecycle_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "rollout_lifecycle_limitation", 256)?;
        }
        if self.status == RolloutLifecycleEvidenceStatus::Verified {
            let Some(approval_ref) = self.operator_approval_ref.as_deref() else {
                return Err("rollout_lifecycle_verified_evidence_incomplete".to_owned());
            };
            if approval_ref.trim() != approval_ref
                || !approval_ref.to_ascii_lowercase().starts_with("approval:")
            {
                return Err("rollout_lifecycle_operator_approval_ref_invalid".to_owned());
            }
            if self.backend == OrchestratedBackend::Simulation {
                return Err("rollout_lifecycle_simulation_cannot_verify".to_owned());
            }
            if self.proof_level == RolloutLifecycleProofLevel::Source
                || self.health_window_digest.is_none()
                || self.verification_digest.is_none()
                || self.active_run_count > 0
                || self.active_writer_count > 0
                || !self.old_root_retained
                || self.phase == RolloutLifecyclePhase::Planned
            {
                return Err("rollout_lifecycle_verified_evidence_incomplete".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("rollout_lifecycle_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("rollout_lifecycle_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "backend": self.backend,
            "phase": self.phase,
            "action": self.action,
            "state_digest": self.state_digest,
            "plan_digest": self.plan_digest,
            "health_window_digest": self.health_window_digest,
            "verification_digest": self.verification_digest,
            "retention_digest": self.retention_digest,
            "drain_digest": self.drain_digest,
            "old_root_retained": self.old_root_retained,
            "deletion_eligible": self.deletion_eligible,
            "active_run_count": self.active_run_count,
            "active_writer_count": self.active_writer_count,
            "proof_level": self.proof_level,
            "status": self.status,
            "operator_approval_ref": self.operator_approval_ref,
            "result_unknown": self.result_unknown,
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
