//! EQ-40 single-dimension QualityCandidate and version-snapshot binding.
//!
//! A candidate is a value contract for later gate evaluation. It cannot change a route, policy,
//! grant, receipt or configuration by existing.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CANDIDATE_INPUT_SCHEMA: &str = "kiana.quality-candidate-input.v1";
pub const CANDIDATE_EVALUATOR_ID: &str = "candidate-binding";
const CANDIDATE_SCHEMA: &str = "kiana.quality-candidate.v1";
const MAX_TEXT_BYTES: usize = 512;
const MAX_DIMENSIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDimension {
    Model,
    Prompt,
    ToolCatalog,
    MemoryIndex,
    Workflow,
    Route,
}

impl CandidateDimension {
    fn key(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Prompt => "prompt",
            Self::ToolCatalog => "tool_catalog",
            Self::MemoryIndex => "memory_index",
            Self::Workflow => "workflow",
            Self::Route => "route",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Draft,
    OfflineEvaluated,
    Shadowed,
    Approved,
    Rejected,
    RolledBack,
    Deprecated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityCandidate {
    pub schema: String,
    pub candidate_id: String,
    pub baseline_id: String,
    pub suite_digest: String,
    pub changed_dimension: CandidateDimension,
    pub changed_version_ref: String,
    pub base_versions: BTreeMap<String, String>,
    pub passive_version_refs: BTreeMap<String, String>,
    pub snapshot_versions: BTreeMap<String, String>,
    pub snapshot_digest: String,
    pub owner_id: String,
    pub status: CandidateStatus,
    pub candidate_digest: String,
}

impl QualityCandidate {
    pub fn validate(&self) -> Result<(), &'static str> {
        let changed_key = self.changed_dimension.key();
        if self.schema != CANDIDATE_SCHEMA
            || !valid_text(&self.candidate_id)
            || !valid_text(&self.baseline_id)
            || !valid_text(&self.owner_id)
            || !valid_digest(&self.suite_digest)
            || !valid_digest(&self.changed_version_ref)
            || !valid_digest(&self.snapshot_digest)
            || !valid_digest(&self.candidate_digest)
            || self.base_versions.len() == 0
            || self.base_versions.len() > MAX_DIMENSIONS
            || self.snapshot_versions.len() != self.base_versions.len()
            || self.passive_version_refs.len() + 1 != self.base_versions.len()
            || !all_versions_valid(&self.base_versions)
            || !all_versions_valid(&self.passive_version_refs)
            || !all_versions_valid(&self.snapshot_versions)
            || self.candidate_digest != self.digest()
        {
            return Err("candidate_header_invalid");
        }
        let Some(base_changed) = self.base_versions.get(changed_key) else {
            return Err("candidate_changed_dimension_missing");
        };
        if base_changed == &self.changed_version_ref
            || self.passive_version_refs.contains_key(changed_key)
            || self.snapshot_versions.get(changed_key) != Some(&self.changed_version_ref)
        {
            return Err("candidate_primary_dimension_invalid");
        }
        for (dimension, version) in &self.base_versions {
            if dimension == changed_key {
                continue;
            }
            if self.passive_version_refs.get(dimension) != Some(version)
                || self.snapshot_versions.get(dimension) != Some(version)
            {
                return Err("candidate_passive_version_drift");
            }
        }
        let snapshot_digest = Self::snapshot_digest(&self.snapshot_versions);
        if snapshot_digest != self.snapshot_digest {
            return Err("candidate_snapshot_digest_mismatch");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "candidate_id": self.candidate_id,
            "baseline_id": self.baseline_id,
            "suite_digest": self.suite_digest,
            "changed_dimension": self.changed_dimension,
            "changed_version_ref": self.changed_version_ref,
            "base_versions": self.base_versions,
            "passive_version_refs": self.passive_version_refs,
            "snapshot_versions": self.snapshot_versions,
            "snapshot_digest": self.snapshot_digest,
            "owner_id": self.owner_id,
            "status": self.status,
        }))
    }

    pub fn snapshot_digest(versions: &BTreeMap<String, String>) -> String {
        json_digest(&serde_json::json!(versions))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateInput {
    pub schema: String,
    pub candidate: QualityCandidate,
}

impl CandidateInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CANDIDATE_INPUT_SCHEMA {
            return Err("candidate_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CandidateEvaluator;

impl DeterministicEvaluator for CandidateEvaluator {
    fn evaluator_id(&self) -> &str {
        CANDIDATE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: CandidateInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("candidate"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        let mut findings = Vec::new();
        if decoded.candidate.validate().is_err() {
            emit(
                &mut findings,
                "candidate.binding_invalid",
                "single_dimension_snapshot_binding",
                "invalid",
            )?;
        }
        Ok(findings)
    }
}

fn all_versions_valid(values: &BTreeMap<String, String>) -> bool {
    values.len() <= MAX_DIMENSIONS
        && values
            .iter()
            .all(|(dimension, digest)| valid_text(dimension) && valid_digest(digest))
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: &'static str,
    actual: &'static str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::new(
            code,
            serde_json::Value::String(expected.to_owned()),
            serde_json::Value::String(actual.to_owned()),
            format!("candidate binding finding: {code}"),
            "candidate:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
