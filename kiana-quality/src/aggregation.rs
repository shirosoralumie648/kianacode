//! EQ-36 dimension aggregation, absolute/relative threshold, minimum-sample and
//! confidence-interval contract.
//!
//! A dimension aggregate is caller-supplied evidence: a bounded dimension identity, a sample
//! count, an explicit minimum sample count, an observed per-mille measurement, optional
//! absolute/relative limits and an optional confidence interval.  The evaluator checks that the
//! evidence is bounded, explicit and self-consistent:
//!
//! - an insufficient sample is blocking, and it can only be `blocked` or `needs_review`; a
//!   `pass` verdict on it is reported as `aggregation.false_pass` and
//!   `aggregation.sample_verdict_invalid`;
//! - a dimension must declare at least one explicit absolute or relative threshold, a relative
//!   limit needs a usable nonzero baseline, and an exceeded limit is blocking;
//! - confidence bounds are explicit when required, must be an ordered pair that contains the
//!   observed value, and a required-but-absent interval is blocking;
//! - a `pass` verdict that conflicts with any blocking evidence emits `aggregation.false_pass`.
//!
//! This is a pure value boundary.  It does not run an experiment, compute statistics from raw
//! samples, update a gate, advance a candidate or reach a model, store, runner, broker or
//! filesystem.  Every caller-supplied value passes through [`Finding::redacted`] and each finding
//! cites the evidence section (`aggregation:dimension:<i>` or `aggregation:verdict`) it came from.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::redact_text;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Versioned envelope for one aggregation evaluation input.
pub const AGGREGATION_INPUT_SCHEMA: &str = "kiana.quality-aggregation-input.v1";
/// Stable registry identifier for the aggregation evaluator.
pub const AGGREGATION_EVALUATOR_ID: &str = "aggregation";
/// Versioned schema for one caller-supplied dimension aggregate.
pub const DIMENSION_AGGREGATE_SCHEMA: &str = "kiana.quality-dimension-aggregate.v1";
/// Maximum number of dimension aggregates accepted in one envelope.
pub const MAX_AGGREGATION_DIMENSIONS: usize = 128;
/// Maximum bytes accepted in one dimension identity.
pub const MAX_AGGREGATION_TEXT_BYTES: usize = 512;

const DIMENSION_EVIDENCE_PREFIX: &str = "aggregation:dimension:";
const VERDICT_EVIDENCE: &str = "aggregation:verdict";

/// Verdict declared by the caller for one dimension aggregate.
///
/// `Pass` is the only verdict this evaluator can contradict: blocking evidence plus a pass
/// verdict produces `aggregation.false_pass`.  `Fail`, `NeedsReview` and `Blocked` stay visible
/// as claims; an insufficient sample additionally requires `Blocked` or `NeedsReview`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateVerdict {
    Pass,
    Fail,
    NeedsReview,
    Blocked,
}

impl AggregateVerdict {
    /// Every verdict accepted by this contract, in stable rank order.
    pub const ALL: [Self; 4] = [Self::Pass, Self::Fail, Self::NeedsReview, Self::Blocked];

    /// Stable rank used for deterministic dimension ordering.
    pub const fn rank(self) -> u8 {
        match self {
            AggregateVerdict::Pass => 0,
            AggregateVerdict::Fail => 1,
            AggregateVerdict::NeedsReview => 2,
            AggregateVerdict::Blocked => 3,
        }
    }
}

/// One caller-supplied dimension aggregate.  Values are per-mille integers so the contract stays
/// in exact integer arithmetic without floating point.
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

impl DimensionAggregate {
    /// Build a dimension aggregate with no threshold or interval evidence yet.  Callers add the
    /// policies they declare through the `with_*` methods.
    pub fn new(
        dimension: impl Into<String>,
        sample_count: u32,
        minimum_samples: u32,
        observed_milli: u64,
        verdict: AggregateVerdict,
    ) -> Self {
        Self {
            schema: DIMENSION_AGGREGATE_SCHEMA.to_owned(),
            dimension: dimension.into(),
            sample_count,
            minimum_samples,
            observed_milli,
            baseline_milli: None,
            absolute_limit_milli: None,
            relative_limit_milli: None,
            confidence_low_milli: None,
            confidence_high_milli: None,
            confidence_required: false,
            verdict,
        }
    }

    pub fn with_baseline_milli(mut self, baseline_milli: u64) -> Self {
        self.baseline_milli = Some(baseline_milli);
        self
    }

    pub fn with_absolute_limit_milli(mut self, absolute_limit_milli: u64) -> Self {
        self.absolute_limit_milli = Some(absolute_limit_milli);
        self
    }

    pub fn with_relative_limit_milli(mut self, relative_limit_milli: u64) -> Self {
        self.relative_limit_milli = Some(relative_limit_milli);
        self
    }

