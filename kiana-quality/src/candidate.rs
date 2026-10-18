//! EQ-40 single-primary-dimension `QualityCandidate` and version-snapshot binding.
//!
//! A candidate records exactly one primary version change (model, prompt, tool catalog, memory
//! index, workflow or route), one explicit passive version reference for every other snapshot
//! dimension, the recomputed snapshot digest, the bound baseline identity/suite digest and its own
//! canonical digest.  Hidden model/prompt/tool drift, missing passive references, unknown
//! dimensions, a no-op change and snapshot/digest tampering fail closed, and the admission
//! evaluator refuses a candidate whose bound baseline identity, baseline digest, suite digest or
//! validity window does not match the supplied EQ-39 baseline record.
//!
//! This is a pure value contract.  It does not alter a route, policy, grant, receipt or
//! configuration, does not execute a target, does not persist a candidate and does not decide a
//! quality gate.

use crate::baseline::BaselineRecord;
use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Versioned wire schema for one quality candidate.
pub const CANDIDATE_SCHEMA: &str = "kiana.quality-candidate.v2";
/// Versioned envelope for one candidate-only validation.
pub const CANDIDATE_INPUT_SCHEMA: &str = "kiana.quality-candidate-input.v2";
/// Versioned envelope for one candidate-to-baseline admission check.
pub const CANDIDATE_ADMISSION_INPUT_SCHEMA: &str = "kiana.quality-candidate-admission-input.v1";
/// Stable registry identifier for the candidate-only evaluator.
pub const CANDIDATE_EVALUATOR_ID: &str = "candidate-binding";
/// Stable registry identifier for the candidate/baseline admission evaluator.
pub const CANDIDATE_ADMISSION_EVALUATOR_ID: &str = "candidate-baseline-admission";
/// Maximum snapshot dimensions: the six known version dimensions.
pub const MAX_CANDIDATE_DIMENSIONS: usize = 6;
/// Maximum bytes accepted in one bounded identity or version token.
pub const MAX_CANDIDATE_TEXT_BYTES: usize = 512;

const CANDIDATE_EVIDENCE: &str = "candidate:input";
const ADMISSION_EVIDENCE: &str = "candidate:admission";
const BINDING_INVALID: &str = "candidate.binding_invalid";
const BASELINE_INVALID: &str = "candidate.baseline_invalid";
const BASELINE_IDENTITY_MISMATCH: &str = "candidate.baseline_identity_mismatch";
const SUITE_DIGEST_MISMATCH: &str = "candidate.suite_digest_mismatch";
const BASELINE_NOT_YET_VALID: &str = "candidate.baseline_not_yet_valid";
const BASELINE_STALE: &str = "candidate.baseline_stale";
const STATUS_NOT_ADMISSIBLE: &str = "candidate.status_not_admissible";

/// One version dimension that a candidate may change.
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
    /// The stable version-map key for this dimension.
    pub fn key(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Prompt => "prompt",
            Self::ToolCatalog => "tool_catalog",
            Self::MemoryIndex => "memory_index",
            Self::Workflow => "workflow",
            Self::Route => "route",
        }
    }

    /// Resolve a version-map key back to a known dimension.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "model" => Some(Self::Model),
            "prompt" => Some(Self::Prompt),
            "tool_catalog" => Some(Self::ToolCatalog),
            "memory_index" => Some(Self::MemoryIndex),
            "workflow" => Some(Self::Workflow),
            "route" => Some(Self::Route),
            _ => None,
        }
    }
}

/// Lifecycle status of a candidate.
///
/// Only [`CandidateStatus::Draft`] may be admitted against a baseline; every later state is a
/// decision outcome recorded elsewhere and may not be re-bound to a baseline by this contract.
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

impl CandidateStatus {
    /// The stable wire key for this status.
    pub fn key(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::OfflineEvaluated => "offline_evaluated",
            Self::Shadowed => "shadowed",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::RolledBack => "rolled_back",
            Self::Deprecated => "deprecated",
        }
    }
}

