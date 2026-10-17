//! EQ-34 performance and cost metric evaluation over bounded measurements.
//!
//! Metrics are diagnostic evidence, not a budget authority or a billing ledger.  Every latency,
//! token and tool-call threshold must be explicit and positive, the token and cache counters must
//! be internally consistent, and the cache digest and measurement digest must be strict SHA-256
//! bindings.  Estimated, measured and unknown cost buckets stay separate: a measured amount needs
//! a provider receipt, an estimate needs a rate card and the usage it was derived from, and an
//! unknown bucket is never silently turned into a numeric zero.
//!
//! This is a pure diagnostic over the evidence it is given: it never reads operational metrics,
//! mutates a budget ledger, queries a rate card, contacts a provider, infers billing truth or
//! changes scheduling/authorization.  Cost buckets are ordered by a stable identity key before
//! evaluation, so the finding set does not depend on the caller's array order while duplicate
//! evidence stays visible.  Every caller-supplied value passes through [`Finding::redacted`], and
//! each finding cites the evidence section it was derived from.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::redact_text;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Versioned envelope for one performance/cost evaluation input.
pub const PERFORMANCE_COST_INPUT_SCHEMA: &str = "kiana.quality-performance-cost-input.v1";
/// Stable registry identifier for the performance/cost evaluator.
pub const PERFORMANCE_COST_EVALUATOR_ID: &str = "performance-cost";
const METRICS_SCHEMA: &str = "kiana.quality-performance-metrics.v1";
const THRESHOLDS_SCHEMA: &str = "kiana.quality-performance-thresholds.v1";
const BUCKET_SCHEMA: &str = "kiana.quality-performance-cost-bucket.v1";
/// Maximum number of cost bucket entries accepted in one input.
pub const MAX_COST_BUCKETS: usize = 64;
const MAX_TEXT_BYTES: usize = 512;
const MAX_CACHE_LOOKUPS: u32 = 10_000_000;
const INPUT_EVIDENCE: &str = "metrics:input";
const THRESHOLDS_EVIDENCE: &str = "metrics:thresholds";
const DURATION_EVIDENCE: &str = "metrics:duration";
const TOKEN_EVIDENCE: &str = "metrics:tokens";
const TOOL_EVIDENCE: &str = "metrics:tools";
const CACHE_EVIDENCE: &str = "metrics:cache";
const USAGE_EVIDENCE: &str = "metrics:usage";
const COST_EVIDENCE: &str = "metrics:cost";
const BUCKET_EVIDENCE: &str = "metrics:bucket:";

/// How one cost amount is known.  The three buckets are mutually exclusive and never folded
/// together: an unknown amount carries no number, and an estimate is not a measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostBucket {
    Estimated,
    Measured,
    Unknown,
}

/// Duration, token, tool-call and cache counters observed for one run, bound to a strict
/// measurement digest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceMetrics {
    pub schema: String,
    pub duration_ms: u64,
    /// Whether the duration was measured for this run rather than inferred.
    pub duration_measured: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub tool_calls: u32,
    pub cache_lookups: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
    /// Strict `sha256:` digest binding the cache lookup key.  Empty only without cache use.
    pub cache_key_digest: String,
    pub usage_complete: bool,
    /// Strict `sha256:` digest binding the measurement evidence.
    pub measurement_digest: String,
}

/// Explicit latency, token, tool-call and cost thresholds.  Zero means "not declared"; a missing
/// threshold is a finding rather than a pass.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceThresholds {
    pub schema: String,
    pub max_duration_ms: u64,
    pub max_total_tokens: u64,
    pub max_tool_calls: u32,
    pub max_cost_micros: u64,
    /// Whether the declared cost limit was enforced for this run.
    pub cost_limit_enforced: bool,
    /// Whether a measured (not estimated) cost bucket is required.
    pub require_measured_cost: bool,
}

/// One cost bucket with the provenance its kind requires.
///
/// `unknown` carries no numeric amount (`cost_micros` is zero and `currency` is `UNK`) and must
/// explain itself through `unknown_reason`; `estimated` and `measured` must bind a bounded rate
/// card and the strict usage digest the amount was derived from, and `measured` additionally needs
/// the provider receipt that reported it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostBucketEvidence {
    pub schema: String,
    pub bucket: CostBucket,
    pub cost_micros: u64,
    pub currency: String,
    pub rate_card_ref: String,
    pub receipt_ref: String,
    pub usage_digest: String,
    pub unknown_reason: String,
}

