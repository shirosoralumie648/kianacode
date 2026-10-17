use kiana_quality::{
    canonical_findings, digest_findings, evaluate_performance_cost, CostBucket, CostBucketEvidence,
    DeterministicEvaluator, EvaluatorRegistry, Finding, PerformanceCostEvaluator,
    PerformanceCostInput, PerformanceMetrics, PerformanceThresholds, EVALUATION_RESULT_SCHEMA,
    MAX_COST_BUCKETS, PERFORMANCE_COST_EVALUATOR_ID, PERFORMANCE_COST_INPUT_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const METRICS_SCHEMA: &str = "kiana.quality-performance-metrics.v1";
const THRESHOLDS_SCHEMA: &str = "kiana.quality-performance-thresholds.v1";
const BUCKET_SCHEMA: &str = "kiana.quality-performance-cost-bucket.v1";
const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn measured_bucket(cost_micros: u64) -> Value {
    json!({
        "schema": BUCKET_SCHEMA,
        "bucket": "measured",
        "cost_micros": cost_micros,
        "currency": "USD",
        "rate_card_ref": "rate-card/2026-10",
        "receipt_ref": "receipt/run-2026-10",
        "usage_digest": DIGEST,
        "unknown_reason": "",
    })
}

fn estimated_bucket(cost_micros: u64) -> Value {
    json!({
        "schema": BUCKET_SCHEMA,
        "bucket": "estimated",
        "cost_micros": cost_micros,
        "currency": "USD",
        "rate_card_ref": "rate-card/2026-10",
        "receipt_ref": "",
        "usage_digest": DIGEST,
        "unknown_reason": "",
    })
}

fn unknown_bucket(reason: &str) -> Value {
    json!({
        "schema": BUCKET_SCHEMA,
        "bucket": "unknown",
        "cost_micros": 0,
        "currency": "UNK",
        "rate_card_ref": "",
        "receipt_ref": "",
        "usage_digest": "",
        "unknown_reason": reason,
    })
}

fn metrics() -> Value {
    json!({
        "schema": METRICS_SCHEMA,
        "duration_ms": 120,
        "duration_measured": true,
        "input_tokens": 40,
        "output_tokens": 30,
        "total_tokens": 70,
        "tool_calls": 2,
        "cache_lookups": 4,
        "cache_hits": 3,
        "cache_misses": 1,
        "cache_key_digest": DIGEST,
        "usage_complete": true,
        "measurement_digest": DIGEST,
    })
}

fn thresholds() -> Value {
    json!({
        "schema": THRESHOLDS_SCHEMA,
        "max_duration_ms": 200,
        "max_total_tokens": 100,
        "max_tool_calls": 3,
        "max_cost_micros": 1000,
        "cost_limit_enforced": true,
        "require_measured_cost": true,
    })
}

fn valid_input() -> Value {
    json!({
        "schema": PERFORMANCE_COST_INPUT_SCHEMA,
        "metrics": metrics(),
        "thresholds": thresholds(),
        "cost_buckets": [measured_bucket(900)],
    })
}

fn evaluate(input: &Value) -> Vec<Finding> {
    PerformanceCostEvaluator
        .evaluate(input)
        .expect("performance fixture decodes")
}

fn codes(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect()
}

fn set(codes: &[&str]) -> BTreeSet<String> {
    codes.iter().map(|code| (*code).to_owned()).collect()
}

#[test]
fn budget_and_latency_thresholds_are_explicit() {
    // Explicit positive thresholds inside the declared budgets are a pass.
    assert!(evaluate(&valid_input()).is_empty());

    // A zero threshold is a missing declaration, never an unbounded budget.
    let mut zeroed = valid_input();
    zeroed["thresholds"]["max_duration_ms"] = json!(0);
    zeroed["thresholds"]["max_total_tokens"] = json!(0);
    zeroed["thresholds"]["max_tool_calls"] = json!(0);
    let findings = evaluate(&zeroed);
    assert_eq!(codes(&findings), set(&["metrics.threshold_missing"]));
    assert_eq!(findings[0].expected, json!("explicit_positive_thresholds"));
    assert_eq!(findings[0].evidence_ref, "metrics:thresholds");

    // A required measured cost without a positive limit is not an explicit budget either.
    let mut unbounded_cost = valid_input();
    unbounded_cost["thresholds"]["max_cost_micros"] = json!(0);
    assert_eq!(
        codes(&evaluate(&unbounded_cost)),
        set(&["metrics.threshold_missing", "metrics.cost_limit_invalid"])
    );
}

#[test]
fn performance_evidence_within_explicit_budgets_has_no_findings() {
    assert!(evaluate(&valid_input()).is_empty());

    // An estimate is acceptable when no measured cost is required.
    let mut estimated_only = valid_input();
    estimated_only["thresholds"]["require_measured_cost"] = json!(false);
    estimated_only["cost_buckets"] = json!([estimated_bucket(900)]);
    assert!(evaluate(&estimated_only).is_empty());
}

#[test]
fn unmeasured_duration_is_not_latency_evidence() {
    let mut input = valid_input();
    input["metrics"]["duration_measured"] = json!(false);
    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["metrics.duration_unmeasured"]));
    assert_eq!(findings[0].evidence_ref, "metrics:duration");
}

