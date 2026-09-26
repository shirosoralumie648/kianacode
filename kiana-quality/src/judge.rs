//! EQ-35 optional semantic-judge evidence contract.
//!
//! The repository already owns the async `kiana-ports::Judge` boundary. This module keeps the
//! quality-side fixed configuration/result contract and fail-closed evaluation separate from any
//! adapter: an unavailable judge can never be represented as a pass or a quality-gate approval.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};

pub const SEMANTIC_JUDGE_INPUT_SCHEMA: &str = "kiana.quality-semantic-judge-input.v1";
pub const SEMANTIC_JUDGE_EVALUATOR_ID: &str = "semantic-judge";
const CONFIG_SCHEMA: &str = "kiana.quality-semantic-judge-config.v1";
const REQUEST_SCHEMA: &str = "kiana.quality-semantic-judge-request.v1";
const RESULT_SCHEMA: &str = "kiana.quality-semantic-judge-result.v1";
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeAvailability {
    Available,
    Unavailable,
    NotConfigured,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeVerdict {
    Pass,
    Fail,
    NeedsReview,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticJudgeConfig {
    pub schema: String,
    pub provider_id: String,
    pub model_version: String,
    pub prompt_version: String,
    pub temperature_milli: u16,
    pub configuration_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticJudgeRequest {
    pub schema: String,
    pub configuration_digest: String,
    pub input_digest: String,
    pub reference_digest: String,
    pub output_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticJudgeResult {
    pub schema: String,
    pub verdict: JudgeVerdict,
    pub score_milli: u16,
    pub result_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticJudgeInput {
    pub schema: String,
    pub required: bool,
    pub availability: JudgeAvailability,
    pub config: Option<SemanticJudgeConfig>,
    pub request: Option<SemanticJudgeRequest>,
    pub result: Option<SemanticJudgeResult>,
    pub unavailable_reason: Option<String>,
}

impl SemanticJudgeInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SEMANTIC_JUDGE_INPUT_SCHEMA {
            return Err("semantic_judge_input_schema_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SemanticJudgeEvaluator;

impl DeterministicEvaluator for SemanticJudgeEvaluator {
    fn evaluator_id(&self) -> &str {
        SEMANTIC_JUDGE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: SemanticJudgeInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("semantic_judge"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_semantic_judge(&decoded)
    }
}

pub fn evaluate_semantic_judge(input: &SemanticJudgeInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let fixed = input.config.as_ref();
    if matches!(input.availability, JudgeAvailability::Available) {
        if fixed.is_none() || input.request.is_none() || input.result.is_none() {
            emit(
                &mut findings,
                "judge.evidence_missing",
                "config_request_result",
                "missing",
            )?;
        }
    } else {
        if input.required {
            emit(
                &mut findings,
                "judge.unavailable",
                "judge_available_or_blocked",
                "unavailable",
            )?;
        }
        if input
            .result
            .as_ref()
            .is_some_and(|result| result.verdict == JudgeVerdict::Pass)
        {
            emit(
                &mut findings,
                "judge.unavailable_not_pass",
                "unavailable_cannot_pass",
                "pass",
            )?;
        }
        if input
            .unavailable_reason
            .as_deref()
            .is_none_or(|reason| !valid_text(reason))
        {
            emit(
                &mut findings,
                "judge.unavailable_reason_missing",
                "bounded_reason",
                "missing",
            )?;
        }
    }

    if let Some(config) = fixed {
        check_schema(
            &mut findings,
            "judge.config_schema_invalid",
            &config.schema,
            CONFIG_SCHEMA,
        )?;
        if !valid_text(&config.provider_id)
            || !valid_text(&config.model_version)
            || !valid_text(&config.prompt_version)
            || config.temperature_milli > 2_000
            || !valid_digest(&config.configuration_digest)
        {
            emit(
                &mut findings,
                "judge.config_invalid",
                "fixed_bounded_versioned_config",
                "invalid",
            )?;
        }
    }
    if let Some(request) = &input.request {
        check_schema(
            &mut findings,
            "judge.request_schema_invalid",
            &request.schema,
            REQUEST_SCHEMA,
        )?;
        if !valid_digest(&request.configuration_digest)
            || !valid_digest(&request.input_digest)
            || !valid_digest(&request.reference_digest)
            || !valid_digest(&request.output_digest)
        {
            emit(
                &mut findings,
                "judge.request_invalid",
                "sha256_input_reference_output_config",
                "invalid",
            )?;
        }
        if fixed.is_some_and(|config| config.configuration_digest != request.configuration_digest) {
            emit(
                &mut findings,
                "judge.config_drift",
                "request_uses_fixed_config",
                "mismatch",
            )?;
        }
    }
    if let Some(result) = &input.result {
        check_schema(
            &mut findings,
            "judge.result_schema_invalid",
            &result.schema,
            RESULT_SCHEMA,
        )?;
        if result.score_milli > 1_000 || !valid_digest(&result.result_digest) {
            emit(
                &mut findings,
                "judge.result_invalid",
                "bounded_score_and_digest",
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
            format!("semantic judge finding: {code}"),
            "judge:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
