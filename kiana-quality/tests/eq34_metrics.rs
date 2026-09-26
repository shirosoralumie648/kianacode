use kiana_quality::{
    DeterministicEvaluator, PerformanceCostEvaluator, PERFORMANCE_COST_INPUT_SCHEMA,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn valid_input() -> Value {
    json!({
        "schema": PERFORMANCE_COST_INPUT_SCHEMA,
        "metrics": {
            "schema": "kiana.quality-performance-metrics.v1",
            "duration_ms": 120,
            "input_tokens": 40,
            "output_tokens": 30,
            "total_tokens": 70,
            "tool_calls": 2,
            "cache_hits": 3,
            "cache_misses": 1,
            "cache_key_digest": DIGEST,
            "usage_complete": true,
            "cost_bucket": "measured",
            "cost_micros": 900,
            "measurement_digest": DIGEST,
        },
        "thresholds": {
            "schema": "kiana.quality-performance-thresholds.v1",
            "max_duration_ms": 200,
            "max_total_tokens": 100,
            "max_tool_calls": 3,
            "max_cost_micros": 1000,
            "require_measured_cost": true,
        },
    })
}

#[test]
fn measured_metrics_with_explicit_thresholds_have_no_findings() {
    let findings = PerformanceCostEvaluator.evaluate(&valid_input()).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn budget_and_latency_thresholds_are_explicit() {
    let mut input = valid_input();
    input["metrics"]["duration_ms"] = json!(201);
    input["metrics"]["input_tokens"] = json!(60);
    input["metrics"]["output_tokens"] = json!(60);
    input["metrics"]["total_tokens"] = json!(150);
    input["metrics"]["tool_calls"] = json!(4);
    input["metrics"]["cache_key_digest"] = Value::Null;
    input["metrics"]["usage_complete"] = json!(false);
    input["metrics"]["cost_bucket"] = json!("estimated");
    input["metrics"]["cost_micros"] = json!(1200);
    input["thresholds"]["max_cost_micros"] = json!(1000);

    let findings = PerformanceCostEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"metrics.duration_exceeded"));
    assert!(codes.contains(&"metrics.token_total_invalid"));
    assert!(codes.contains(&"metrics.tokens_exceeded"));
    assert!(codes.contains(&"metrics.tool_calls_exceeded"));
    assert!(codes.contains(&"metrics.cache_key_missing"));
    assert!(codes.contains(&"metrics.usage_incomplete"));
    assert!(codes.contains(&"metrics.cost_unmeasured"));
    assert!(codes.contains(&"metrics.cost_exceeded"));
}

#[test]
fn unknown_cost_and_missing_thresholds_are_not_pass() {
    let mut input = valid_input();
    input["metrics"]["cost_bucket"] = json!("unknown");
    input["metrics"]["cost_micros"] = Value::Null;
    input["thresholds"]["max_duration_ms"] = json!(0);
    input["thresholds"]["max_total_tokens"] = json!(0);
    input["thresholds"]["max_tool_calls"] = json!(0);
    input["thresholds"]["max_cost_micros"] = Value::Null;

    let findings = PerformanceCostEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"metrics.threshold_missing"));
    assert!(codes.contains(&"metrics.cost_unknown"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut input = valid_input();
    input["unexpected"] = json!(true);
    assert!(PerformanceCostEvaluator.evaluate(&input).is_err());
}