#[test]
fn duration_token_and_tool_overages_are_blocking() {
    let mut input = valid_input();
    input["metrics"]["duration_ms"] = json!(201);
    input["metrics"]["input_tokens"] = json!(60);
    input["metrics"]["output_tokens"] = json!(60);
    input["metrics"]["total_tokens"] = json!(120);
    input["metrics"]["tool_calls"] = json!(4);
    let findings = evaluate(&input);
    assert_eq!(
        codes(&findings),
        set(&[
            "metrics.duration_exceeded",
            "metrics.tokens_exceeded",
            "metrics.tool_calls_exceeded",
        ])
    );
}

#[test]
fn token_total_must_equal_input_plus_output_without_overflow() {
    let mut mismatch = valid_input();
    mismatch["metrics"]["total_tokens"] = json!(71);
    assert_eq!(
        codes(&evaluate(&mismatch)),
        set(&["metrics.token_total_invalid"])
    );

    let mut overflow = valid_input();
    overflow["metrics"]["input_tokens"] = json!(u64::MAX);
    overflow["metrics"]["output_tokens"] = json!(1);
    overflow["metrics"]["total_tokens"] = json!(0);
    assert_eq!(
        codes(&evaluate(&overflow)),
        set(&["metrics.token_total_invalid"])
    );
}

#[test]
fn incomplete_usage_is_never_a_pass() {
    let mut input = valid_input();
    input["metrics"]["usage_complete"] = json!(false);
    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["metrics.usage_incomplete"]));
    assert_eq!(findings[0].evidence_ref, "metrics:usage");
}

#[test]
fn cache_usage_requires_a_strict_key_digest() {
    let mut missing = valid_input();
    missing["metrics"]["cache_key_digest"] = json!("");
    assert_eq!(
        codes(&evaluate(&missing)),
        set(&["metrics.cache_key_missing"])
    );

    let mut sloppy = valid_input();
    sloppy["metrics"]["cache_key_digest"] = json!("sha256:zz");
    assert_eq!(
        codes(&evaluate(&sloppy)),
        set(&["metrics.cache_key_missing"])
    );
}

#[test]
fn cache_key_without_usage_is_unexpected() {
    let mut input = valid_input();
    input["metrics"]["cache_lookups"] = json!(0);
    input["metrics"]["cache_hits"] = json!(0);
    input["metrics"]["cache_misses"] = json!(0);
    assert_eq!(
        codes(&evaluate(&input)),
        set(&["metrics.cache_key_unexpected"])
    );
}

#[test]
fn cache_counts_must_be_bounded_and_consistent() {
    let mut mismatch = valid_input();
    mismatch["metrics"]["cache_misses"] = json!(2);
    assert_eq!(
        codes(&evaluate(&mismatch)),
        set(&["metrics.cache_count_mismatch"])
    );

    let mut hits_beyond_lookups = valid_input();
    hits_beyond_lookups["metrics"]["cache_lookups"] = json!(1);
    hits_beyond_lookups["metrics"]["cache_hits"] = json!(2);
    hits_beyond_lookups["metrics"]["cache_misses"] = json!(0);
    assert_eq!(
        codes(&evaluate(&hits_beyond_lookups)),
        set(&["metrics.cache_count_mismatch"])
    );

    let mut bounded = valid_input();
    bounded["metrics"]["cache_lookups"] = json!(10_000_001);
    bounded["metrics"]["cache_hits"] = json!(10_000_000);
    bounded["metrics"]["cache_misses"] = json!(1);
    assert_eq!(
        codes(&evaluate(&bounded)),
        set(&["metrics.cache_bounds_exceeded"])
    );
}

