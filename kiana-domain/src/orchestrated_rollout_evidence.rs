//! Orchestrated rollout evidence and target-backend proof boundary.
//!
//! The manifest records decisions made from rollout facts. It never changes traffic, fences a
//! worker, calls an orchestrator or turns a simulation into a live deployment receipt.

use crate::{json_digest, OrchestratedBackend, OrchestratedRolloutAction, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const ORCHESTRATED_ROLLOUT_EVIDENCE_SCHEMA: &str = "kiana.orchestrated-rollout-evidence.v1";
pub const ORCHESTRATED_ROLLOUT_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratedRolloutProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratedRolloutEvidenceStatus {
    Simulated,
    Target,
    Verified,
    Unknown,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratedRolloutPhase {
    Planned,
    Paused,
    Promoted,
    RolledBack,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratedRolloutEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub rollout_id: String,
    pub backend: OrchestratedBackend,
    pub phase: OrchestratedRolloutPhase,
    pub action: OrchestratedRolloutAction,
    pub plan_digest: String,
    pub routing_digest: String,
    pub canary_digest: String,
    pub decision_digest: String,
    pub active_writer_revision_id: String,
    pub writer_fence_digest: String,
    pub progress_deadline_unix_ms: u64,
    pub observed_at_unix_ms: u64,
    pub proof_level: OrchestratedRolloutProofLevel,
    pub status: OrchestratedRolloutEvidenceStatus,
    pub operator_approval_ref: Option<String>,
    pub health_receipt_digest: Option<String>,
    pub traffic_drain_receipt_digest: Option<String>,
    pub result_unknown: bool,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl OrchestratedRolloutEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rollout_id: impl Into<String>,
        backend: OrchestratedBackend,
        phase: OrchestratedRolloutPhase,
        action: OrchestratedRolloutAction,
        plan_digest: impl Into<String>,
        routing_digest: impl Into<String>,
        canary_digest: impl Into<String>,
        decision_digest: impl Into<String>,
        active_writer_revision_id: impl Into<String>,
        writer_fence_digest: impl Into<String>,
        progress_deadline_unix_ms: u64,
        observed_at_unix_ms: u64,
        proof_level: OrchestratedRolloutProofLevel,
        status: OrchestratedRolloutEvidenceStatus,
        operator_approval_ref: Option<String>,
        health_receipt_digest: Option<String>,
        traffic_drain_receipt_digest: Option<String>,
        result_unknown: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: ORCHESTRATED_ROLLOUT_EVIDENCE_SCHEMA.to_owned(),
            version: ORCHESTRATED_ROLLOUT_EVIDENCE_VERSION,
            rollout_id: rollout_id.into(),
            backend,
            phase,
            action,
            plan_digest: plan_digest.into(),
            routing_digest: routing_digest.into(),
            canary_digest: canary_digest.into(),
            decision_digest: decision_digest.into(),
            active_writer_revision_id: active_writer_revision_id.into(),
            writer_fence_digest: writer_fence_digest.into(),
            progress_deadline_unix_ms,
            observed_at_unix_ms,
            proof_level,
            status,
            operator_approval_ref,
            health_receipt_digest,
            traffic_drain_receipt_digest,
            result_unknown,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ORCHESTRATED_ROLLOUT_EVIDENCE_SCHEMA
            || self.version != ORCHESTRATED_ROLLOUT_EVIDENCE_VERSION
        {
            return Err("orchestrated_rollout_evidence_header_invalid".to_owned());
        }
        bounded(&self.rollout_id, "orchestrated_rollout_id", 128)?;
        bounded(
            &self.active_writer_revision_id,
            "orchestrated_active_writer_revision",
            256,
        )?;
        for (value, field) in [
            (
                &self.health_receipt_digest,
                "orchestrated_health_receipt_digest",
            ),
            (
                &self.traffic_drain_receipt_digest,
                "orchestrated_traffic_drain_receipt_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        for (value, field) in [
            (&self.plan_digest, "orchestrated_plan_digest"),
            (&self.routing_digest, "orchestrated_routing_digest"),
            (&self.canary_digest, "orchestrated_canary_digest"),
            (&self.decision_digest, "orchestrated_decision_digest"),
            (
                &self.writer_fence_digest,
                "orchestrated_writer_fence_digest",
            ),
        ] {
            digest(value, field)?;
        }
        if self.progress_deadline_unix_ms == 0
            || self.observed_at_unix_ms == 0
            || self.observed_at_unix_ms > self.progress_deadline_unix_ms
        {
            return Err("orchestrated_rollout_deadline_invalid".to_owned());
        }
        if let Some(value) = &self.operator_approval_ref {
            bounded(value, "orchestrated_operator_approval_ref", 256)?;
        }
        if self.limitations.len() > MAX_LIMITATIONS {
            return Err("orchestrated_rollout_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "orchestrated_rollout_limitation", 256)?;
        }
        if self.backend.is_target() && self.status == OrchestratedRolloutEvidenceStatus::Verified {
            return Err("orchestrated_target_backend_cannot_verify".to_owned());
        }
        if self.result_unknown && self.status == OrchestratedRolloutEvidenceStatus::Verified {
            return Err("orchestrated_unknown_cannot_verify".to_owned());
        }
        if self.status == OrchestratedRolloutEvidenceStatus::Verified {
            if self.proof_level != OrchestratedRolloutProofLevel::Live
                || self.operator_approval_ref.is_none()
                || self.health_receipt_digest.is_none()
                || self.traffic_drain_receipt_digest.is_none()
                || self.phase != OrchestratedRolloutPhase::Promoted
            {
                return Err("orchestrated_verified_evidence_incomplete".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("orchestrated_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("orchestrated_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "rollout_id": self.rollout_id,
            "backend": self.backend,
            "phase": self.phase,
            "action": self.action,
            "plan_digest": self.plan_digest,
            "routing_digest": self.routing_digest,
            "canary_digest": self.canary_digest,
            "decision_digest": self.decision_digest,
            "active_writer_revision_id": self.active_writer_revision_id,
            "writer_fence_digest": self.writer_fence_digest,
            "progress_deadline_unix_ms": self.progress_deadline_unix_ms,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "proof_level": self.proof_level,
            "status": self.status,
            "operator_approval_ref": self.operator_approval_ref,
            "health_receipt_digest": self.health_receipt_digest,
            "traffic_drain_receipt_digest": self.traffic_drain_receipt_digest,
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
