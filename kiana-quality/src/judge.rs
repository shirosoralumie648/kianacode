//! EQ-35 optional semantic-judge evidence contract.
//!
//! The repository already owns the async `kiana-ports::Judge` boundary; this module is the pure
//! quality-side contract that decides whether one judge result is usable evidence.  It fixes the
//! judge identity (provider, model version, prompt version, prompt digest, temperature and
//! configuration digest), binds the request and result to strict SHA-256 digests and keeps
//! `Available`, `Unavailable` and `NotConfigured` explicit.  An unavailable or unconfigured judge
//! is never a pass: a required judge blocks, and an optional judge stays a visible
//! `judge.not_evaluated` diagnostic instead of being silently counted as success.
//!
//! This module cannot reach a model, account, network, filesystem, runner, broker or gate.  Judge
//! output is a quality diagnostic only; it never changes a grant, a receipt or a candidate's
//! standing.  Every caller-supplied value passes through [`Finding::redacted`] and each finding
//! cites the evidence section it came from.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::redact_text;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Versioned envelope for one semantic judge evaluation input.
pub const SEMANTIC_JUDGE_INPUT_SCHEMA: &str = "kiana.quality-semantic-judge-input.v1";
/// Stable registry identifier for the semantic judge evaluator.
pub const SEMANTIC_JUDGE_EVALUATOR_ID: &str = "semantic-judge";
const CONFIG_SCHEMA: &str = "kiana.quality-semantic-judge-config.v1";
const REQUEST_SCHEMA: &str = "kiana.quality-semantic-judge-request.v1";
const RESULT_SCHEMA: &str = "kiana.quality-semantic-judge-result.v1";
/// Maximum bytes accepted in one judge identity/reference string.
pub const MAX_JUDGE_TEXT_BYTES: usize = 512;
/// Maximum unavailable-reason bytes accepted before the envelope fails closed.
pub const MAX_JUDGE_REASON_BYTES: usize = 512;
/// Maximum temperature accepted for a fixed judge, in thousandths (2.000).
pub const MAX_JUDGE_TEMPERATURE_MILLI: u16 = 2_000;
/// Upper bound for a judge score in thousandths.
pub const MAX_JUDGE_SCORE_MILLI: u16 = 1_000;

const AVAILABILITY_EVIDENCE: &str = "judge:availability";
const CONFIG_EVIDENCE: &str = "judge:config";
const REQUEST_EVIDENCE: &str = "judge:request";
const RESULT_EVIDENCE: &str = "judge:result";

/// Availability observed for one judge call.
///
/// `NotConfigured` means no judge is wired at all, `Unavailable` means a wired judge could not
/// produce evidence.  Both are explicit failures, never an implicit pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeAvailability {
    Available,
    Unavailable,
    NotConfigured,
}

/// Verdict reported by a judge whose evidence is available.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeVerdict {
    Pass,
    Fail,
    NeedsReview,
}

/// Fixed judge identity.  The prompt digest and the configuration digest bind the exact prompt,
/// model and sampling settings; a drifting request is reported instead of being re-interpreted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticJudgeConfig {
    pub schema: String,
    pub provider_id: String,
    pub model_version: String,
    pub prompt_version: String,
    /// Strict `sha256:` digest of the exact prompt bytes executed by the judge.
    pub prompt_digest: String,
    pub temperature_milli: u16,
    /// Strict `sha256:` digest of the full fixed configuration identity.
    pub configuration_digest: String,
}

/// One judge request bound to the fixed configuration and to the compared evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticJudgeRequest {
    pub schema: String,
    pub configuration_digest: String,
    pub prompt_digest: String,
    pub input_digest: String,
    pub reference_digest: String,
    pub output_digest: String,
}

/// One judge result.  The score is bounded and the digest binds the verdict/score pair.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticJudgeResult {
    pub schema: String,
    pub verdict: JudgeVerdict,
    pub score_milli: u16,
    pub result_digest: String,
}

/// Versioned input envelope for [`SemanticJudgeEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticJudgeInput {
    pub schema: String,
    /// Whether the suite requires a semantic verdict for this case.
    pub required: bool,
    pub availability: JudgeAvailability,
    pub config: Option<SemanticJudgeConfig>,
    pub request: Option<SemanticJudgeRequest>,
    pub result: Option<SemanticJudgeResult>,
    pub unavailable_reason: Option<String>,
}

impl SemanticJudgeInput {
    /// Build evidence for a judge that produced a result.  The three evidence sections are
    /// inseparable in this constructor, so "available without evidence" cannot be declared here.
    pub fn available(
        required: bool,
        config: SemanticJudgeConfig,
        request: SemanticJudgeRequest,
        result: SemanticJudgeResult,
    ) -> Self {
        Self {
            schema: SEMANTIC_JUDGE_INPUT_SCHEMA.to_owned(),
            required,
            availability: JudgeAvailability::Available,
            config: Some(config),
            request: Some(request),
            result: Some(result),
            unavailable_reason: None,
        }
    }

