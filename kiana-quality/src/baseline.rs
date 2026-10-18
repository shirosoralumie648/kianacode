//! EQ-39 baseline registry and compatibility comparison contract.
//!
//! A baseline binds a suite, case, target and evaluator version digest with an owner, a bounded
//! validity window and refresh provenance.  The registry is digest checked and the comparison
//! rejects expired, not-yet-valid, tampered or drifted baselines before a candidate result may be
//! compared against them, so a stale or incompatible baseline can never become a silent
//! comparison base.
//!
//! This is a pure value contract.  It does not refresh a baseline, execute a target, persist an
//! EvalStore record, invoke a judge, alter a candidate or submit a gate decision.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Versioned schema for one baseline record.
pub const BASELINE_RECORD_SCHEMA: &str = "kiana.quality-baseline-record.v1";
/// Versioned schema for one digest-bound baseline registry.
pub const BASELINE_REGISTRY_SCHEMA: &str = "kiana.quality-baseline-registry.v1";
/// Versioned envelope for one direct record comparison.
pub const BASELINE_INPUT_SCHEMA: &str = "kiana.quality-baseline-input.v1";
/// Versioned envelope for one registry lookup comparison.
pub const BASELINE_REGISTRY_INPUT_SCHEMA: &str = "kiana.quality-baseline-registry-input.v1";
/// Stable registry identifier for the direct baseline comparison evaluator.
pub const BASELINE_EVALUATOR_ID: &str = "baseline-registry";
/// Stable registry identifier for the registry lookup comparison evaluator.
pub const BASELINE_LOOKUP_EVALUATOR_ID: &str = "baseline-registry-lookup";
/// Maximum registry entries accepted in one comparison.
pub const MAX_BASELINE_ENTRIES: usize = 512;
/// Maximum bytes accepted in one bounded identity or provenance token.
pub const MAX_BASELINE_TEXT_BYTES: usize = 512;
/// Maximum validity window accepted for one baseline (180 days).
pub const MAX_BASELINE_TTL_MS: u64 = 180 * 24 * 60 * 60 * 1_000;

const COMPARISON_EVIDENCE: &str = "baseline:input";
const REGISTRY_EVIDENCE: &str = "baseline:registry";
const INPUT_SCHEMA_INVALID: &str = "baseline_input_schema_invalid";
const INPUT_TIME_INVALID: &str = "baseline_input_time_invalid";
const INPUT_DIGEST_INVALID: &str = "baseline_input_digest_invalid";
const REGISTRY_INPUT_SCHEMA_INVALID: &str = "baseline_registry_input_schema_invalid";
const REGISTRY_INPUT_IDENTITY_INVALID: &str = "baseline_registry_input_identity_invalid";

/// One immutable baseline binding.
///
/// `owner_id` and `refresh_provenance` are bounded evidence tokens, not authenticated authority.
/// `refreshed_from_baseline_digest` stays empty for a fresh capture and otherwise names the
/// predecessor record a refresh replaced.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRecord {
    /// Wire schema; must equal [`BASELINE_RECORD_SCHEMA`].
    pub schema: String,
    /// Stable baseline identity inside a registry.
    pub baseline_id: String,
    /// Digest of the suite version this baseline was captured for.
    pub suite_digest: String,
    /// Digest of the case set this baseline was captured for.
    pub case_digest: String,
    /// Digest of the target snapshot this baseline was captured for.
    pub target_digest: String,
    /// Digest of the evaluator set that produced the baseline.
    pub evaluator_digest: String,
    /// Bounded owner identity; evidence, not authenticated authority.
    pub owner_id: String,
    /// Capture time in Unix milliseconds.
    pub created_at_unix_ms: u64,
    /// Exclusive expiry in Unix milliseconds; a comparison at or after it is stale.
    pub expires_at_unix_ms: u64,
    /// Provenance of the capture or refresh act that produced this record.
    pub refresh_provenance: String,
    /// Digest of the predecessor baseline this record refreshed, or empty for a fresh capture.
    pub refreshed_from_baseline_digest: String,
    /// Canonical digest binding every other field.
    pub baseline_digest: String,
}