#[test]
fn measurement_digest_must_be_a_strict_sha256() {
    let mut input = valid_input();
    input["metrics"]["measurement_digest"] = json!("sha256:not-a-digest");
    let findings = evaluate(&input);
    assert_eq!(
        codes(&findings),
        set(&["metrics.measurement_provenance_invalid"])
    );
    assert_eq!(findings[0].evidence_ref, "metrics:input");
}

#[test]
fn measured_cost_requires_a_receipt_rate_card_and_usage_digest() {
    let mut missing_receipt = valid_input();
    missing_receipt["cost_buckets"][0]["receipt_ref"] = json!("");
    assert_eq!(
        codes(&evaluate(&missing_receipt)),
        set(&["metrics.bucket_measured_unbound"])
    );

    let mut rejected_receipt = valid_input();
    rejected_receipt["cost_buckets"][0]["receipt_ref"] = json!("receipt token=supersecret");
    let findings = evaluate(&rejected_receipt);
    assert_eq!(codes(&findings), set(&["metrics.bucket_measured_unbound"]));
    assert!(!serde_json::to_string(&findings)
        .unwrap()
        .contains("supersecret"));

    let mut bad_usage = valid_input();
    bad_usage["cost_buckets"][0]["usage_digest"] = json!("sha256:short");
    assert_eq!(
        codes(&evaluate(&bad_usage)),
        set(&["metrics.bucket_measured_unbound"])
    );

    let mut bad_rate_card = valid_input();
    bad_rate_card["cost_buckets"][0]["rate_card_ref"] = json!("");
    let findings = evaluate(&bad_rate_card);
    assert_eq!(codes(&findings), set(&["metrics.bucket_measured_unbound"]));
    assert_eq!(findings[0].evidence_ref, "metrics:bucket:0");
}

#[test]
fn estimated_cost_requires_a_rate_card_and_usage_digest() {
    let mut missing_rate_card = valid_input();
    missing_rate_card["thresholds"]["require_measured_cost"] = json!(false);
    missing_rate_card["cost_buckets"] = json!([estimated_bucket(500)]);
    missing_rate_card["cost_buckets"][0]["rate_card_ref"] = json!("");
    assert_eq!(
        codes(&evaluate(&missing_rate_card)),
        set(&["metrics.bucket_estimate_unbound"])
    );

    let mut bad_usage = valid_input();
    bad_usage["thresholds"]["require_measured_cost"] = json!(false);
    bad_usage["cost_buckets"] = json!([estimated_bucket(500)]);
    bad_usage["cost_buckets"][0]["usage_digest"] = json!("not-a-digest");
    assert_eq!(
        codes(&evaluate(&bad_usage)),
        set(&["metrics.bucket_estimate_unbound"])
    );

    let mut reason = valid_input();
    reason["thresholds"]["require_measured_cost"] = json!(false);
    reason["cost_buckets"] = json!([estimated_bucket(500)]);
    reason["cost_buckets"][0]["unknown_reason"] = json!("provider silent");
    assert_eq!(
        codes(&evaluate(&reason)),
        set(&["metrics.bucket_reason_unexpected"])
    );
}

#[test]
fn unknown_cost_is_never_a_numeric_zero() {
    let mut unknown_only = valid_input();
    unknown_only["thresholds"]["require_measured_cost"] = json!(false);
    unknown_only["cost_buckets"] = json!([unknown_bucket("rate-card-missing")]);
    assert_eq!(
        codes(&evaluate(&unknown_only)),
        set(&["metrics.cost_unknown"])
    );

    let mut with_amount = valid_input();
    with_amount["thresholds"]["require_measured_cost"] = json!(false);
    with_amount["cost_buckets"] = json!([unknown_bucket("rate-card-missing")]);
    with_amount["cost_buckets"][0]["cost_micros"] = json!(1);
    with_amount["cost_buckets"][0]["currency"] = json!("USD");
    assert_eq!(
        codes(&evaluate(&with_amount)),
        set(&[
            "metrics.bucket_amount_invalid",
            "metrics.bucket_currency_invalid",
            "metrics.cost_unknown",
        ])
    );

    let mut unbound = valid_input();
    unbound["thresholds"]["require_measured_cost"] = json!(false);
    unbound["cost_buckets"] = json!([unknown_bucket("")]);
    assert_eq!(
        codes(&evaluate(&unbound)),
        set(&["metrics.bucket_unknown_unbound", "metrics.cost_unknown"])
    );
}

