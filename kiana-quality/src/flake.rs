//! EQ-37 retry-once flake, quarantine and infrastructure-failure evidence.
//!
//! A second attempt is a classifier aid, never permission to count a failure as a pass. Flaky
//! and infrastructure outcomes remain visible and require bounded quarantine/classification
//! evidence.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};

pub const FLAKE_INPUT_SCHEMA: &str = "kiana.quality-flake-input.v1";
pub const FLAKE_EVALUATOR_ID: &str = "flake-classifier";
const ATTEMPT_SCHEMA: &str = "kiana.quality-flake-attempt.v1";
const QUARANTINE_SCHEMA: &str = "kiana.quality-flake-quarantine.v1";
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Pass,
    Fail,
    Infra,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfraFailureClass {
    Runner,
    Fixture,
    Network,
    Resource,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlakeFinalStatus {
    Pass,
    Fail,
    Infra,
    NeedsReview,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakeAttempt {
    pub schema: String,
    pub attempt: u8,
    pub outcome: AttemptOutcome,
    pub infra_class: Option<InfraFailureClass>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakeQuarantine {
    pub schema: String,
    pub case_id: String,
    pub reason: String,
    pub owner: String,
    pub expires_at_unix_ms: u64,
    pub evidence_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakeInput {
    pub schema: String,
    pub retry_once: bool,
    pub attempts: Vec<FlakeAttempt>,
    pub final_status: FlakeFinalStatus,
    pub quarantine: Option<FlakeQuarantine>,
}

impl FlakeInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != FLAKE_INPUT_SCHEMA || self.attempts.len() > 2 {
            return Err("flake_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FlakeClassifierEvaluator;

impl DeterministicEvaluator for FlakeClassifierEvaluator {
    fn evaluator_id(&self) -> &str {
        FLAKE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: FlakeInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("flake"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_flake(&decoded)
    }
}

pub fn evaluate_flake(input: &FlakeInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    if input.attempts.is_empty() {
        emit(
            &mut findings,
            "flake.attempt_missing",
            "one_or_two_attempts",
            "missing",
        )?;
    }
    for (index, attempt) in input.attempts.iter().enumerate() {
        check_schema(
            &mut findings,
            "flake.attempt_schema_invalid",
            &attempt.schema,
            ATTEMPT_SCHEMA,
        )?;
        if attempt.attempt != (index + 1) as u8 {
            emit(
                &mut findings,
                "flake.attempt_order_invalid",
                "sequential_attempts",
                "invalid",
            )?;
        }
        if attempt.outcome == AttemptOutcome::Infra
            && matches!(attempt.infra_class, None | Some(InfraFailureClass::Unknown))
        {
            emit(
                &mut findings,
                "flake.infra_unclassified",
                "known_infra_failure_class",
                "unknown",
            )?;
        }
    }
    if !input.retry_once && input.attempts.len() > 1 {
        emit(
            &mut findings,
            "flake.retry_policy_invalid",
            "retry_once_enabled",
            "multiple_attempts",
        )?;
    }
    if input.attempts.get(1).is_some_and(|attempt| {
        input
            .attempts
            .first()
            .is_some_and(|first| first.outcome == AttemptOutcome::Pass)
            && attempt.outcome != AttemptOutcome::Pass
    }) {
        emit(
            &mut findings,
            "flake.retry_after_pass",
            "no_retry_after_pass",
            "retry",
        )?;
    }

    let flaky = input.attempts.len() == 2
        && input.attempts[0].outcome != AttemptOutcome::Pass
        && input.attempts[1].outcome == AttemptOutcome::Pass;
    let infra_present = input
        .attempts
        .iter()
        .any(|attempt| attempt.outcome == AttemptOutcome::Infra);
    if flaky {
        if input.final_status == FlakeFinalStatus::Pass {
            emit(
                &mut findings,
                "flake.flaky_counted_as_pass",
                "needs_review_or_quarantine",
                "pass",
            )?;
        }
        if input.quarantine.is_none() {
            emit(
                &mut findings,
                "flake.quarantine_missing",
                "flaky_case_has_quarantine",
                "missing",
            )?;
        }
    }
    if infra_present
        && matches!(
            input.final_status,
            FlakeFinalStatus::Pass | FlakeFinalStatus::Fail
        )
    {
        emit(
            &mut findings,
            "flake.infra_counted_as_result",
            "infra_or_quarantine_status",
            "result",
        )?;
    }
    if input.final_status == FlakeFinalStatus::Quarantined && input.quarantine.is_none() {
        emit(
            &mut findings,
            "flake.quarantine_missing",
            "quarantine_record",
            "missing",
        )?;
    }
    if let Some(quarantine) = &input.quarantine {
        check_schema(
            &mut findings,
            "flake.quarantine_schema_invalid",
            &quarantine.schema,
            QUARANTINE_SCHEMA,
        )?;
        if !valid_text(&quarantine.case_id)
            || !valid_text(&quarantine.reason)
            || !valid_text(&quarantine.owner)
            || quarantine.expires_at_unix_ms == 0
            || !valid_digest(&quarantine.evidence_digest)
        {
            emit(
                &mut findings,
                "flake.quarantine_invalid",
                "bounded_owner_reason_expiry_digest",
                "invalid",
            )?;
        }
    }
    Ok(findings)
}

fn check_schema(
    findings: &mut Vec<Finding>,
    code: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), EvaluatorError> {
    if actual != expected {
        emit(findings, code, expected, "invalid")?;
    }
    Ok(())
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
            format!("flake classifier finding: {code}"),
            "flake:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