    pub fn with_confidence_interval(
        mut self,
        low_milli: u64,
        high_milli: u64,
        required: bool,
    ) -> Self {
        self.confidence_low_milli = Some(low_milli);
        self.confidence_high_milli = Some(high_milli);
        self.confidence_required = required;
        self
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("dimension aggregate is serializable")
    }
}

/// Versioned input envelope for [`AggregationEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggregationInput {
    pub schema: String,
    pub dimensions: Vec<DimensionAggregate>,
}

impl AggregationInput {
    pub fn new(dimensions: Vec<DimensionAggregate>) -> Self {
        Self {
            schema: AGGREGATION_INPUT_SCHEMA.to_owned(),
            dimensions,
        }
    }

    /// Hard envelope bounds fail closed before any finding is produced: a drifted schema, an
    /// empty dimension set and an oversized dimension list are structural refusals.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AGGREGATION_INPUT_SCHEMA {
            return Err("aggregation_input_schema_invalid");
        }
        if self.dimensions.is_empty() {
            return Err("aggregation_input_dimensions_missing");
        }
        if self.dimensions.len() > MAX_AGGREGATION_DIMENSIONS {
            return Err("aggregation_dimension_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("aggregation input is serializable")
    }
}

/// Pure dimension aggregation evaluator.  All scheduling, gate and promotion authority stays
/// outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct AggregationEvaluator;

impl DeterministicEvaluator for AggregationEvaluator {
    fn evaluator_id(&self) -> &str {
        AGGREGATION_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: AggregationInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("aggregation"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_aggregation(&decoded)
    }
}

/// Evaluate one aggregation envelope without running any experiment.
///
/// Dimensions are ordered by a stable full-record key before evaluation, so finding order,
/// evidence indices and the false-pass verdict never depend on the caller's array order.
pub fn evaluate_aggregation(input: &AggregationInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let mut seen = BTreeSet::new();
    let mut ordered: Vec<&DimensionAggregate> = input.dimensions.iter().collect();
    ordered.sort_by(|left, right| dimension_sort_key(left).cmp(&dimension_sort_key(right)));

    for (index, entry) in ordered.iter().enumerate() {
        let aggregate = *entry;
        let evidence = format!("{DIMENSION_EVIDENCE_PREFIX}{index}");
        let actual = dimension_actual(aggregate);
        let mut blocking = false;

        if aggregate.schema != DIMENSION_AGGREGATE_SCHEMA {
            blocking = true;
            emit(
                &mut findings,
                "aggregation.dimension_schema_invalid",
                text(DIMENSION_AGGREGATE_SCHEMA),
                text(&aggregate.schema),
                &evidence,
            )?;
        }

        // A dimension identity must stay a bounded, single-line, secret-free token and must be
        // unique across the envelope; a duplicate is refused instead of being silently merged.
        let bounded = is_bounded_dimension(&aggregate.dimension);
        let duplicate = !seen.insert(aggregate.dimension.clone());
        if !bounded || duplicate {
            blocking = true;
            emit(
                &mut findings,
                "aggregation.dimension_invalid",
                text("bounded_unique_dimension"),
                actual.clone(),
                &evidence,
            )?;
        }

        if aggregate.minimum_samples == 0 {
            blocking = true;
            emit(
                &mut findings,
                "aggregation.minimum_sample_invalid",
                text("positive_minimum_sample"),
                actual.clone(),
                &evidence,
            )?;
        }

        let insufficient = aggregate.sample_count < aggregate.minimum_samples;
        if insufficient {
            blocking = true;
            emit(
                &mut findings,
                "aggregation.insufficient_sample",
                text("sample_count_meets_minimum"),
                actual.clone(),
                &evidence,
            )?;
        }

        // At least one explicit threshold policy is required: a pass verdict without any
        // declared limit would be a vacuous success.
        if aggregate.absolute_limit_milli.is_none() && aggregate.relative_limit_milli.is_none() {
            blocking = true;
            emit(
                &mut findings,
                "aggregation.threshold_missing",
                text("explicit_absolute_or_relative_limit"),
                actual.clone(),
                &evidence,
            )?;
        }

        if let Some(limit) = aggregate.absolute_limit_milli {
            if aggregate.observed_milli > limit {
                blocking = true;
                emit(
                    &mut findings,
                    "aggregation.absolute_threshold_exceeded",
                    text("observed_within_absolute_limit"),
                    actual.clone(),
                    &evidence,
                )?;
            }
        }

        if let Some(limit) = aggregate.relative_limit_milli {
            match aggregate.baseline_milli {
                None => {
                    blocking = true;
                    emit(
                        &mut findings,
                        "aggregation.relative_threshold_invalid",
                        text("baseline_for_relative_check"),
                        actual.clone(),
                        &evidence,
                    )?;
                }
                Some(0) if aggregate.observed_milli > 0 => {
                    blocking = true;
                    emit(
                        &mut findings,
                        "aggregation.relative_threshold_invalid",
                        text("nonzero_baseline_or_zero_observed"),
                        actual.clone(),
                        &evidence,
                    )?;
                }
                Some(baseline) => {
                    let delta = aggregate.observed_milli.abs_diff(baseline);
                    let relative_milli = u128::from(delta) * 1_000 / u128::from(baseline.max(1));
                    if relative_milli > u128::from(limit) {
                        blocking = true;
                        emit(
                            &mut findings,
                            "aggregation.relative_threshold_exceeded",
                            text("relative_delta_within_limit"),
                            actual.clone(),
                            &evidence,
                        )?;
                    }
                }
            }
        }

        // A provided confidence interval must be a complete ordered pair that contains the
        // observed value; when the policy requires confidence the pair must exist at all.
        match (
            aggregate.confidence_low_milli,
            aggregate.confidence_high_milli,
        ) {
            (None, None) => {
                if aggregate.confidence_required {
                    blocking = true;
                    emit(
                        &mut findings,
                        "aggregation.confidence_missing",
                        text("ordered_confidence_interval"),
                        actual.clone(),
                        &evidence,
                    )?;
                }
            }
            (Some(low), Some(high)) => {
                if low > high || aggregate.observed_milli < low || aggregate.observed_milli > high {
                    blocking = true;
                    emit(
                        &mut findings,
                        "aggregation.confidence_invalid",
                        text("observed_inside_ordered_confidence_interval"),
                        actual.clone(),
                        &evidence,
                    )?;
                }
            }
            _ => {
                blocking = true;
                let code = if aggregate.confidence_required {
                    "aggregation.confidence_missing"
                } else {
                    "aggregation.confidence_invalid"
                };
                emit(
                    &mut findings,
                    code,
                    text("complete_confidence_interval_pair"),
                    actual.clone(),
                    &evidence,
                )?;
            }
        }

        if blocking && aggregate.verdict == AggregateVerdict::Pass {
            emit(
                &mut findings,
                "aggregation.false_pass",
                text("blocking_dimension_is_not_pass"),
                actual.clone(),
                VERDICT_EVIDENCE,
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
                text("blocked_or_needs_review"),
                actual,
                VERDICT_EVIDENCE,
            )?;
        }
    }

    Ok(findings)
}

/// Stable full-record ordering key; it never depends on the caller's array order and keeps
/// duplicate records in a deterministic position.
type DimensionSortKey<'a> = (
    &'a str,
    u32,
    u32,
    u64,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    bool,
    u8,
);

