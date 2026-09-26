use kiana_quality::{DeterministicEvaluator, FlakeClassifierEvaluator, FLAKE_INPUT_SCHEMA};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn input(attempts: Value, final_status: &str, quarantine: Value) -> Value {
    json!({
        "schema": FLAKE_INPUT_SCHEMA,
        "retry_once": true,
        "attempts": attempts,
        "final_status": final_status,
        "quarantine": quarantine,
    })
}

fn attempt(number: u64, outcome: &str, infra_class: Value) -> Value {
    json!({
        "schema": "kiana.quality-flake-attempt.v1",
        "attempt": number,
        "outcome": outcome,
        "infra_class": infra_class,
    })
}

fn quarantine() -> Value {
    json!({
        "schema": "kiana.quality-flake-quarantine.v1",
        "case_id": "case:one",
        "reason": "first_failure_then_pass",
        "owner": "quality-owner",
        "expires_at_unix_ms": 2000000000000u64,
        "evidence_digest": DIGEST,
    })
}

#[test]
fn stable_pass_has_no_findings() {
    let value = input(
        json!([attempt(1, "pass", Value::Null)]),
        "pass",
        Value::Null,
    );
    let findings = FlakeClassifierEvaluator.evaluate(&value).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn flaky_is_never_counted_as_pass() {
    let value = input(
        json!([
            attempt(1, "fail", Value::Null),
            attempt(2, "pass", Value::Null),
        ]),
        "pass",
        Value::Null,
    );
    let findings = FlakeClassifierEvaluator.evaluate(&value).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"flake.flaky_counted_as_pass"));
    assert!(codes.contains(&"flake.quarantine_missing"));

    let quarantined = input(
        json![
            attempt(1, "fail", Value::Null),
            attempt(2, "pass", Value::Null)
        ],
        "quarantined",
        quarantine(),
    );
    assert!(FlakeClassifierEvaluator
        .evaluate(&quarantined)
        .unwrap()
        .is_empty());
}

#[test]
fn infra_failure_requires_classification_and_cannot_be_a_result() {
    let value = input(
        json!([attempt(1, "infra", Value::Null)]),
        "pass",
        Value::Null,
    );
    let findings = FlakeClassifierEvaluator.evaluate(&value).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"flake.infra_unclassified"));
    assert!(codes.contains(&"flake.infra_counted_as_result"));
}

#[test]
fn retry_after_pass_and_unknown_fields_fail_closed() {
    let value = input(
        json![
            attempt(1, "pass", Value::Null),
            attempt(2, "fail", Value::Null)
        ],
        "fail",
        Value::Null,
    );
    assert!(FlakeClassifierEvaluator
        .evaluate(&value)
        .unwrap()
        .iter()
        .any(|finding| finding.code == "flake.retry_after_pass"));

    let mut unknown = input(json![attempt(1, "pass", Value::Null)], "pass", Value::Null);
    unknown["unexpected"] = json!(true);
    assert!(FlakeClassifierEvaluator.evaluate(&unknown).is_err());
}
