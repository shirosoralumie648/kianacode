use kiana_quality::{
    DeterministicEvaluator, ShadowAdmission, ShadowEvaluator, ShadowInput, ShadowObservation,
    SHADOW_INPUT_SCHEMA,
};
use serde_json::{json, Value};

const CANDIDATE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASELINE: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ROUTE: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const GRANT: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn admission() -> ShadowAdmission {
    let mut admission = ShadowAdmission {
        schema: "kiana.quality-shadow-admission.v1".to_owned(),
        candidate_digest: CANDIDATE.to_owned(),
        baseline_digest: BASELINE.to_owned(),
        route_digest: ROUTE.to_owned(),
        grant_digest: GRANT.to_owned(),
        sample_limit: 10,
        admitted_at_unix_ms: 100,
        expires_at_unix_ms: 200,
        admission_digest: String::new(),
    };
    admission.admission_digest = admission.digest();
    admission
}

fn input(observation: ShadowObservation) -> Value {
    serde_json::to_value(ShadowInput {
        schema: SHADOW_INPUT_SCHEMA.to_owned(),
        admission: admission(),
        observation,
    })
    .unwrap()
}

#[test]
fn shadow_regression_rolls_back_without_grant_change() {
    let findings = ShadowEvaluator
        .evaluate(&input(ShadowObservation {
            schema: "kiana.quality-shadow-observation.v1".to_owned(),
            now_unix_ms: 150,
            samples_observed: 5,
            regression_detected: true,
            rollback_requested: true,
            rollback_route_digest: Some(BASELINE.to_owned()),
            rollback_grant_digest: Some(GRANT.to_owned()),
        }))
        .unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn shadow_rollback_requires_baseline_route_and_unchanged_grant() {
    let mut value = input(ShadowObservation {
        schema: "kiana.quality-shadow-observation.v1".to_owned(),
        now_unix_ms: 150,
        samples_observed: 5,
        regression_detected: true,
        rollback_requested: true,
        rollback_route_digest: Some(ROUTE.to_owned()),
        rollback_grant_digest: Some(CANDIDATE.to_owned()),
    });
    let findings = ShadowEvaluator.evaluate(&value).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"shadow.rollback_route_invalid"));
    assert!(codes.contains(&"shadow.rollback_grant_changed"));

    value["observation"]["rollback_requested"] = json!(false);
    assert!(ShadowEvaluator
        .evaluate(&value)
        .unwrap()
        .iter()
        .any(|finding| finding.code == "shadow.rollback_missing"));
}

#[test]
fn shadow_ttl_and_sample_limits_are_visible() {
    let findings = ShadowEvaluator
        .evaluate(&input(ShadowObservation {
            schema: "kiana.quality-shadow-observation.v1".to_owned(),
            now_unix_ms: 200,
            samples_observed: 11,
            regression_detected: false,
            rollback_requested: true,
            rollback_route_digest: None,
            rollback_grant_digest: None,
        }))
        .unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"shadow.ttl_expired"));
    assert!(codes.contains(&"shadow.sample_limit_exceeded"));
    assert!(codes.contains(&"shadow.rollback_without_regression"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = input(ShadowObservation {
        schema: "kiana.quality-shadow-observation.v1".to_owned(),
        now_unix_ms: 150,
        samples_observed: 1,
        regression_detected: false,
        rollback_requested: false,
        rollback_route_digest: None,
        rollback_grant_digest: None,
    });
    value["unexpected"] = Value::Bool(true);
    assert!(ShadowEvaluator.evaluate(&value).is_err());
}