    /// Build evidence for a wired judge that could not produce a result.
    pub fn unavailable(required: bool, reason: impl Into<String>) -> Self {
        Self::without_judge(
            required,
            JudgeAvailability::Unavailable,
            Some(reason.into()),
        )
    }

    /// Build evidence for an installation with no judge wired at all.
    pub fn not_configured(required: bool, reason: impl Into<String>) -> Self {
        Self::without_judge(
            required,
            JudgeAvailability::NotConfigured,
            Some(reason.into()),
        )
    }

    fn without_judge(
        required: bool,
        availability: JudgeAvailability,
        unavailable_reason: Option<String>,
    ) -> Self {
        Self {
            schema: SEMANTIC_JUDGE_INPUT_SCHEMA.to_owned(),
            required,
            availability,
            config: None,
            request: None,
            result: None,
            unavailable_reason,
        }
    }

    /// Hard envelope bounds.  Structural drift and an oversized reason fail closed before any
    /// finding is produced; entry shape problems remain findings so the report explains *why* the
    /// judge evidence is unusable.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SEMANTIC_JUDGE_INPUT_SCHEMA {
            return Err("semantic_judge_input_schema_invalid");
        }
        if self
            .unavailable_reason
            .as_ref()
            .is_some_and(|reason| reason.len() > MAX_JUDGE_REASON_BYTES)
        {
            return Err("semantic_judge_reason_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("semantic judge input is serializable")
    }
}

/// Pure semantic-judge evaluator.  All model, network and gate authority stays outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct SemanticJudgeEvaluator;

impl DeterministicEvaluator for SemanticJudgeEvaluator {
    fn evaluator_id(&self) -> &str {
        SEMANTIC_JUDGE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: SemanticJudgeInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("semantic_judge"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_semantic_judge(&decoded)
    }
}

/// Evaluate one judge evidence envelope without invoking a judge.
///
/// Check order is deliberate: availability first (so an unavailable judge can never be read as a
/// pass), then the fixed configuration identity, then configuration/prompt drift in the request,
/// then the bounded result.  A failing or review verdict from an available judge stays visible
/// instead of being collapsed into success.
pub fn evaluate_semantic_judge(input: &SemanticJudgeInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();

    match input.availability {
        JudgeAvailability::Available => {
            if input.config.is_none() || input.request.is_none() || input.result.is_none() {
                emit(
                    &mut findings,
                    "judge.evidence_missing",
                    text("fixed_config_request_and_result"),
                    availability_actual(input),
                    AVAILABILITY_EVIDENCE,
                )?;
            }
            if input.unavailable_reason.is_some() {
                emit(
                    &mut findings,
                    "judge.unavailable_reason_unexpected",
                    text("no_reason_for_an_available_judge"),
                    availability_actual(input),
                    AVAILABILITY_EVIDENCE,
                )?;
            }
        }
        JudgeAvailability::Unavailable | JudgeAvailability::NotConfigured => {
            let not_configured = matches!(input.availability, JudgeAvailability::NotConfigured);
            if input.required {
                let code = if not_configured {
                    "judge.not_configured"
                } else {
                    "judge.unavailable"
                };
                emit(
                    &mut findings,
                    code,
                    text("available_judge_evidence"),
                    availability_actual(input),
                    AVAILABILITY_EVIDENCE,
                )?;
            } else {
                // An optional judge that did not run is a visible diagnostic, never a silent pass.
                emit(
                    &mut findings,
                    "judge.not_evaluated",
                    text("explicit_not_evaluated_not_pass"),
                    availability_actual(input),
                    AVAILABILITY_EVIDENCE,
                )?;
            }
            if let Some(result) = &input.result {
                if result.verdict == JudgeVerdict::Pass {
                    emit(
                        &mut findings,
                        "judge.unavailable_not_pass",
                        text("no_pass_verdict_from_an_unavailable_judge"),
                        result_actual(result),
                        AVAILABILITY_EVIDENCE,
                    )?;
                } else {
                    emit(
                        &mut findings,
                        "judge.evidence_unexpected",
                        text("no_result_from_an_unavailable_judge"),
                        result_actual(result),
                        AVAILABILITY_EVIDENCE,
                    )?;
                }
            }
            if input
                .unavailable_reason
                .as_deref()
                .is_none_or(|reason| !is_bounded_identity(reason))
            {
                emit(
                    &mut findings,
                    "judge.unavailable_reason_missing",
                    text("bounded_unavailable_reason"),
                    availability_actual(input),
                    AVAILABILITY_EVIDENCE,
                )?;
            }
        }
    }

    if let Some(config) = &input.config {
        check_schema(
            &mut findings,
            "judge.config_schema_invalid",
            &config.schema,
            CONFIG_SCHEMA,
            CONFIG_EVIDENCE,
        )?;
        if !is_bounded_identity(&config.provider_id)
            || !is_bounded_identity(&config.model_version)
            || !is_bounded_identity(&config.prompt_version)
            || !is_sha256_digest(&config.prompt_digest)
            || config.temperature_milli > MAX_JUDGE_TEMPERATURE_MILLI
            || !is_sha256_digest(&config.configuration_digest)
        {
            emit(
                &mut findings,
                "judge.config_invalid",
                text("fixed_bounded_versioned_prompt_identity"),
                config_actual(config),
                CONFIG_EVIDENCE,
            )?;
        }
    }

    if let Some(request) = &input.request {
        check_schema(
            &mut findings,
            "judge.request_schema_invalid",
            &request.schema,
            REQUEST_SCHEMA,
            REQUEST_EVIDENCE,
        )?;
        if !is_sha256_digest(&request.configuration_digest)
            || !is_sha256_digest(&request.prompt_digest)
            || !is_sha256_digest(&request.input_digest)
            || !is_sha256_digest(&request.reference_digest)
            || !is_sha256_digest(&request.output_digest)
        {
            emit(
                &mut findings,
                "judge.request_invalid",
                text("sha256_config_prompt_input_reference_output"),
                request_actual(request),
                REQUEST_EVIDENCE,
            )?;
        }
        if let Some(config) = &input.config {
            if config.configuration_digest != request.configuration_digest {
                emit(
                    &mut findings,
                    "judge.config_drift",
                    text("request_uses_the_fixed_configuration"),
                    request_actual(request),
                    REQUEST_EVIDENCE,
                )?;
            }
            if config.prompt_digest != request.prompt_digest {
                emit(
                    &mut findings,
                    "judge.prompt_drift",
                    text("request_uses_the_fixed_prompt"),
                    request_actual(request),
                    REQUEST_EVIDENCE,
                )?;
            }
        }
    }

    if let Some(result) = &input.result {
        check_schema(
            &mut findings,
            "judge.result_schema_invalid",
            &result.schema,
            RESULT_SCHEMA,
            RESULT_EVIDENCE,
        )?;
        if result.score_milli > MAX_JUDGE_SCORE_MILLI || !is_sha256_digest(&result.result_digest) {
            emit(
                &mut findings,
                "judge.result_invalid",
                text("bounded_score_and_sha256_result_digest"),
                result_actual(result),
                RESULT_EVIDENCE,
            )?;
        }
        if matches!(input.availability, JudgeAvailability::Available) {
            match result.verdict {
                JudgeVerdict::Pass => {
                    if result.score_milli == 0 {
                        emit(
                            &mut findings,
                            "judge.result_inconsistent",
                            text("pass_verdict_with_a_positive_score"),
                            result_actual(result),
                            RESULT_EVIDENCE,
                        )?;
                    }
                }
                JudgeVerdict::Fail => emit(
                    &mut findings,
                    "judge.verdict_failed",
                    text("pass_verdict"),
                    result_actual(result),
                    RESULT_EVIDENCE,
                )?,
                JudgeVerdict::NeedsReview => emit(
                    &mut findings,
                    "judge.verdict_needs_review",
                    text("reviewed_verdict"),
                    result_actual(result),
                    RESULT_EVIDENCE,
                )?,
            }
        }
    }

    Ok(findings)
}

fn check_schema(
    findings: &mut Vec<Finding>,
    code: &'static str,
    actual: &str,
    expected: &'static str,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if actual != expected {
        emit(findings, code, text(expected), text(actual), evidence_ref)?;
    }
    Ok(())
}

