//! EQ-34 performance and cost metric evaluation over bounded measurements.
//!
//! Metrics are diagnostic evidence, not a budget authority or a billing ledger. The evaluator
//! keeps estimated, measured and unknown cost buckets separate and requires explicit latency,
//! token and tool-call thresholds.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};

pub const PERFORMANCE_COST_INPUT_SCHEMA: &str = "kiana.quality-performance-cost-input.v1";
pub const PERFORMANCE_COST_EVALUATOR_ID: &str = "performance-cost";
const METRICS_SCHEMA: &str = "kiana.quality-performance-metrics.v1";
const THRESHOLDS_SCHEMA: &str = "kiana.quality-performance-thresholds.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostBucket {
    Estimated,
    Measured,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceMetrics {
    pub schema: String,
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub tool_calls: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub cache_key_digest: Option<String>,
    pub usage_complete: bool,
    pub cost_bucket: CostBucket,
    pub cost_micros: Option<u64>,
    pub measurement_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceThresholds {
    pub schema: String,
    pub max_duration_ms: u64,
    pub max_total_tokens: u64,
    pub max_tool_calls: u32,
    pub max_cost_micros: Option<u64>,
    pub require_measured_cost: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceCostInput {
    pub schema: String,
    pub metrics: PerformanceMetrics,
    pub thresholds: PerformanceThresholds,
}

impl PerformanceCostInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PERFORMANCE_COST_INPUT_SCHEMA {
            return Err("performance_cost_input_schema_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PerformanceCostEvaluator;

impl DeterministicEvaluator for PerformanceCostEvaluator {
    fn evaluator_id(&self) -> &str {
        PERFORMANCE_COST_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: PerformanceCostInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("performance_cost"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_performance_cost(&decoded)
    }
}

pub fn evaluate_performance_cost(
    input: &PerformanceCostInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let metrics = &input.metrics;
    let thresholds = &input.thresholds;

    check_schema(
        &mut findings,
        "metrics.measurement_schema_invalid",
        &metrics.schema,
        METRICS_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "metrics.threshold_schema_invalid",
        &thresholds.schema,
        THRESHOLDS_SCHEMA,
    )?;
    if !valid_digest(&metrics.measurement_digest) {
        emit(
            &mut findings,
            "metrics.measurement_provenance_invalid",
            "sha256_measurement_digest",
            "invalid",
        )?;
    }
    if thresholds.max_duration_ms == 0
        || thresholds.max_total_tokens == 0
        || thresholds.max_tool_calls == 0
        || (thresholds.require_measured_cost && thresholds.max_cost_micros.is_none())
    {
        emit(
            &mut findings,
            "metrics.threshold_missing",
            "explicit_positive_thresholds",
            "missing",
        )?;
    }

    if !metrics.usage_complete {
        emit(
            &mut findings,
            "metrics.usage_incomplete",
            "complete_usage",
            "incomplete",
        )?;
    }
    if metrics.total_tokens != metrics.input_tokens.saturating_add(metrics.output_tokens) {
        emit(
            &mut findings,
            "metrics.token_total_invalid",
            "input_plus_output_equals_total",
            "mismatch",
        )?;
    }
    if metrics.duration_ms > thresholds.max_duration_ms {
        emit(
            &mut findings,
            "metrics.duration_exceeded",
            "duration_within_threshold",
            "exceeded",
        )?;
    }
    if metrics.total_tokens > thresholds.max_total_tokens {
        emit(
            &mut findings,
            "metrics.tokens_exceeded",
            "tokens_within_threshold",
            "exceeded",
        )?;
    }
    if metrics.tool_calls > thresholds.max_tool_calls {
        emit(
            &mut findings,
            "metrics.tool_calls_exceeded",
            "tool_calls_within_threshold",
            "exceeded",
        )?;
    }

    if metrics.cache_hits.saturating_add(metrics.cache_misses) > 0
        && metrics
            .cache_key_digest
            .as_deref()
            .is_none_or(|digest| !valid_digest(digest))
    {
        emit(
            &mut findings,
            "metrics.cache_key_missing",
            "cache_usage_has_key_digest",
            "missing_or_invalid",
        )?;
    }

    match metrics.cost_bucket {
        CostBucket::Measured => {
            if metrics.cost_micros.is_none() {
                emit(
                    &mut findings,
                    "metrics.cost_missing",
                    "measured_cost_value",
                    "missing",
                )?;
            }
            if thresholds.require_measured_cost && metrics.cost_micros.is_none() {
                emit(
                    &mut findings,
                    "metrics.cost_unmeasured",
                    "measured_cost_required",
                    "missing",
                )?;
            }
        }
        CostBucket::Estimated => {
            if metrics.cost_micros.is_none() {
                emit(
                    &mut findings,
                    "metrics.cost_missing",
                    "estimated_cost_value",
                    "missing",
                )?;
            }
            if thresholds.require_measured_cost {
                emit(
                    &mut findings,
                    "metrics.cost_unmeasured",
                    "measured_cost_required",
                    "estimated",
                )?;
            }
        }
        CostBucket::Unknown => {
            emit(
                &mut findings,
                "metrics.cost_unknown",
                "known_estimated_or_measured_cost",
                "unknown",
            )?;
        }
    }
    if let (Some(cost), Some(limit)) = (metrics.cost_micros, thresholds.max_cost_micros) {
        if cost > limit {
            emit(
                &mut findings,
                "metrics.cost_exceeded",
                "cost_within_threshold",
                "exceeded",
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
            format!("performance and cost finding: {code}"),
            "metrics:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