/// One candidate with exactly one primary version change and a digest-bound version snapshot.
///
/// `base_versions` records the baseline version of every dimension the candidate touches,
/// `passive_version_refs` explicitly repeats the version of every dimension that does not change,
/// and `snapshot_versions` is the candidate version snapshot (one dimension changed).  `owner_id`
/// is a bounded evidence token, not authenticated authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityCandidate {
    pub schema: String,
    pub candidate_id: String,
    pub baseline_id: String,
    pub baseline_digest: String,
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
    /// Build a draft candidate that changes exactly one version dimension.
    ///
    /// The passive references and the candidate snapshot are derived from `base_versions`, so a
    /// caller cannot construct a candidate that hides a second change or omits a passive
    /// reference: only `changed_dimension` may differ from the baseline versions, and it must
    /// differ.  The snapshot and candidate digests are recomputed and the result is validated
    /// before it is returned.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        candidate_id: impl Into<String>,
        baseline_id: impl Into<String>,
        baseline_digest: impl Into<String>,
        suite_digest: impl Into<String>,
        changed_dimension: CandidateDimension,
        base_versions: BTreeMap<String, String>,
        changed_version_ref: impl Into<String>,
        owner_id: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let changed_key = changed_dimension.key();
        let changed_version_ref = changed_version_ref.into();
        if !base_versions.contains_key(changed_key) {
            return Err("candidate_changed_dimension_missing");
        }
        let mut passive_version_refs = BTreeMap::new();
        let mut snapshot_versions = BTreeMap::new();
        for (dimension, version) in &base_versions {
            if dimension == changed_key {
                snapshot_versions.insert(dimension.clone(), changed_version_ref.clone());
            } else {
                passive_version_refs.insert(dimension.clone(), version.clone());
                snapshot_versions.insert(dimension.clone(), version.clone());
            }
        }
        let mut candidate = Self {
            schema: CANDIDATE_SCHEMA.to_owned(),
            candidate_id: candidate_id.into(),
            baseline_id: baseline_id.into(),
            baseline_digest: baseline_digest.into(),
            suite_digest: suite_digest.into(),
            changed_dimension,
            changed_version_ref,
            base_versions,
            passive_version_refs,
            snapshot_versions,
            snapshot_digest: String::new(),
            owner_id: owner_id.into(),
            status: CandidateStatus::Draft,
            candidate_digest: String::new(),
        };
        candidate.snapshot_digest = Self::snapshot_digest(&candidate.snapshot_versions);
        candidate.candidate_digest = candidate.digest();
        candidate.validate()?;
        Ok(candidate)
    }

    /// Validate the schema, bounded identities, digest shapes, the single primary change and the
    /// version-snapshot binding.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CANDIDATE_SCHEMA
            || !valid_text(&self.candidate_id)
            || !valid_text(&self.baseline_id)
            || !valid_text(&self.owner_id)
            || !valid_digest(&self.baseline_digest)
            || !valid_digest(&self.suite_digest)
            || !valid_digest(&self.changed_version_ref)
            || !valid_digest(&self.snapshot_digest)
            || !valid_digest(&self.candidate_digest)
            || self.base_versions.is_empty()
            || self.base_versions.len() > MAX_CANDIDATE_DIMENSIONS
            || self.snapshot_versions.len() != self.base_versions.len()
            || self.passive_version_refs.len() + 1 != self.base_versions.len()
            || !all_versions_valid(&self.base_versions)
            || !all_versions_valid(&self.passive_version_refs)
            || !all_versions_valid(&self.snapshot_versions)
        {
            return Err("candidate_header_invalid");
        }
        for dimension in self
            .base_versions
            .keys()
            .chain(self.passive_version_refs.keys())
            .chain(self.snapshot_versions.keys())
        {
            if CandidateDimension::from_key(dimension).is_none() {
                return Err("candidate_dimension_unknown");
            }
        }
        let changed_key = self.changed_dimension.key();
        let Some(base_changed) = self.base_versions.get(changed_key) else {
            return Err("candidate_changed_dimension_missing");
        };
        if base_changed == &self.changed_version_ref {
            return Err("candidate_changed_dimension_unchanged");
        }
        if self.passive_version_refs.contains_key(changed_key)
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
        if Self::snapshot_digest(&self.snapshot_versions) != self.snapshot_digest {
            return Err("candidate_snapshot_digest_mismatch");
        }
        if self.candidate_digest != self.digest() {
            return Err("candidate_digest_mismatch");
        }
        Ok(())
    }

    /// Canonical digest binding every candidate field, including the baseline binding and the
    /// complete version maps.
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "candidate_id": self.candidate_id,
            "baseline_id": self.baseline_id,
            "baseline_digest": self.baseline_digest,
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

    /// Canonical digest of one version snapshot.
    pub fn snapshot_digest(versions: &BTreeMap<String, String>) -> String {
        json_digest(&serde_json::json!(versions))
    }
}

/// Envelope for one candidate-only validation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateInput {
    /// Wire schema; must equal [`CANDIDATE_INPUT_SCHEMA`].
    pub schema: String,
    /// The candidate value under validation.
    pub candidate: QualityCandidate,
}

impl CandidateInput {
    /// Build the candidate-only validation envelope.
    pub fn new(candidate: QualityCandidate) -> Self {
        Self {
            schema: CANDIDATE_INPUT_SCHEMA.to_owned(),
            candidate,
        }
    }

    /// Validate the envelope schema before the candidate is checked.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CANDIDATE_INPUT_SCHEMA {
            return Err("candidate_input_schema_invalid");
        }
        Ok(())
    }

    /// Serialize the envelope into its wire value.
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("candidate input is serializable")
    }
}

/// Envelope binding one candidate to the EQ-39 baseline record it claims to change.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateAdmissionInput {
    /// Wire schema; must equal [`CANDIDATE_ADMISSION_INPUT_SCHEMA`].
    pub schema: String,
    /// Admission time in Unix milliseconds.
    pub now_unix_ms: u64,
    /// The committed baseline record the candidate is bound to.
    pub baseline: BaselineRecord,
    /// The candidate value under admission.
    pub candidate: QualityCandidate,
}