impl BaselineRecord {
    /// Build a fresh record and bind its canonical digest.  Use [`Self::refreshed_from`] to mark
    /// it as a refresh of an earlier baseline.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        baseline_id: impl Into<String>,
        suite_digest: impl Into<String>,
        case_digest: impl Into<String>,
        target_digest: impl Into<String>,
        evaluator_digest: impl Into<String>,
        owner_id: impl Into<String>,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        refresh_provenance: impl Into<String>,
    ) -> Self {
        let mut record = Self {
            schema: BASELINE_RECORD_SCHEMA.to_owned(),
            baseline_id: baseline_id.into(),
            suite_digest: suite_digest.into(),
            case_digest: case_digest.into(),
            target_digest: target_digest.into(),
            evaluator_digest: evaluator_digest.into(),
            owner_id: owner_id.into(),
            created_at_unix_ms,
            expires_at_unix_ms,
            refresh_provenance: refresh_provenance.into(),
            refreshed_from_baseline_digest: String::new(),
            baseline_digest: String::new(),
        };
        record.baseline_digest = record.digest();
        record
    }

    /// Mark this record as a refresh of the predecessor with the supplied baseline digest.
    pub fn refreshed_from(mut self, predecessor_digest: impl Into<String>) -> Self {
        self.refreshed_from_baseline_digest = predecessor_digest.into();
        self.baseline_digest = self.digest();
        self
    }

    /// Validate the schema, bounded identity, validity window, digest shapes and digest binding.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BASELINE_RECORD_SCHEMA {
            return Err("baseline_record_schema_invalid");
        }
        if !valid_text(&self.baseline_id)
            || !valid_text(&self.owner_id)
            || !valid_text(&self.refresh_provenance)
        {
            return Err("baseline_record_identity_invalid");
        }
        if self.created_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.created_at_unix_ms
            || self.expires_at_unix_ms - self.created_at_unix_ms > MAX_BASELINE_TTL_MS
        {
            return Err("baseline_record_window_invalid");
        }
        if !valid_digest(&self.suite_digest)
            || !valid_digest(&self.case_digest)
            || !valid_digest(&self.target_digest)
            || !valid_digest(&self.evaluator_digest)
            || !valid_digest(&self.baseline_digest)
        {
            return Err("baseline_record_digest_invalid");
        }
        if !self.refreshed_from_baseline_digest.is_empty()
            && (!valid_digest(&self.refreshed_from_baseline_digest)
                || self.refreshed_from_baseline_digest == self.baseline_digest)
        {
            return Err("baseline_record_digest_invalid");
        }
        if self.baseline_digest != self.digest() {
            return Err("baseline_record_digest_mismatch");
        }
        Ok(())
    }

    /// Canonical digest binding identity, version digests, owner, window and provenance.
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "baseline_id": self.baseline_id,
            "suite_digest": self.suite_digest,
            "case_digest": self.case_digest,
            "target_digest": self.target_digest,
            "evaluator_digest": self.evaluator_digest,
            "owner_id": self.owner_id,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "refresh_provenance": self.refresh_provenance,
            "refreshed_from_baseline_digest": self.refreshed_from_baseline_digest,
        }))
    }

    /// The suite/case/target/evaluator tuple a reference must match to compare.
    fn version_key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.suite_digest, self.case_digest, self.target_digest, self.evaluator_digest
        )
    }
}

/// A digest-bound set of baseline records.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRegistry {
    /// Wire schema; must equal [`BASELINE_REGISTRY_SCHEMA`].
    pub schema: String,
    /// Bounded baseline records, identified by `baseline_id`.
    pub entries: Vec<BaselineRecord>,
    /// Canonical digest binding the schema and every entry.
    pub registry_digest: String,
}

