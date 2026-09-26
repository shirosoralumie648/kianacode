use kiana_quality::{AggregationEvaluator, DeterministicEvaluator, AGGREGATION_INPUT_SCHEMA};
use serde_json::{json, Value};

fn dimension(sample_count: u32, minimum_samples: u32, observed_milli: u64, verdict: &str) -> Value {
    json!({
        "schema": "kiana.quality-dimension-aggregate.v1",
        "dimension": "latency",
        "sample_count": sample_count,
        "minimum_samples": minimum_samples,
        "observed_milli": observed_milli,
        "baseline_milli": 1000,
        "absolute_limit_milli": 1300,
        "relative_limit_milli": 200,
        "confidence_low_milli": 900,
        "confidence_high_milli": 1200,
        "confidence_required": true,
        "verdict": verdict,
    })
}

fn input(dimensions: Vec<Value>) -> Value {
    json!({"schema": AGGREGATION_INPUT_SCHEMA, "dimensions": dimensions})
}

#[test]
fn sufficient_sample_with_explicit_thresholds_and_interval_is_valid() {
    let findings = AggregationEvaluator
        .evaluate(&input(vec![dimension(30, 20, 1100, "pass")]))
        .unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn insufficient_sample_is_blocked_or_needs_review() {
    let blocked = AggregationEvaluator
        .evaluate(&input(vec![dimension(4, 10, 1100, "blocked")]))
        .unwrap();
    assert!(blocked
        .iter()
        .any(|finding| finding.code == "aggregation.insufficient_sample"));
    assert!(!blocked
        .iter()
        .any(|finding| finding.code == "aggregation.sample_verdict_invalid"));

    let pass = AggregationEvaluator
        .evaluate(&input(vec![dimension(4, 10, 1100, "pass")]))
        .unwrap();
    assert!(pass
        .iter()
        .any(|finding| finding.code == "aggregation.false_pass"));
}

#[test]
fn absolute_relative_and_confidence_failures_are_explicit() {
    let mut value = dimension(20, 10, 1500, "pass");
    value["confidence_low_milli"] = json!(1000);
    value["confidence_high_milli"] = json!(1400);
    let findings = AggregationEvaluator.evaluate(&input(vec![value])).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"aggregation.absolute_threshold_exceeded"));
    assert!(codes.contains(&"aggregation.relative_threshold_exceeded"));
    assert!(codes.contains(&"aggregation.confidence_invalid"));
    assert!(codes.contains(&"aggregation.false_pass"));
}

#[test]
fn missing_relative_baseline_confidence_and_unknown_fields_fail_closed() {
    let mut value = dimension(10, 10, 1000, "needs_review");
    value["baseline_milli"] = Value::Null;
    value["confidence_high_milli"] = Value::Null;
    let findings = AggregationEvaluator.evaluate(&input(vec![value])).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"aggregation.relative_threshold_invalid"));
    assert!(codes.contains(&"aggregation.confidence_missing"));

    let mut unknown = input(vec![dimension(20, 10, 1100, "pass")]);
    unknown["dimensions"][0]["unexpected"] = json!(true);
    assert!(AggregationEvaluator.evaluate(&unknown).is_err());
}
