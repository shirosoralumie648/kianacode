//! ER-34 durable-gate evidence ceiling.
//!
//! A gate record distinguishes a CI-only source check from a durable observation. It is
//! fail-closed: no command history, moving worktree or unobserved CI run can claim durable proof.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const ER34_DURABLE_GATE_SCHEMA: &str = "kiana.er34-durable-gate.v1";

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er34GateOrigin {
    GitHubActions,
    LocalWorktree,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er34GateResult {
    Pending,
    Blocked,
    Passed,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er34ProofLevel {
    Source,
    Durable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Er34DurableGateEvidence {
    pub schema: String,
    pub snapshot_digest: String,
    pub command_digest: String,
    pub origin: Er34GateOrigin,
    pub result: Er34GateResult,
    pub proof_level: Er34ProofLevel,
    pub ci_focused_tests_executed: bool,
    pub local_tests_executed: bool,
    pub durable_facts_observed: bool,
    pub cache_deleted_and_rebuilt: bool,
    pub event_frame_hash: Option<String>,
    pub artifact_hash: Option<String>,
    pub process_fact_digest: Option<String>,
    pub limitations: Vec<String>,
    pub digest: String,
}

impl Er34DurableGateEvidence {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ER34_DURABLE_GATE_SCHEMA || self.limitations.len() > 32 {
            return Err("er34_gate_header_invalid".to_owned());
        }
        digest(&self.snapshot_digest, "er34_snapshot_digest_invalid")?;
        digest(&self.command_digest, "er34_command_digest_invalid")?;
        for value in [
            &self.event_frame_hash,
            &self.artifact_hash,
            &self.process_fact_digest,
        ]
        .into_iter()
        .flatten()
        {
            digest(value, "er34_observation_digest_invalid")?;
        }
        for limitation in &self.limitations {
            required(limitation, "er34_limitation_invalid")?;
        }
        if self.local_tests_executed && self.origin != Er34GateOrigin::LocalWorktree {
            return Err("er34_local_execution_origin_mismatch".to_owned());
        }
        if self.proof_level == Er34ProofLevel::Durable
            && (!self.ci_focused_tests_executed
                || !self.durable_facts_observed
                || !self.cache_deleted_and_rebuilt
                || self.result != Er34GateResult::Passed
                || self.event_frame_hash.is_none()
                || self.artifact_hash.is_none()
                || self.process_fact_digest.is_none())
        {
            return Err("er34_durable_proof_evidence_incomplete".to_owned());
        }
        if self.result == Er34GateResult::Passed && self.proof_level == Er34ProofLevel::Source {
            return Err("er34_source_pass_cannot_claim_gate".to_owned());
        }
        if self.result != Er34GateResult::Passed && self.proof_level == Er34ProofLevel::Durable {
            return Err("er34_nonpass_cannot_claim_durable".to_owned());
        }
        digest(&self.digest, "er34_gate_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("er34_gate_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "snapshot_digest": self.snapshot_digest,
            "command_digest": self.command_digest,
            "origin": self.origin,
            "result": self.result,
            "proof_level": self.proof_level,
            "ci_focused_tests_executed": self.ci_focused_tests_executed,
            "local_tests_executed": self.local_tests_executed,
            "durable_facts_observed": self.durable_facts_observed,
            "cache_deleted_and_rebuilt": self.cache_deleted_and_rebuilt,
            "event_frame_hash": self.event_frame_hash,
            "artifact_hash": self.artifact_hash,
            "process_fact_digest": self.process_fact_digest,
            "limitations": self.limitations,
        }))
    }
}
