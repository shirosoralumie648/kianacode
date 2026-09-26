//! EQ-44 shadow admission, sample/TTL and rollback evidence contract.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};

pub const SHADOW_INPUT_SCHEMA: &str = "kiana.quality-shadow-input.v1";
pub const SHADOW_EVALUATOR_ID: &str = "shadow-admission";
const ADMISSION_SCHEMA: &str = "kiana.quality-shadow-admission.v1";
const OBSERVATION_SCHEMA: &str = "kiana.quality-shadow-observation.v1";
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowAdmission {
    pub schema: String,
    pub candidate_digest: String,
    pub baseline_digest: String,
    pub route_digest: String,
    pub grant_digest: String,
    pub sample_limit: u32,
    pub admitted_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub admission_digest: String,
}

impl ShadowAdmission {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != ADMISSION_SCHEMA
            || !all_digests_valid([
                &self.candidate_digest,
                &self.baseline_digest,
                &self.route_digest,
                &self.grant_digest,
                &self.admission_digest,
            ])
            || self.sample_limit == 0
            || self.admitted_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.admitted_at_unix_ms
            || self.admission_digest != self.digest()
        {
            return Err("shadow_admission_invalid");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "candidate_digest": self.candidate_digest,
            "baseline_digest": self.baseline_digest,
            "route_digest": self.route_digest,
            "grant_digest": self.grant_digest,
            "sample_limit": self.sample_limit,
            "admitted_at_unix_ms": self.admitted_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowObservation {
    pub schema: String,
    pub now_unix_ms: u64,
    pub samples_observed: u32,
    pub regression_detected: bool,
    pub rollback_requested: bool,
    pub rollback_route_digest: Option<String>,
    pub rollback_grant_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowInput {
    pub schema: String,
    pub admission: ShadowAdmission,
    pub observation: ShadowObservation,
}

impl ShadowInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SHADOW_INPUT_SCHEMA {
            return Err("shadow_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ShadowEvaluator;

impl DeterministicEvaluator for ShadowEvaluator {
    fn evaluator_id(&self) -> &str {
        SHADOW_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: ShadowInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("shadow"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_shadow(&decoded)
    }
}

pub fn evaluate_shadow(input: &ShadowInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    if input.admission.validate().is_err() {
        emit(
            &mut findings,
            "shadow.admission_invalid",
            "validated_admission",
            "invalid",
        )?;
        return Ok(findings);
    }
    if input.observation.schema != OBSERVATION_SCHEMA {
        emit(
            &mut findings,
            "shadow.observation_schema_invalid",
            OBSERVATION_SCHEMA,
            "invalid",
        )?;
    }
    if input.observation.now_unix_ms == 0 {
        emit(
            &mut findings,
            "shadow.clock_invalid",
            "positive_now",
            "invalid",
        )?;
    }
    if input.observation.now_unix_ms >= input.admission.expires_at_unix_ms {
        emit(
            &mut findings,
            "shadow.ttl_expired",
            "observation_before_expiry",
            "expired",
        )?;
    }
    if input.observation.samples_observed > input.admission.sample_limit {
        emit(
            &mut findings,
            "shadow.sample_limit_exceeded",
            "samples_within_limit",
            "exceeded",
        )?;
    }
    if input.observation.regression_detected {
        if !input.observation.rollback_requested {
            emit(
                &mut findings,
                "shadow.rollback_missing",
                "regression_has_rollback_route",
                "missing",
            )?;
        }
        if input.observation.rollback_route_digest != Some(input.admission.baseline_digest.clone())
        {
            emit(
                &mut findings,
                "shadow.rollback_route_invalid",
                "rollback_targets_baseline",
                "mismatch",
            )?;
        }
        if input.observation.rollback_grant_digest != Some(input.admission.grant_digest.clone()) {
            emit(
                &mut findings,
                "shadow.rollback_grant_changed",
                "rollback_preserves_grant",
                "changed",
            )?;
        }
    } else if input.observation.rollback_requested {
        emit(
            &mut findings,
            "shadow.rollback_without_regression",
            "rollback_requires_regression",
            "unexpected",
        )?;
    }
    if let Some(route) = &input.observation.rollback_route_digest {
        if !valid_digest(route) {
            emit(
                &mut findings,
                "shadow.rollback_route_invalid",
                "sha256_route_digest",
                "invalid",
            )?;
        }
    }
    if let Some(grant) = &input.observation.rollback_grant_digest {
        if !valid_digest(grant) {
            emit(
                &mut findings,
                "shadow.rollback_grant_invalid",
                "sha256_grant_digest",
                "invalid",
            )?;
        }
    }
    Ok(findings)
}

fn all_digests_valid<'a>(values: impl IntoIterator<Item = &'a String>) -> bool {
    values.into_iter().all(|value| valid_digest(value))
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[allow(dead_code)]
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_TEXT_BYTES && !value.contains('\0')
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
            format!("shadow admission finding: {code}"),
            "shadow:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