#[test]
fn cost_buckets_must_not_mix_currencies() {
    let mut input = valid_input();
    input["thresholds"]["require_measured_cost"] = json!(false);
    input["cost_buckets"] = json!([estimated_bucket(400), measured_bucket(400)]);
    input["cost_buckets"][0]["currency"] = json!("EUR");
    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["metrics.bucket_currency_mismatch"]));
    assert_eq!(findings[0].evidence_ref, "metrics:cost");
}

#[test]
fn enforced_cost_limit_blocks_known_bucket_overage() {
    let mut input = valid_input();
    input["cost_buckets"] = json!([measured_bucket(1200)]);
    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["metrics.cost_exceeded"]));
    assert_eq!(findings[0].expected, json!("1000"));
    assert_eq!(findings[0].evidence_ref, "metrics:cost");

    // A saturating split across buckets stays blocking.
    let mut split = valid_input();
    split["cost_buckets"] = json!([measured_bucket(u64::MAX), estimated_bucket(10)]);
    assert_eq!(codes(&evaluate(&split)), set(&["metrics.cost_exceeded"]));
}

#[test]
fn cost_limit_must_be_explicitly_enforced() {
    let mut unenforced = valid_input();
    unenforced["thresholds"]["cost_limit_enforced"] = json!(false);
    assert_eq!(
        codes(&evaluate(&unenforced)),
        set(&["metrics.cost_unenforced"])
    );

    let mut invalid_limit = valid_input();
    invalid_limit["thresholds"]["require_measured_cost"] = json!(false);
    invalid_limit["thresholds"]["max_cost_micros"] = json!(0);
    assert_eq!(
        codes(&evaluate(&invalid_limit)),
        set(&["metrics.cost_limit_invalid"])
    );
}

#[test]
fn measured_cost_requirement_rejects_estimates_and_unknown() {
    let mut estimated = valid_input();
    estimated["cost_buckets"] = json!([estimated_bucket(900)]);
    assert_eq!(
        codes(&evaluate(&estimated)),
        set(&["metrics.cost_unmeasured"])
    );

    let mut unknown = valid_input();
    unknown["cost_buckets"] = json!([unknown_bucket("rate-card-missing")]);
    assert_eq!(
        codes(&evaluate(&unknown)),
        set(&["metrics.cost_unmeasured", "metrics.cost_unknown"])
    );

    let mut none = valid_input();
    none["cost_buckets"] = json!([]);
    assert_eq!(codes(&evaluate(&none)), set(&["metrics.cost_unmeasured"]));
}

#[test]
fn duplicate_cost_buckets_are_refused() {
    let mut input = valid_input();
    input["cost_buckets"] = json!([measured_bucket(400), measured_bucket(500)]);
    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["metrics.bucket_duplicate"]));
    assert_eq!(findings.len(), 1, "duplicate evidence stays visible once");
    assert_eq!(findings[0].evidence_ref, "metrics:bucket:1");
}

#[test]
fn bucket_schema_drift_on_every_section_is_rejected() {
    let mut metrics_drift = valid_input();
    metrics_drift["metrics"]["schema"] = json!("kiana.quality-performance-metrics.v0");
    assert_eq!(
        codes(&evaluate(&metrics_drift)),
        set(&["metrics.metrics_schema_invalid"])
    );

    let mut threshold_drift = valid_input();
    threshold_drift["thresholds"]["schema"] = json!("kiana.quality-performance-thresholds.v0");
    assert_eq!(
        codes(&evaluate(&threshold_drift)),
        set(&["metrics.threshold_schema_invalid"])
    );

    let mut bucket_drift = valid_input();
    bucket_drift["cost_buckets"][0]["schema"] = json!("kiana.quality-performance-cost-bucket.v0");
    let findings = evaluate(&bucket_drift);
    assert_eq!(codes(&findings), set(&["metrics.bucket_schema_invalid"]));
    assert_eq!(findings[0].evidence_ref, "metrics:bucket:0");
}

