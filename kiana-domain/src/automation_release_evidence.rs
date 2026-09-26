//! AUT-24 durable/live release evidence manifest.
//!
//! The manifest is a fail-closed handoff index. It cannot upgrade source evidence to durable/live
//! without explicit artifacts, approval and limitations.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const AUT24_RELEASE_GATE_SCHEMA: &str = "kiana.aut24-release-gate.v1";

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}
fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aut24FeatureStatus {
    Implemented,
    Partial,
    Deferred,
    NotSupported,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aut24ProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
    Physical,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aut24EvidenceRow {
    pub step: String,
    pub feature_status: Aut24FeatureStatus,
    pub proof_level: Aut24ProofLevel,
    pub evidence_ref: String,
    pub next_gate: String,
    pub limitations: Vec<String>,
    pub digest: String,
}

impl Aut24EvidenceRow {
    fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (&self.step, "aut24_step_required"),
            (&self.evidence_ref, "aut24_evidence_ref_required"),
            (&self.next_gate, "aut24_next_gate_required"),
        ] {
            required(value, field)?;
        }
        if self.limitations.is_empty() {
            return Err("aut24_limitations_required".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "aut24_limitation_invalid")?;
        }
        if self.proof_level >= Aut24ProofLevel::Live
            && self.feature_status != Aut24FeatureStatus::Implemented
        {
            return Err("aut24_live_proof_requires_implemented_status".to_owned());
        }
        if self.feature_status == Aut24FeatureStatus::Implemented && self.evidence_ref == "none" {
            return Err("aut24_implemented_evidence_missing".to_owned());
        }
        digest(&self.digest, "aut24_row_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut24_row_digest_mismatch".to_owned());
        }
        Ok(())
    }
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "step": self.step,
            "feature_status": self.feature_status,
            "proof_level": self.proof_level,
            "evidence_ref": self.evidence_ref,
            "next_gate": self.next_gate,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aut24ReleaseGate {
    pub schema: String,
    pub snapshot_digest: String,
    pub workflow_ref: String,
    pub rows: Vec<Aut24EvidenceRow>,
    pub blanket_completion: bool,
    pub digest: String,
}

impl Aut24ReleaseGate {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUT24_RELEASE_GATE_SCHEMA
            || self.rows.is_empty()
            || self.rows.len() > 256
            || self.blanket_completion
        {
            return Err("aut24_gate_header_or_blanket_completion_invalid".to_owned());
        }
        digest(&self.snapshot_digest, "aut24_snapshot_digest_invalid")?;
        required(&self.workflow_ref, "aut24_workflow_ref_required")?;
        let mut steps = BTreeSet::new();
        for row in &self.rows {
            row.validate()?;
            if !steps.insert(row.step.clone()) {
                return Err("aut24_step_duplicate".to_owned());
            }
        }
        digest(&self.digest, "aut24_gate_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut24_gate_digest_mismatch".to_owned());
        }
        Ok(())
    }
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "snapshot_digest": self.snapshot_digest,
            "workflow_ref": self.workflow_ref,
            "rows": self.rows,
            "blanket_completion": self.blanket_completion,
        }))
    }
}
