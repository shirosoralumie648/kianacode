//! EQ-39 baseline registry and compatibility comparison contract.
//!
//! Baselines are comparison evidence, not authority. Expiry, owner/provenance and every version
//! digest are checked before a caller may compare a candidate result.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BASELINE_INPUT_SCHEMA: &str = "kiana.quality-baseline-input.v1";
pub const BASELINE_EVALUATOR_ID: &str = "baseline-registry";
const BASELINE_SCHEMA: &str = "kiana.quality-baseline-record.v1";
const REGISTRY_SCHEMA: &str = "kiana.quality-baseline-registry.v1";
const MAX_TEXT_BYTES: usize = 512;
const MAX_ENTRIES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRecord {
    pub schema: String,
    pub baseline_id: String,
    pub suite_digest: String,
    pub case_digest: String,
    pub target_digest: String,
    pub evaluator_digest: String,
    pub owner_id: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub refresh_provenance: String,
    pub baseline_digest: String,
}

impl BaselineRecord {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BASELINE_SCHEMA
            || !valid_text(&self.baseline_id)
            || !valid_text(&self.owner_id)
            || !valid_text(&self.refresh_provenance)
            || self.created_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.created_at_unix_ms
            || !all_digests_valid([
                &self.suite_digest,
                &self.case_digest,
                &self.target_digest,
                &self.evaluator_digest,
                &self.baseline_digest,
            ])
            || self.baseline_digest != self.digest()
        {
            return Err("baseline_record_invalid");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
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
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRegistry {
    pub schema: String,
    pub entries: Vec<BaselineRecord>,
    pub registry_digest: String,
}

impl BaselineRegistry {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != REGISTRY_SCHEMA
            || self.entries.is_empty()
            || self.entries.len() > MAX_ENTRIES
            || !valid_digest(&self.registry_digest)
            || self.registry_digest != self.digest()
        {
            return Err("baseline_registry_invalid");
        }
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !ids.insert(&entry.baseline_id) {
                return Err("baseline_registry_duplicate");
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "entries": self.entries,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineComparisonInput {
    pub schema: String,
    pub baseline: BaselineRecord,
    pub now_unix_ms: u64,
    pub suite_digest: String,
    pub case_digest: String,
    pub target_digest: String,
    pub evaluator_digest: String,
}

impl BaselineComparisonInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BASELINE_INPUT_SCHEMA || self.now_unix_ms == 0 {
            return Err("baseline_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BaselineEvaluator;

impl DeterministicEvaluator for BaselineEvaluator {
    fn evaluator_id(&self) -> &str {
        BASELINE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: BaselineComparisonInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("baseline"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_baseline_comparison(&decoded)
    }
}

pub fn evaluate_baseline_comparison(
    input: &BaselineComparisonInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    if input.baseline.validate().is_err() {
        emit(
            &mut findings,
            "baseline.record_invalid",
            "validated_baseline_record",
            "invalid",
        )?;
        return Ok(findings);
    }
    if input.now_unix_ms >= input.baseline.expires_at_unix_ms {
        emit(
            &mut findings,
            "baseline.stale",
            "unexpired_baseline",
            "expired",
        )?;
    }
    for (expected, actual) in [
        (&input.baseline.suite_digest, &input.suite_digest),
        (&input.baseline.case_digest, &input.case_digest),
        (&input.baseline.target_digest, &input.target_digest),
        (&input.baseline.evaluator_digest, &input.evaluator_digest),
    ] {
        if !valid_digest(actual) || expected != actual {
            emit(
                &mut findings,
                "baseline.incompatible",
                "matching_version_digest",
                "mismatch",
            )?;
        }
    }
    Ok(findings)
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

fn all_digests_valid<'a>(values: impl IntoIterator<Item = &'a String>) -> bool {
    values.into_iter().all(|value| valid_digest(value))
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
            format!("baseline finding: {code}"),
            "baseline:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