#[test]
fn unknown_fields_and_envelope_drift_fail_closed() {
    let mut extra = valid_input();
    extra["unexpected"] = json!(true);
    assert!(PerformanceCostEvaluator.evaluate(&extra).is_err());

    let mut metrics_extra = valid_input();
    metrics_extra["metrics"]["unexpected"] = json!(true);
    assert!(PerformanceCostEvaluator.evaluate(&metrics_extra).is_err());

    let mut bucket_extra = valid_input();
    bucket_extra["cost_buckets"][0]["unexpected"] = json!(true);
    assert!(PerformanceCostEvaluator.evaluate(&bucket_extra).is_err());

    let decoded: PerformanceCostInput = serde_json::from_value(valid_input()).unwrap();
    assert_eq!(decoded.validate(), Ok(()));

    let mut drift = valid_input();
    drift["schema"] = json!("kiana.quality-performance-cost-input.v0");
    let decoded: PerformanceCostInput = serde_json::from_value(drift).unwrap();
    assert_eq!(
        decoded.validate(),
        Err("performance_cost_input_schema_invalid")
    );
}

#[test]
fn bounded_bucket_references_fail_closed() {
    let mut too_long = valid_input();
    too_long["cost_buckets"][0]["rate_card_ref"] = json!("x".repeat(513));
    assert_eq!(
        codes(&evaluate(&too_long)),
        set(&["metrics.bucket_measured_unbound"])
    );

    let mut injected = valid_input();
    injected["cost_buckets"][0]["rate_card_ref"] = json!("rate-card\ninjected");
    assert_eq!(
        codes(&evaluate(&injected)),
        set(&["metrics.bucket_measured_unbound"])
    );

    let mut secret = valid_input();
    secret["cost_buckets"][0]["rate_card_ref"] = json!("token=supersecret");
    let findings = evaluate(&secret);
    assert_eq!(codes(&findings), set(&["metrics.bucket_measured_unbound"]));
    assert!(!serde_json::to_string(&findings)
        .unwrap()
        .contains("supersecret"));
}

#[test]
fn oversized_bucket_sets_fail_closed_before_evaluation() {
    let buckets: Vec<Value> = (0..=MAX_COST_BUCKETS)
        .map(|index| unknown_bucket(&format!("reason-{index}")))
        .collect();
    let mut too_many = valid_input();
    too_many["cost_buckets"] = json!(buckets);
    let decoded: PerformanceCostInput =
        serde_json::from_value(too_many).expect("decodes structurally");
    assert_eq!(
        decoded.validate(),
        Err("performance_cost_cost_bucket_limit_exceeded")
    );

    let bounded: Vec<Value> = (0..MAX_COST_BUCKETS)
        .map(|index| unknown_bucket(&format!("reason-{index}")))
        .collect();
    let mut exactly = valid_input();
    exactly["cost_buckets"] = json!(bounded);
    let decoded: PerformanceCostInput =
        serde_json::from_value(exactly).expect("decodes structurally");
    assert!(decoded.validate().is_ok());
}

#[test]
fn bucket_order_does_not_change_the_findings() {
    let mut forward_input = valid_input();
    forward_input["thresholds"]["require_measured_cost"] = json!(false);
    forward_input["cost_buckets"] = json!([
        measured_bucket(1200),
        estimated_bucket(10),
        unknown_bucket("rate-card-missing"),
    ]);
    let mut reversed_input = forward_input.clone();
    reversed_input["cost_buckets"] = json!([
        unknown_bucket("rate-card-missing"),
        estimated_bucket(10),
        measured_bucket(1200),
    ]);

    let forward = evaluate(&forward_input);
    let reversed = evaluate(&reversed_input);
    assert_eq!(forward.len(), 2, "unexpected findings: {forward:?}");
    assert_eq!(
        codes(&forward),
        set(&["metrics.cost_exceeded", "metrics.cost_unknown"])
    );
    assert_eq!(
        canonical_findings(forward.clone()).unwrap(),
        canonical_findings(reversed).unwrap()
    );
    let unknown = forward
        .iter()
        .find(|finding| finding.code == "metrics.cost_unknown")
        .expect("unknown cost finding");
    assert_eq!(unknown.evidence_ref, "metrics:cost");
}

