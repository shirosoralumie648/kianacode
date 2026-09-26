//! EQ-36 dimension aggregation and threshold evidence.
//!
//! Aggregates are diagnostics only. They make sample sufficiency and confidence explicit and do
//! not advance a candidate or mutate a quality gate.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const AGGREGATION_INPUT_SCHEMA: &str = "kiana.quality-aggregation-input.v1";
pub const AGGREGATION_EVALUATOR_ID: &str = "aggregation";
const DIMENSION_SCHEMA: &str = "kiana.quality-dimension-aggregate.v1";
const MAX_DIMENSIONS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateVerdict {
    Pass,
    Fail,
    NeedsReview,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionAggregate {
    pub schema: String,
    pub dimension: String,
    pub sample_count: u32,
    pub minimum_samples: u32,
    pub observed_milli: u64,
    pub baseline_milli: Option<u64>,
    pub absolute_limit_milli: Option<u64>,
    pub relative_limit_milli: Option<u64>,
    pub confidence_low_milli: Option<u64>,
    pub confidence_high_milli: Option<u64>,
    pub confidence_required: bool,
    pub verdict: AggregateVerdict,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggregationInput {
    pub schema: String,
    pub dimensions: Vec<DimensionAggregate>,
}

impl AggregationInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AGGREGATION_INPUT_SCHEMA || self.dimensions.len() > MAX_DIMENSIONS {
            return Err("aggregation_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AggregationEvaluator;

impl DeterministicEvaluator for AggregationEvaluator {
    fn evaluator_id(&self) -> &str {
        AGGREGATION_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: AggregationInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("aggregation"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_aggregation(&decoded)
    }
}

pub fn evaluate_aggregation(input: &AggregationInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let mut dimensions = BTreeSet::new();
    for aggregate in &input.dimensions {
        check_schema(
            &mut findings,
            "aggregation.dimension_schema_invalid",
            &aggregate.schema,
            DIMENSION_SCHEMA,
        )?;
        if aggregate.dimension.trim().is_empty()
            || aggregate.dimension.len() > MAX_TEXT_BYTES
            || !dimensions.insert(&aggregate.dimension)
        {
            emit(
                &mut findings,
                "aggregation.dimension_invalid",
                "unique_dimension",
                "invalid_or_duplicate",
            )?;
        }
        if aggregate.minimum_samples == 0 {
            emit(
                &mut findings,
                "aggregation.minimum_sample_invalid",
                "positive_minimum_sample",
                "zero",
            )?;
        }
        let insufficient = aggregate.sample_count < aggregate.minimum_samples;
        if insufficient {
            emit(
                &mut findings,
                "aggregation.insufficient_sample",
                "sample_count_meets_minimum",
                "insufficient",
            )?;
        }
        if let Some(limit) = aggregate.absolute_limit_milli {
            if aggregate.observed_milli > limit {
                emit(
                    &mut findings,
                    "aggregation.absolute_threshold_exceeded",
                    "observed_within_absolute_limit",
                    "exceeded",
                )?;
            }
        }
        let mut relative_blocking = false;
        if let Some(relative_limit) = aggregate.relative_limit_milli {
            match aggregate.baseline_milli {
                Some(0) if aggregate.observed_milli > 0 => {
                    relative_blocking = true;
                    emit(
                        &mut findings,
                        "aggregation.relative_threshold_invalid",
                        "nonzero_baseline_for_relative_check",
                        "zero",
                    )?;
                }
                Some(baseline) => {
                    let delta = aggregate.observed_milli.abs_diff(baseline);
                    let relative_milli = delta.saturating_mul(1_000) / baseline.max(1);
                    if relative_milli > relative_limit {
                        relative_blocking = true;
                        emit(
                            &mut findings,
                            "aggregation.relative_threshold_exceeded",
                            "relative_delta_within_limit",
                            "exceeded",
                        )?;
                    }
                }
                None => {
                    relative_blocking = true;
                    emit(
                        &mut findings,
                        "aggregation.relative_threshold_invalid",
                        "baseline_for_relative_check",
                        "missing",
                    )?;
                }
            }
        }
        let confidence_blocking = aggregate.confidence_required
            && match (
                aggregate.confidence_low_milli,
                aggregate.confidence_high_milli,
            ) {
                (Some(low), Some(high)) => {
                    !(low <= high
                        && low <= aggregate.observed_milli
                        && aggregate.observed_milli <= high)
                }
                _ => true,
            };
        if aggregate.confidence_required {
            match (
                aggregate.confidence_low_milli,
                aggregate.confidence_high_milli,
            ) {
                (Some(low), Some(high))
                    if low <= high
                        && low <= aggregate.observed_milli
                        && aggregate.observed_milli <= high => {}
                (Some(low), Some(high)) if low <= high => {
                    emit(
                        &mut findings,
                        "aggregation.confidence_invalid",
                        "observed_inside_confidence_interval",
                        "outside",
                    )?;
                }
                _ => emit(
                    &mut findings,
                    "aggregation.confidence_missing",
                    "ordered_confidence_interval",
                    "missing_or_invalid",
                )?,
            }
        }
        let blocking = insufficient
            || aggregate
                .absolute_limit_milli
                .is_some_and(|limit| aggregate.observed_milli > limit)
            || relative_blocking
            || confidence_blocking;
        if blocking && aggregate.verdict == AggregateVerdict::Pass {
            emit(
                &mut findings,
                "aggregation.false_pass",
                "blocking_dimension_is_not_pass",
                "pass",
            )?;
        }
        if insufficient
            && !matches!(
                aggregate.verdict,
                AggregateVerdict::Blocked | AggregateVerdict::NeedsReview
            )
        {
            emit(
                &mut findings,
                "aggregation.sample_verdict_invalid",
                "blocked_or_needs_review",
                "other",
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
            format!("aggregation finding: {code}"),
            "aggregation:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