/// Versioned input envelope for [`PerformanceCostEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceCostInput {
    pub schema: String,
    pub metrics: PerformanceMetrics,
    pub thresholds: PerformanceThresholds,
    #[serde(default)]
    pub cost_buckets: Vec<CostBucketEvidence>,
}

impl PerformanceCostInput {
    pub fn new(
        metrics: PerformanceMetrics,
        thresholds: PerformanceThresholds,
        cost_buckets: Vec<CostBucketEvidence>,
    ) -> Self {
        Self {
            schema: PERFORMANCE_COST_INPUT_SCHEMA.to_owned(),
            metrics,
            thresholds,
            cost_buckets,
        }
    }

    /// Hard input bounds.  Structural drift and an oversized bucket set fail closed before any
    /// finding is produced; entry shape problems remain findings so the report explains *why* the
    /// evidence is unusable.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PERFORMANCE_COST_INPUT_SCHEMA {
            return Err("performance_cost_input_schema_invalid");
        }
        if self.cost_buckets.len() > MAX_COST_BUCKETS {
            return Err("performance_cost_cost_bucket_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("performance cost input is serializable")
    }
}

/// Pure performance/cost evaluator.  All authority-bearing actions remain outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct PerformanceCostEvaluator;

impl DeterministicEvaluator for PerformanceCostEvaluator {
    fn evaluator_id(&self) -> &str {
        PERFORMANCE_COST_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: PerformanceCostInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("performance_cost"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_performance_cost(&decoded)
    }
}

/// Evaluate the deny-first performance/cost matrix without reading any operational metric.
///
/// Check order is deliberate: every section schema, the measurement provenance, the explicit
/// thresholds, duration, token accounting, tool calls, cache accounting, and finally the cost
/// buckets with their per-kind provenance and the aggregate limit/measured/unknown rules.  Cost
/// buckets are ordered by a stable identity key before evaluation so the finding set does not
/// depend on the caller's array order.
pub fn evaluate_performance_cost(
    input: &PerformanceCostInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let metrics = &input.metrics;
    let thresholds = &input.thresholds;

    check_schema(
        &mut findings,
        "metrics.metrics_schema_invalid",
        &metrics.schema,
        METRICS_SCHEMA,
        INPUT_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "metrics.threshold_schema_invalid",
        &thresholds.schema,
        THRESHOLDS_SCHEMA,
        THRESHOLDS_EVIDENCE,
    )?;

    // Measurement provenance: the counters are only evidence when bound to a strict digest.
    if !is_sha256_digest(&metrics.measurement_digest) {
        emit(
            &mut findings,
            "metrics.measurement_provenance_invalid",
            text("sha256_measurement_digest"),
            metrics_actual(metrics),
            INPUT_EVIDENCE,
        )?;
    }

    // Explicit thresholds.  A zero latency/token/tool-call bound is not a declaration, and a
    // required measured cost needs a positive cost limit to check against.
    let explicit = thresholds.max_duration_ms > 0
        && thresholds.max_total_tokens > 0
        && thresholds.max_tool_calls > 0;
    if !explicit || (thresholds.require_measured_cost && thresholds.max_cost_micros == 0) {
        emit(
            &mut findings,
            "metrics.threshold_missing",
            text("explicit_positive_thresholds"),
            thresholds_actual(thresholds),
            THRESHOLDS_EVIDENCE,
        )?;
    }
    if thresholds.cost_limit_enforced && thresholds.max_cost_micros == 0 {
        emit(
            &mut findings,
            "metrics.cost_limit_invalid",
            text("positive_cost_limit_when_enforced"),
            thresholds_actual(thresholds),
            THRESHOLDS_EVIDENCE,
        )?;
    }

    // Duration: an inferred duration is not latency evidence, and an overage is blocking.
    if !metrics.duration_measured {
        emit(
            &mut findings,
            "metrics.duration_unmeasured",
            text("measured_duration"),
            duration_actual(metrics),
            DURATION_EVIDENCE,
        )?;
    }
    if thresholds.max_duration_ms > 0 && metrics.duration_ms > thresholds.max_duration_ms {
        emit(
            &mut findings,
            "metrics.duration_exceeded",
            text(&thresholds.max_duration_ms.to_string()),
            duration_actual(metrics),
            DURATION_EVIDENCE,
        )?;
    }

    // Token accounting: complete usage and an exact input+output total without overflow.
    if !metrics.usage_complete {
        emit(
            &mut findings,
            "metrics.usage_incomplete",
            text("complete_usage"),
            token_actual(metrics),
            USAGE_EVIDENCE,
        )?;
    }
    match metrics.input_tokens.checked_add(metrics.output_tokens) {
        Some(sum) if sum == metrics.total_tokens => {}
        _ => emit(
            &mut findings,
            "metrics.token_total_invalid",
            text("input_plus_output_equals_total"),
            token_actual(metrics),
            TOKEN_EVIDENCE,
        )?,
    }
    if thresholds.max_total_tokens > 0 && metrics.total_tokens > thresholds.max_total_tokens {
        emit(
            &mut findings,
            "metrics.tokens_exceeded",
            text(&thresholds.max_total_tokens.to_string()),
            token_actual(metrics),
            TOKEN_EVIDENCE,
        )?;
    }

    // Tool calls.
    if thresholds.max_tool_calls > 0 && metrics.tool_calls > thresholds.max_tool_calls {
        emit(
            &mut findings,
            "metrics.tool_calls_exceeded",
            text(&thresholds.max_tool_calls.to_string()),
            serde_json::json!({"tool_calls": metrics.tool_calls}),
            TOOL_EVIDENCE,
        )?;
    }

    // Cache accounting: bounded counters, hits+misses equal to the observed lookups, a strict key
    // digest whenever the cache was used, and no key digest without cache activity.
    if metrics.cache_lookups > MAX_CACHE_LOOKUPS {
        emit(
            &mut findings,
            "metrics.cache_bounds_exceeded",
            text(&MAX_CACHE_LOOKUPS.to_string()),
            cache_actual(metrics),
            CACHE_EVIDENCE,
        )?;
    }
    if metrics.cache_hits.checked_add(metrics.cache_misses) != Some(metrics.cache_lookups) {
        emit(
            &mut findings,
            "metrics.cache_count_mismatch",
            text("hits_plus_misses_equals_lookups"),
            cache_actual(metrics),
            CACHE_EVIDENCE,
        )?;
    }
    let cache_used =
        metrics.cache_lookups > 0 || metrics.cache_hits > 0 || metrics.cache_misses > 0;
    if cache_used && !is_sha256_digest(&metrics.cache_key_digest) {
        emit(
            &mut findings,
            "metrics.cache_key_missing",
            text("cache_usage_has_key_digest"),
            cache_actual(metrics),
            CACHE_EVIDENCE,
        )?;
    }
    if !cache_used && !metrics.cache_key_digest.is_empty() {
        emit(
            &mut findings,
            "metrics.cache_key_unexpected",
            text("no_cache_key_digest_without_cache_usage"),
            cache_actual(metrics),
            CACHE_EVIDENCE,
        )?;
    }

    // Cost buckets: per-kind provenance first, then the aggregate limit/measured/unknown rules.
    let mut buckets: Vec<&CostBucketEvidence> = input.cost_buckets.iter().collect();
    buckets.sort_by(|left, right| bucket_sort_key(*left).cmp(&bucket_sort_key(*right)));

    let mut seen_kinds: Vec<CostBucket> = Vec::new();
    let mut known_currencies = BTreeSet::new();
    let mut known_total = 0_u64;
    let mut has_measured = false;
    let mut has_unknown = false;
    for (index, bucket) in buckets.iter().copied().enumerate() {
        let bucket_ref = format!("{BUCKET_EVIDENCE}{index}");
        check_schema(
            &mut findings,
            "metrics.bucket_schema_invalid",
            &bucket.schema,
            BUCKET_SCHEMA,
            &bucket_ref,
        )?;
        if seen_kinds.contains(&bucket.bucket) {
            emit(
                &mut findings,
                "metrics.bucket_duplicate",
                text("one_bucket_per_kind"),
                bucket_actual(bucket),
                &bucket_ref,
            )?;
        } else {
            seen_kinds.push(bucket.bucket);
        }
        let known = matches!(bucket.bucket, CostBucket::Estimated | CostBucket::Measured);
        if !is_currency_code(&bucket.currency) {
            emit(
                &mut findings,
                "metrics.bucket_currency_invalid",
                text("three_letter_currency_code"),
                bucket_actual(bucket),
                &bucket_ref,
            )?;
        } else if known && bucket.currency == "UNK" {
            emit(
                &mut findings,
                "metrics.bucket_currency_invalid",
                text("known_currency_for_a_known_cost"),
                bucket_actual(bucket),
                &bucket_ref,
            )?;
        } else if !known && bucket.currency != "UNK" {
            emit(
                &mut findings,
                "metrics.bucket_currency_invalid",
                text("UNK_currency_for_an_unknown_cost"),
                bucket_actual(bucket),
                &bucket_ref,
            )?;
        }
        if !known && bucket.cost_micros != 0 {
            emit(
                &mut findings,
                "metrics.bucket_amount_invalid",
                text("no_numeric_amount_for_an_unknown_cost"),
                bucket_actual(bucket),
                &bucket_ref,
            )?;
        }
        match bucket.bucket {
            CostBucket::Estimated => {
                if !is_bounded_reference(&bucket.rate_card_ref)
                    || !is_sha256_digest(&bucket.usage_digest)
                {
                    emit(
                        &mut findings,
                        "metrics.bucket_estimate_unbound",
                        text("bounded_rate_card_and_sha256_usage_digest"),
                        bucket_actual(bucket),
                        &bucket_ref,
                    )?;
                }
                if !bucket.unknown_reason.is_empty() {
                    emit(
                        &mut findings,
                        "metrics.bucket_reason_unexpected",
                        text("no_unknown_reason_for_a_known_cost"),
                        bucket_actual(bucket),
                        &bucket_ref,
                    )?;
                }
            }
            CostBucket::Measured => {
                if !is_bounded_reference(&bucket.rate_card_ref)
                    || !is_sha256_digest(&bucket.usage_digest)
                    || !is_bounded_reference(&bucket.receipt_ref)
                {
                    emit(
                        &mut findings,
                        "metrics.bucket_measured_unbound",
                        text("bounded_receipt_rate_card_and_sha256_usage_digest"),
                        bucket_actual(bucket),
                        &bucket_ref,
                    )?;
                }
                if !bucket.unknown_reason.is_empty() {
                    emit(
                        &mut findings,
                        "metrics.bucket_reason_unexpected",
                        text("no_unknown_reason_for_a_known_cost"),
                        bucket_actual(bucket),
                        &bucket_ref,
                    )?;
                }
            }
            CostBucket::Unknown => {
                if !is_bounded_reference(&bucket.unknown_reason) {
                    emit(
                        &mut findings,
                        "metrics.bucket_unknown_unbound",
                        text("bounded_unknown_reason"),
                        bucket_actual(bucket),
                        &bucket_ref,
                    )?;
                }
                if !bucket.rate_card_ref.is_empty()
                    || !bucket.receipt_ref.is_empty()
                    || !bucket.usage_digest.is_empty()
                {
                    emit(
                        &mut findings,
                        "metrics.bucket_unknown_unbound",
                        text("no_binding_evidence_for_an_unknown_cost"),
                        bucket_actual(bucket),
                        &bucket_ref,
                    )?;
                }
            }
        }

        match bucket.bucket {
            CostBucket::Measured => {
                has_measured = true;
                known_currencies.insert(bucket.currency.as_str());
                known_total = known_total.saturating_add(bucket.cost_micros);
            }
            CostBucket::Estimated => {
                known_currencies.insert(bucket.currency.as_str());
                known_total = known_total.saturating_add(bucket.cost_micros);
            }
            CostBucket::Unknown => has_unknown = true,
        }
    }

    if thresholds.require_measured_cost && !has_measured {
        emit(
            &mut findings,
            "metrics.cost_unmeasured",
            text("measured_cost_bucket"),
            cost_summary(&buckets, known_total, has_unknown),
            COST_EVIDENCE,
        )?;
    }
    if has_unknown {
        emit(
            &mut findings,
            "metrics.cost_unknown",
            text("known_estimated_or_measured_cost"),
            cost_summary(&buckets, known_total, has_unknown),
            COST_EVIDENCE,
        )?;
    }
    if known_currencies.len() > 1 {
        emit(
            &mut findings,
            "metrics.bucket_currency_mismatch",
            text("single_currency_per_cost_report"),
            cost_summary(&buckets, known_total, has_unknown),
            COST_EVIDENCE,
        )?;
    }
    if known_currencies.len() == 1 {
        if !thresholds.cost_limit_enforced {
            emit(
                &mut findings,
                "metrics.cost_unenforced",
                text("enforced_cost_limit"),
                cost_summary(&buckets, known_total, has_unknown),
                COST_EVIDENCE,
            )?;
        } else if thresholds.max_cost_micros > 0 && known_total > thresholds.max_cost_micros {
            emit(
                &mut findings,
                "metrics.cost_exceeded",
                text(&thresholds.max_cost_micros.to_string()),
                cost_summary(&buckets, known_total, has_unknown),
                COST_EVIDENCE,
            )?;
        }
    }

    Ok(findings)
}

fn bucket_sort_key(bucket: &CostBucketEvidence) -> (u8, &str, u64, &str, &str, &str, &str) {
    (
        match bucket.bucket {
            CostBucket::Estimated => 0,
            CostBucket::Measured => 1,
            CostBucket::Unknown => 2,
        },
        bucket.currency.as_str(),
        bucket.cost_micros,
        bucket.rate_card_ref.as_str(),
        bucket.receipt_ref.as_str(),
        bucket.usage_digest.as_str(),
        bucket.unknown_reason.as_str(),
    )
}

fn metrics_actual(metrics: &PerformanceMetrics) -> Value {
    serde_json::json!({
        "duration_ms": metrics.duration_ms,
        "duration_measured": metrics.duration_measured,
        "input_tokens": metrics.input_tokens,
        "output_tokens": metrics.output_tokens,
        "total_tokens": metrics.total_tokens,
        "tool_calls": metrics.tool_calls,
        "cache_lookups": metrics.cache_lookups,
        "cache_hits": metrics.cache_hits,
        "cache_misses": metrics.cache_misses,
        "cache_key_bound": is_sha256_digest(&metrics.cache_key_digest),
        "usage_complete": metrics.usage_complete,
        "measurement_digest_bound": is_sha256_digest(&metrics.measurement_digest),
    })
}

fn thresholds_actual(thresholds: &PerformanceThresholds) -> Value {
    serde_json::json!({
        "max_duration_ms": thresholds.max_duration_ms,
        "max_total_tokens": thresholds.max_total_tokens,
        "max_tool_calls": thresholds.max_tool_calls,
        "max_cost_micros": thresholds.max_cost_micros,
        "cost_limit_enforced": thresholds.cost_limit_enforced,
        "require_measured_cost": thresholds.require_measured_cost,
    })
}

fn duration_actual(metrics: &PerformanceMetrics) -> Value {
    serde_json::json!({
        "duration_ms": metrics.duration_ms,
        "duration_measured": metrics.duration_measured,
    })
}

fn token_actual(metrics: &PerformanceMetrics) -> Value {
    // Keys stay inside the shared redaction allowlist for token metrics so the observed numbers
    // survive `Finding::redacted` instead of being replaced by the secret placeholder.
    serde_json::json!({
        "input_tokens": metrics.input_tokens,
        "output_tokens": metrics.output_tokens,
        "total_tokens": metrics.total_tokens,
        "usage_complete": metrics.usage_complete,
    })
}

fn cache_actual(metrics: &PerformanceMetrics) -> Value {
    serde_json::json!({
        "cache_lookups": metrics.cache_lookups,
        "cache_hits": metrics.cache_hits,
        "cache_misses": metrics.cache_misses,
        "cache_key_bound": is_sha256_digest(&metrics.cache_key_digest),
    })
}

fn bucket_actual(bucket: &CostBucketEvidence) -> Value {
    serde_json::json!({
        "bucket": bucket.bucket,
        "cost_micros": bucket.cost_micros,
        "currency": bucket.currency,
        "rate_card_ref": bucket.rate_card_ref,
        "receipt_ref": bucket.receipt_ref,
        "usage_digest": bucket.usage_digest,
        "unknown_reason": bucket.unknown_reason,
    })
}

fn cost_summary(
    buckets: &[&CostBucketEvidence],
    known_total_micros: u64,
    has_unknown: bool,
) -> Value {
    serde_json::json!({
        "bucket_count": buckets.len(),
        "known_total_micros": known_total_micros,
        "unknown_present": has_unknown,
    })
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

/// Bounded, single-line, secret-free reference accepted in rate-card and receipt identities.
fn is_bounded_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'#' | b'-' | b'@')
        })
        && redact_text(value) == value
}

fn is_currency_code(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// Push one finding.  Caller-supplied values go through [`Finding::redacted`] so a secret-shaped
/// reference cannot escape through the finding itself.
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
            format!("performance and cost finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