#[test]
fn redaction_boundary_covers_caller_supplied_values() {
    let mut input = valid_input();
    input["cost_buckets"][0]["receipt_ref"] = json!("token=supersecret");
    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["metrics.bucket_measured_unbound"]));
    let serialized = serde_json::to_string(&findings).unwrap();
    assert!(
        !serialized.contains("supersecret"),
        "secret leaked into findings: {serialized}"
    );
    assert!(serialized.contains("[REDACTED]"));
}

#[test]
fn performance_cost_contracts_round_trip_through_typed_input() {
    let typed = PerformanceCostInput::new(
        PerformanceMetrics {
            schema: METRICS_SCHEMA.to_owned(),
            duration_ms: 120,
            duration_measured: true,
            input_tokens: 40,
            output_tokens: 30,
            total_tokens: 70,
            tool_calls: 2,
            cache_lookups: 4,
            cache_hits: 3,
            cache_misses: 1,
            cache_key_digest: DIGEST.to_owned(),
            usage_complete: true,
            measurement_digest: DIGEST.to_owned(),
        },
        PerformanceThresholds {
            schema: THRESHOLDS_SCHEMA.to_owned(),
            max_duration_ms: 200,
            max_total_tokens: 100,
            max_tool_calls: 3,
            max_cost_micros: 1000,
            cost_limit_enforced: true,
            require_measured_cost: true,
        },
        vec![CostBucketEvidence {
            schema: BUCKET_SCHEMA.to_owned(),
            bucket: CostBucket::Measured,
            cost_micros: 900,
            currency: "USD".to_owned(),
            rate_card_ref: "rate-card/2026-10".to_owned(),
            receipt_ref: "receipt/run-2026-10".to_owned(),
            usage_digest: DIGEST.to_owned(),
            unknown_reason: String::new(),
        }],
    );
    assert!(typed.validate().is_ok());
    let value = typed.as_value();
    let decoded: PerformanceCostInput = serde_json::from_value(value.clone()).expect("round-trips");
    assert_eq!(decoded, typed);
    assert!(evaluate_performance_cost(&decoded).unwrap().is_empty());

    let mut tampered = value;
    tampered["schema"] = json!("kiana.quality-performance-cost-input.v0");
    assert!(PerformanceCostEvaluator.evaluate(&tampered).is_err());
}

#[test]
fn performance_output_is_deterministic_evidence_bound_and_registry_wired() {
    let mut fixture = valid_input();
    fixture["metrics"]["duration_ms"] = json!(260);
    fixture["metrics"]["tool_calls"] = json!(5);
    fixture["cost_buckets"] = json!([measured_bucket(1200)]);

    let first = evaluate(&fixture);
    let second = evaluate(&fixture);
    assert_eq!(first, second);
    assert_eq!(
        digest_findings(&first).unwrap(),
        digest_findings(&second).unwrap()
    );
    assert_eq!(
        codes(&first),
        set(&[
            "metrics.duration_exceeded",
            "metrics.tool_calls_exceeded",
            "metrics.cost_exceeded",
        ])
    );
    for finding in &first {
        assert!(
            finding.code.starts_with("metrics."),
            "unstable finding code: {}",
            finding.code
        );
        assert!(
            finding.evidence_ref.starts_with("metrics:"),
            "missing evidence reference: {}",
            finding.evidence_ref
        );
    }

    let mut registry = EvaluatorRegistry::new();
    registry.register(PerformanceCostEvaluator).unwrap();
    assert_eq!(
        registry.evaluator_ids(),
        vec![PERFORMANCE_COST_EVALUATOR_ID]
    );
    let canonical = canonical_findings(first.clone()).unwrap();
    assert_eq!(registry.evaluate(&fixture).unwrap(), canonical);

    let result = registry.evaluate_with_digest(&fixture).unwrap();
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert_eq!(result.findings, canonical);
    assert_eq!(result.findings_digest, digest_findings(&canonical).unwrap());
    assert!(registry
        .evaluate(&json!({"schema": "kiana.quality-performance-cost-input.v0"}))
        .is_err());
}