impl CandidateAdmissionInput {
    /// Build the candidate-to-baseline admission envelope.
    pub fn new(now_unix_ms: u64, baseline: BaselineRecord, candidate: QualityCandidate) -> Self {
        Self {
            schema: CANDIDATE_ADMISSION_INPUT_SCHEMA.to_owned(),
            now_unix_ms,
            baseline,
            candidate,
        }
    }

    /// Validate the envelope schema and admission time before any binding is checked.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CANDIDATE_ADMISSION_INPUT_SCHEMA {
            return Err("candidate_admission_input_schema_invalid");
        }
        if self.now_unix_ms == 0 {
            return Err("candidate_admission_input_time_invalid");
        }
        Ok(())
    }

    /// Serialize the envelope into its wire value.
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("candidate admission input is serializable")
    }
}

/// Candidate-only evaluator over one value object.
#[derive(Clone, Copy, Debug, Default)]
pub struct CandidateEvaluator;

impl DeterministicEvaluator for CandidateEvaluator {
    fn evaluator_id(&self) -> &str {
        CANDIDATE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: CandidateInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("candidate"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        let mut findings = Vec::new();
        if let Err(reason) = decoded.candidate.validate() {
            emit(
                &mut findings,
                BINDING_INVALID,
                "valid_single_dimension_candidate",
                reason,
                CANDIDATE_EVIDENCE,
            )?;
        }
        Ok(findings)
    }
}

/// Candidate/baseline admission evaluator.
#[derive(Clone, Copy, Debug, Default)]
pub struct CandidateAdmissionEvaluator;

impl DeterministicEvaluator for CandidateAdmissionEvaluator {
    fn evaluator_id(&self) -> &str {
        CANDIDATE_ADMISSION_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: CandidateAdmissionInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("candidate_admission"))?;
        evaluate_candidate_admission(&decoded)
    }
}

/// Admit one candidate against the committed baseline record it is bound to.
///
/// An invalid record or candidate, a baseline identity/suite mismatch, a not-yet-valid or stale
/// baseline window and a non-draft candidate status each produce a stable finding; an empty
/// finding list is the only admissible result.  The evaluator compares caller-supplied evidence
/// only and never mutates a route, policy, grant or gate.
pub fn evaluate_candidate_admission(
    input: &CandidateAdmissionInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    input.validate().map_err(EvaluatorError::InputInvalid)?;
    let mut findings = Vec::new();
    if let Err(reason) = input.baseline.validate() {
        emit(
            &mut findings,
            BASELINE_INVALID,
            "validated_baseline_record",
            reason,
            ADMISSION_EVIDENCE,
        )?;
        return Ok(findings);
    }
    if let Err(reason) = input.candidate.validate() {
        emit(
            &mut findings,
            BINDING_INVALID,
            "valid_single_dimension_candidate",
            reason,
            ADMISSION_EVIDENCE,
        )?;
        return Ok(findings);
    }
    if input.candidate.baseline_id != input.baseline.baseline_id {
        emit(
            &mut findings,
            BASELINE_IDENTITY_MISMATCH,
            "bound_baseline_id",
            "baseline_id",
            ADMISSION_EVIDENCE,
        )?;
    }
    if input.candidate.baseline_digest != input.baseline.baseline_digest {
        emit(
            &mut findings,
            BASELINE_IDENTITY_MISMATCH,
            "bound_baseline_digest",
            "baseline_digest",
            ADMISSION_EVIDENCE,
        )?;
    }
    if input.candidate.suite_digest != input.baseline.suite_digest {
        emit(
            &mut findings,
            SUITE_DIGEST_MISMATCH,
            "baseline_suite_digest",
            "suite_digest",
            ADMISSION_EVIDENCE,
        )?;
    }
    if input.now_unix_ms < input.baseline.created_at_unix_ms {
        emit(
            &mut findings,
            BASELINE_NOT_YET_VALID,
            "baseline_window_contains_now",
            "not_yet_valid",
            ADMISSION_EVIDENCE,
        )?;
    } else if input.now_unix_ms >= input.baseline.expires_at_unix_ms {
        emit(
            &mut findings,
            BASELINE_STALE,
            "unexpired_baseline",
            "expired",
            ADMISSION_EVIDENCE,
        )?;
    }
    if input.candidate.status != CandidateStatus::Draft {
        emit(
            &mut findings,
            STATUS_NOT_ADMISSIBLE,
            "draft_candidate",
            input.candidate.status.key(),
            ADMISSION_EVIDENCE,
        )?;
    }
    Ok(findings)
}

fn all_versions_valid(values: &BTreeMap<String, String>) -> bool {
    values.len() <= MAX_CANDIDATE_DIMENSIONS
        && values
            .iter()
            .all(|(dimension, digest)| valid_text(dimension) && valid_digest(digest))
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_CANDIDATE_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: &'static str,
    actual: &'static str,
    evidence_ref: &'static str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::new(
            code,
            Value::String(expected.to_owned()),
            Value::String(actual.to_owned()),
            format!("candidate binding finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