impl BaselineRegistry {
    /// Build a registry and bind its canonical digest.
    pub fn new(entries: Vec<BaselineRecord>) -> Self {
        let mut registry = Self {
            schema: BASELINE_REGISTRY_SCHEMA.to_owned(),
            entries,
            registry_digest: String::new(),
        };
        registry.registry_digest = registry.digest();
        registry
    }

    /// Resolve one baseline by stable identity.
    pub fn lookup(&self, baseline_id: &str) -> Option<&BaselineRecord> {
        self.entries
            .iter()
            .find(|entry| entry.baseline_id == baseline_id)
    }

    /// Validate identity, size, digest binding, entry uniqueness and refresh chains.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BASELINE_REGISTRY_SCHEMA {
            return Err("baseline_registry_schema_invalid");
        }
        if self.entries.is_empty() {
            return Err("baseline_registry_empty");
        }
        if self.entries.len() > MAX_BASELINE_ENTRIES {
            return Err("baseline_registry_size_exceeded");
        }
        if !valid_digest(&self.registry_digest) {
            return Err("baseline_registry_digest_invalid");
        }
        if self.registry_digest != self.digest() {
            return Err("baseline_registry_digest_mismatch");
        }
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !ids.insert(entry.baseline_id.as_str()) {
                return Err("baseline_registry_duplicate");
            }
        }
        self.validate_refresh_chains()
    }

    /// A refreshed baseline may not shadow a live predecessor: entries that bind the same version
    /// tuple must have disjoint windows, name the immediately preceding record as their
    /// predecessor and come from a new capture provenance.
    fn validate_refresh_chains(&self) -> Result<(), &'static str> {
        let mut groups: BTreeMap<String, Vec<&BaselineRecord>> = BTreeMap::new();
        for entry in &self.entries {
            groups.entry(entry.version_key()).or_default().push(entry);
        }
        for entries in groups.values_mut() {
            entries.sort_by(|left, right| {
                left.created_at_unix_ms
                    .cmp(&right.created_at_unix_ms)
                    .then_with(|| left.baseline_id.cmp(&right.baseline_id))
            });
            for pair in entries.windows(2) {
                let earlier = pair[0];
                let later = pair[1];
                if later.created_at_unix_ms < earlier.expires_at_unix_ms {
                    return Err("baseline_registry_overlap");
                }
                if later.refreshed_from_baseline_digest != earlier.baseline_digest {
                    return Err("baseline_registry_provenance_mismatch");
                }
                if later.refresh_provenance == earlier.refresh_provenance {
                    return Err("baseline_registry_provenance_reused");
                }
            }
        }
        Ok(())
    }

    /// Canonical digest binding the registry schema and every entry.
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "entries": self.entries,
        }))
    }
}

/// Envelope for one direct record comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineComparisonInput {
    /// Wire schema; must equal [`BASELINE_INPUT_SCHEMA`].
    pub schema: String,
    /// The baseline record the candidate version tuple is compared against.
    pub baseline: BaselineRecord,
    /// Comparison time in Unix milliseconds.
    pub now_unix_ms: u64,
    /// Candidate suite version digest.
    pub suite_digest: String,
    /// Candidate case set digest.
    pub case_digest: String,
    /// Candidate target snapshot digest.
    pub target_digest: String,
    /// Candidate evaluator set digest.
    pub evaluator_digest: String,
}

impl BaselineComparisonInput {
    /// Build a comparison envelope for one record.
    pub fn new(
        baseline: BaselineRecord,
        now_unix_ms: u64,
        suite_digest: impl Into<String>,
        case_digest: impl Into<String>,
        target_digest: impl Into<String>,
        evaluator_digest: impl Into<String>,
    ) -> Self {
        Self {
            schema: BASELINE_INPUT_SCHEMA.to_owned(),
            baseline,
            now_unix_ms,
            suite_digest: suite_digest.into(),
            case_digest: case_digest.into(),
            target_digest: target_digest.into(),
            evaluator_digest: evaluator_digest.into(),
        }
    }