/// Bounded, single-line, secret-free identity accepted for provider/model/prompt references.
fn is_bounded_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_JUDGE_TEXT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'#' | b'-' | b'@')
        })
        && redact_text(value) == value
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn availability_actual(input: &SemanticJudgeInput) -> Value {
    serde_json::json!({
        "availability": input.availability,
        "required": input.required,
        "has_config": input.config.is_some(),
        "has_request": input.request.is_some(),
        "has_result": input.result.is_some(),
        "reason_bounded": input
            .unavailable_reason
            .as_deref()
            .is_some_and(is_bounded_identity),
    })
}

fn config_actual(config: &SemanticJudgeConfig) -> Value {
    serde_json::json!({
        "provider_id": config.provider_id,
        "model_version": config.model_version,
        "prompt_version": config.prompt_version,
        "prompt_digest_bound": is_sha256_digest(&config.prompt_digest),
        "temperature_milli": config.temperature_milli,
        "configuration_digest_bound": is_sha256_digest(&config.configuration_digest),
    })
}

fn request_actual(request: &SemanticJudgeRequest) -> Value {
    serde_json::json!({
        "configuration_digest_bound": is_sha256_digest(&request.configuration_digest),
        "prompt_digest_bound": is_sha256_digest(&request.prompt_digest),
        "input_digest_bound": is_sha256_digest(&request.input_digest),
        "reference_digest_bound": is_sha256_digest(&request.reference_digest),
        "output_digest_bound": is_sha256_digest(&request.output_digest),
    })
}

fn result_actual(result: &SemanticJudgeResult) -> Value {
    serde_json::json!({
        "verdict": result.verdict,
        "score_milli": result.score_milli,
        "result_digest_bound": is_sha256_digest(&result.result_digest),
    })
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// Push one finding.  Caller-supplied values go through [`Finding::redacted`] so a secret-shaped
/// identity cannot escape through the finding itself.
fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: Value,
    actual: Value,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::redacted(
            code,
            expected,
            actual,
            format!("semantic judge finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