fn dimension_sort_key(aggregate: &DimensionAggregate) -> DimensionSortKey<'_> {
    (
        aggregate.dimension.as_str(),
        aggregate.sample_count,
        aggregate.minimum_samples,
        aggregate.observed_milli,
        aggregate.baseline_milli,
        aggregate.absolute_limit_milli,
        aggregate.relative_limit_milli,
        aggregate.confidence_low_milli,
        aggregate.confidence_high_milli,
        aggregate.confidence_required,
        aggregate.verdict.rank(),
    )
}

/// Render one dimension as bounded, caller-safe evidence.  An unbounded or secret-shaped
/// identity is replaced by `invalid` before the value can reach a finding, so the raw identity
/// never escapes through `actual`.
fn dimension_actual(aggregate: &DimensionAggregate) -> Value {
    let bounded = is_bounded_dimension(&aggregate.dimension);
    let dimension = if bounded {
        aggregate.dimension.clone()
    } else {
        "invalid".to_owned()
    };
    serde_json::json!({
        "dimension": dimension,
        "dimension_bounded": bounded,
        "sample_count": aggregate.sample_count,
        "minimum_samples": aggregate.minimum_samples,
        "observed_milli": aggregate.observed_milli,
        "has_baseline": aggregate.baseline_milli.is_some(),
        "has_absolute_limit": aggregate.absolute_limit_milli.is_some(),
        "has_relative_limit": aggregate.relative_limit_milli.is_some(),
        "confidence_required": aggregate.confidence_required,
        "has_confidence_interval": aggregate.confidence_low_milli.is_some()
            && aggregate.confidence_high_milli.is_some(),
        "verdict": aggregate.verdict,
    })
}

/// Bounded, single-line, secret-free identity accepted for a dimension name.
fn is_bounded_dimension(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_AGGREGATION_TEXT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'#' | b'-' | b'@')
        })
        && redact_text(value) == value
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
            format!("aggregation finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
