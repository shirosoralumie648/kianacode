use kiana_quality::{
    BaselineEvaluator, BaselineRecord, BaselineRegistry, DeterministicEvaluator,
    BASELINE_INPUT_SCHEMA,
};
use serde_json::json;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn record() -> BaselineRecord {
    let mut record = BaselineRecord {
        schema: "kiana.quality-baseline-record.v1".to_owned(),
        baseline_id: "baseline:one".to_owned(),
        suite_digest: DIGEST.to_owned(),
        case_digest: DIGEST.to_owned(),
        target_digest: DIGEST.to_owned(),
        evaluator_digest: DIGEST.to_owned(),
        owner_id: "quality-owner".to_owned(),
        created_at_unix_ms: 100,
        expires_at_unix_ms: 200,
        refresh_provenance: "capture:one".to_owned(),
        baseline_digest: String::new(),
    };
    record.baseline_digest = record.digest();
    record
}

fn comparison(record: BaselineRecord, now: u64, digest: &str) -> serde_json::Value {
    json!({
        "schema": BASELINE_INPUT_SCHEMA,
        "baseline": record,
        "now_unix_ms": now,
        "suite_digest": digest,
        "case_digest": digest,
        "target_digest": digest,
        "evaluator_digest": digest,
    })
}

#[test]
fn compatible_unexpired_baseline_can_compare() {
    let findings = BaselineEvaluator
        .evaluate(&comparison(record(), 150, DIGEST))
        .unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn stale_or_incompatible_baseline_cannot_compare() {
    let stale = BaselineEvaluator
        .evaluate(&comparison(record(), 200, DIGEST))
        .unwrap();
    assert!(stale.iter().any(|finding| finding.code == "baseline.stale"));

    let incompatible = BaselineEvaluator
        .evaluate(&comparison(record(), 150, OTHER_DIGEST))
        .unwrap();
    assert!(incompatible
        .iter()
        .any(|finding| finding.code == "baseline.incompatible"));
}

#[test]
fn registry_rejects_duplicate_or_tampered_entries() {
    let first = record();
    let mut second = first.clone();
    second.baseline_id = "baseline:two".to_owned();
    second.baseline_digest = second.digest();
    let mut registry = BaselineRegistry {
        schema: "kiana.quality-baseline-registry.v1".to_owned(),
        entries: vec![first.clone(), second],
        registry_digest: String::new(),
    };
    registry.registry_digest = registry.digest();
    assert!(registry.validate().is_ok());
    registry.entries.push(first);
    assert!(registry.validate().is_err());
}

#[test]
fn unknown_fields_fail_closed() {
    let mut input = comparison(record(), 150, DIGEST);
    input["unexpected"] = json!(true);
    assert!(BaselineEvaluator.evaluate(&input).is_err());
}