    /// Validate the envelope before any baseline is compared.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BASELINE_INPUT_SCHEMA {
            return Err(INPUT_SCHEMA_INVALID);
        }
        validate_comparison_tail(
            self.now_unix_ms,
            &self.suite_digest,
            &self.case_digest,
            &self.target_digest,
            &self.evaluator_digest,
        )
    }

    /// Serialize the envelope into its wire value.
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("baseline comparison input is serializable")
    }
}

/// Envelope for one registry lookup comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRegistryInput {
    /// Wire schema; must equal [`BASELINE_REGISTRY_INPUT_SCHEMA`].
    pub schema: String,
    /// The digest-bound registry the named baseline is resolved from.
    pub registry: BaselineRegistry,
    /// Stable identity of the baseline to compare against.
    pub baseline_id: String,
    /// Comparison time in Unix milliseconds.
    pub now_unix_ms: u64,
    /// Candidate suite version digest.
    pub suite_digest: String,
    /// Candidate case set digest.
    pub case_digest: String,
    /// Candidate target snapshot digest.
    pub target_digest: String,
    /// Candidate evaluator set digest.
    pub evaluator_digest: String,
}

impl BaselineRegistryInput {
    /// Build a registry lookup envelope.
    pub fn new(
        registry: BaselineRegistry,
        baseline_id: impl Into<String>,
        now_unix_ms: u64,
        suite_digest: impl Into<String>,
        case_digest: impl Into<String>,
        target_digest: impl Into<String>,
        evaluator_digest: impl Into<String>,
    ) -> Self {
        Self {
            schema: BASELINE_REGISTRY_INPUT_SCHEMA.to_owned(),
            registry,
            baseline_id: baseline_id.into(),
            now_unix_ms,
            suite_digest: suite_digest.into(),
            case_digest: case_digest.into(),
            target_digest: target_digest.into(),
            evaluator_digest: evaluator_digest.into(),
        }
    }

    /// Validate the envelope before the registry is resolved.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BASELINE_REGISTRY_INPUT_SCHEMA {
            return Err(REGISTRY_INPUT_SCHEMA_INVALID);
        }
        if !valid_text(&self.baseline_id) {
            return Err(REGISTRY_INPUT_IDENTITY_INVALID);
        }
        validate_comparison_tail(
            self.now_unix_ms,
            &self.suite_digest,
            &self.case_digest,
            &self.target_digest,
            &self.evaluator_digest,
        )
    }

    /// Serialize the envelope into its wire value.
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("baseline registry input is serializable")
    }
}

/// Direct baseline comparison evaluator over one record.
#[derive(Clone, Copy, Debug, Default)]
pub struct BaselineEvaluator;

impl DeterministicEvaluator for BaselineEvaluator {
    fn evaluator_id(&self) -> &str {
        BASELINE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: BaselineComparisonInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("baseline"))?;
        evaluate_baseline_comparison(&decoded)
    }
}

/// Registry lookup comparison evaluator.
#[derive(Clone, Copy, Debug, Default)]
pub struct BaselineRegistryEvaluator;

impl DeterministicEvaluator for BaselineRegistryEvaluator {
    fn evaluator_id(&self) -> &str {
        BASELINE_LOOKUP_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: BaselineRegistryInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("baseline_registry"))?;
        evaluate_baseline_lookup(&decoded)
    }
}

