//! Context/memory golden-path evidence contract.
//!
//! This is a bounded manifest for a fake cassette or an explicitly opted-in live observation. It
//! does not execute a provider, write Memory facts or promote a projection to durable truth.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const CONTEXT_MEMORY_EVIDENCE_SCHEMA: &str = "kiana.context-memory-evidence.v1";
pub const CONTEXT_MEMORY_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMemoryProviderMode {
    FakeCassette,
    LiveOptIn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMemoryProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMemoryEvidenceStatus {
    Verified,
    Partial,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMemoryStageDigests {
    pub context_plan_digest: Option<String>,
    pub provider_request_digest: Option<String>,
    pub tool_receipt_digest: Option<String>,
    pub retrieval_digest: Option<String>,
    pub candidate_digest: Option<String>,
    pub approval_receipt_digest: Option<String>,
    pub projection_digest: Option<String>,
    pub recovery_digest: Option<String>,
    pub run_receipt_digest: Option<String>,
}

impl ContextMemoryStageDigests {
    fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (
                &self.context_plan_digest,
                "context_memory_context_plan_digest",
            ),
            (
                &self.provider_request_digest,
                "context_memory_provider_request_digest",
            ),
            (
                &self.tool_receipt_digest,
                "context_memory_tool_receipt_digest",
            ),
            (&self.retrieval_digest, "context_memory_retrieval_digest"),
            (&self.candidate_digest, "context_memory_candidate_digest"),
            (
                &self.approval_receipt_digest,
                "context_memory_approval_receipt_digest",
            ),
            (&self.projection_digest, "context_memory_projection_digest"),
            (&self.recovery_digest, "context_memory_recovery_digest"),
            (
                &self.run_receipt_digest,
                "context_memory_run_receipt_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        Ok(())
    }

    fn complete(&self) -> bool {
        [
            &self.context_plan_digest,
            &self.provider_request_digest,
            &self.tool_receipt_digest,
            &self.retrieval_digest,
            &self.candidate_digest,
            &self.approval_receipt_digest,
            &self.projection_digest,
            &self.recovery_digest,
            &self.run_receipt_digest,
        ]
        .into_iter()
        .all(Option::is_some)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMemoryGoldenPathEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub provider_mode: ContextMemoryProviderMode,
    pub proof_level: ContextMemoryProofLevel,
    pub scope_digest: String,
    pub redaction_profile_digest: String,
    pub stages: ContextMemoryStageDigests,
    pub provider_live_evidence_digest: Option<String>,
    pub operator_approval_ref: Option<String>,
    pub status: ContextMemoryEvidenceStatus,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl ContextMemoryGoldenPathEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_mode: ContextMemoryProviderMode,
        proof_level: ContextMemoryProofLevel,
        scope_digest: impl Into<String>,
        redaction_profile_digest: impl Into<String>,
        stages: ContextMemoryStageDigests,
        provider_live_evidence_digest: Option<String>,
        operator_approval_ref: Option<String>,
        status: ContextMemoryEvidenceStatus,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: CONTEXT_MEMORY_EVIDENCE_SCHEMA.to_owned(),
            version: CONTEXT_MEMORY_EVIDENCE_VERSION,
            provider_mode,
            proof_level,
            scope_digest: scope_digest.into(),
            redaction_profile_digest: redaction_profile_digest.into(),
            stages,
            provider_live_evidence_digest,
            operator_approval_ref,
            status,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_MEMORY_EVIDENCE_SCHEMA
            || self.version != CONTEXT_MEMORY_EVIDENCE_VERSION
        {
            return Err("context_memory_evidence_header_invalid".to_owned());
        }
        digest(&self.scope_digest, "context_memory_scope_digest")?;
        digest(
            &self.redaction_profile_digest,
            "context_memory_redaction_profile_digest",
        )?;
        self.stages.validate()?;
        if let Some(value) = &self.provider_live_evidence_digest {
            digest(value, "context_memory_provider_live_evidence_digest")?;
        }
        if let Some(value) = &self.operator_approval_ref {
            bounded(value, "context_memory_operator_approval_ref", 256)?;
        }
        if self.limitations.len() > MAX_LIMITATIONS {
            return Err("context_memory_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "context_memory_limitation", 256)?;
        }

        match self.provider_mode {
            ContextMemoryProviderMode::FakeCassette => {
                if self.provider_live_evidence_digest.is_some()
                    || self.operator_approval_ref.is_some()
                    || self.proof_level == ContextMemoryProofLevel::Live
                {
                    return Err("context_memory_fake_cannot_claim_live".to_owned());
                }
            }
            ContextMemoryProviderMode::LiveOptIn => {
                if self.provider_live_evidence_digest.is_none()
                    || self.operator_approval_ref.is_none()
                {
                    return Err("context_memory_live_opt_in_evidence_missing".to_owned());
                }
            }
        }
        if self.proof_level == ContextMemoryProofLevel::Durable
            && (self.stages.recovery_digest.is_none() || self.stages.run_receipt_digest.is_none())
        {
            return Err("context_memory_durable_evidence_missing".to_owned());
        }
        if self.proof_level == ContextMemoryProofLevel::Live
            && (self.provider_mode != ContextMemoryProviderMode::LiveOptIn
                || self.status != ContextMemoryEvidenceStatus::Verified)
        {
            return Err("context_memory_live_proof_requires_verified_opt_in".to_owned());
        }
        if self.status == ContextMemoryEvidenceStatus::Verified {
            if !self.stages.complete() {
                return Err("context_memory_verified_stages_missing".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("context_memory_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("context_memory_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "provider_mode": self.provider_mode,
            "proof_level": self.proof_level,
            "scope_digest": self.scope_digest,
            "redaction_profile_digest": self.redaction_profile_digest,
            "stages": self.stages,
            "provider_live_evidence_digest": self.provider_live_evidence_digest,
            "operator_approval_ref": self.operator_approval_ref,
            "status": self.status,
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