/// Compare a candidate version tuple against one baseline record.
///
/// A record that fails validation, a comparison before the capture time, an expired baseline and
/// any suite/case/target/evaluator drift each produce a stable finding; an empty finding list is
/// the only comparable result.
pub fn evaluate_baseline_comparison(
    input: &BaselineComparisonInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    input.validate().map_err(EvaluatorError::InputInvalid)?;
    let mut findings = Vec::new();
    compare_record(
        &input.baseline,
        input.now_unix_ms,
        &input.suite_digest,
        &input.case_digest,
        &input.target_digest,
        &input.evaluator_digest,
        COMPARISON_EVIDENCE,
        &mut findings,
    )?;
    Ok(findings)
}

/// Resolve one baseline from a digest-bound registry and compare a candidate version tuple.
///
/// A registry that fails validation is reported as `baseline.registry_invalid` and never
/// compared; an absent identity is reported as `baseline.record_missing`; the resolved record is
/// then compared exactly like a direct record comparison.
pub fn evaluate_baseline_lookup(
    input: &BaselineRegistryInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    input.validate().map_err(EvaluatorError::InputInvalid)?;
    let mut findings = Vec::new();
    if let Err(reason) = input.registry.validate() {
        emit(
            &mut findings,
            "baseline.registry_invalid",
            "validated_baseline_registry",
            reason,
            REGISTRY_EVIDENCE,
        )?;
        return Ok(findings);
    }
    let Some(record) = input.registry.lookup(&input.baseline_id) else {
        emit(
            &mut findings,
            "baseline.record_missing",
            "registered_baseline_record",
            "missing",
            REGISTRY_EVIDENCE,
        )?;
        return Ok(findings);
    };
    compare_record(
        record,
        input.now_unix_ms,
        &input.suite_digest,
        &input.case_digest,
        &input.target_digest,
        &input.evaluator_digest,
        REGISTRY_EVIDENCE,
        &mut findings,
    )?;
    Ok(findings)
}

fn validate_comparison_tail(
    now_unix_ms: u64,
    suite_digest: &str,
    case_digest: &str,
    target_digest: &str,
    evaluator_digest: &str,
) -> Result<(), &'static str> {
    if now_unix_ms == 0 {
        return Err(INPUT_TIME_INVALID);
    }
    if !valid_digest(suite_digest)
        || !valid_digest(case_digest)
        || !valid_digest(target_digest)
        || !valid_digest(evaluator_digest)
    {
        return Err(INPUT_DIGEST_INVALID);
    }
    Ok(())
}

fn compare_record(
    record: &BaselineRecord,
    now_unix_ms: u64,
    suite_digest: &str,
    case_digest: &str,
    target_digest: &str,
    evaluator_digest: &str,
    evidence_ref: &'static str,
    findings: &mut Vec<Finding>,
) -> Result<(), EvaluatorError> {
    if let Err(reason) = record.validate() {
        emit(
            findings,
            "baseline.record_invalid",
            "validated_baseline_record",
            reason,
            evidence_ref,
        )?;
        return Ok(());
    }
    if now_unix_ms < record.created_at_unix_ms {
        emit(
            findings,
            "baseline.not_yet_valid",
            "baseline_window_contains_now",
            "not_yet_valid",
            evidence_ref,
        )?;
        return Ok(());
    }
    if now_unix_ms >= record.expires_at_unix_ms {
        emit(
            findings,
            "baseline.stale",
            "unexpired_baseline",
            "expired",
            evidence_ref,
        )?;
    }
    for (axis, expected, actual) in [
        ("suite_digest", record.suite_digest.as_str(), suite_digest),
        ("case_digest", record.case_digest.as_str(), case_digest),
        (
            "target_digest",
            record.target_digest.as_str(),
            target_digest,
        ),
        (
            "evaluator_digest",
            record.evaluator_digest.as_str(),
            evaluator_digest,
        ),
    ] {
        if expected != actual {
            emit(
                findings,
                "baseline.incompatible",
                axis,
                "digest_mismatch",
                evidence_ref,
            )?;
        }
    }
    Ok(())
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_BASELINE_TEXT_BYTES
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
            format!("baseline finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
