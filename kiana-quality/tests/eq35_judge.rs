use kiana_quality::{DeterministicEvaluator, SemanticJudgeEvaluator, SEMANTIC_JUDGE_INPUT_SCHEMA};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn valid_input() -> Value {
    json!({
        "schema": SEMANTIC_JUDGE_INPUT_SCHEMA,
        "required": false,
        "availability": "available",
        "config": {
            "schema": "kiana.quality-semantic-judge-config.v1",
            "provider_id": "offline-judge",
            "model_version": "judge-model-v1",
            "prompt_version": "rubric-v1",
            "temperature_milli": 0,
            "configuration_digest": DIGEST,
        },
        "request": {
            "schema": "kiana.quality-semantic-judge-request.v1",
            "configuration_digest": DIGEST,
            "input_digest": DIGEST,
            "reference_digest": DIGEST,
            "output_digest": DIGEST,
        },
        "result": {
            "schema": "kiana.quality-semantic-judge-result.v1",
            "verdict": "pass",
            "score_milli": 900,
            "result_digest": DIGEST,
        },
        "unavailable_reason": null,
    })
}

#[test]
fn available_fixed_judge_evidence_is_valid() {
    let findings = SemanticJudgeEvaluator.evaluate(&valid_input()).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn judge_unavailable_is_not_a_success() {
    let mut input = valid_input();
    input["required"] = json!(true);
    input["availability"] = json!("unavailable");
    input["unavailable_reason"] = json!("judge_not_configured_in_ci");
    input["result"] = json!({
        "schema": "kiana.quality-semantic-judge-result.v1",
        "verdict": "pass",
        "score_milli": 950,
        "result_digest": DIGEST,
    });

    let findings = SemanticJudgeEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"judge.unavailable"));
    assert!(codes.contains(&"judge.unavailable_not_pass"));
}

#[test]
fn fixed_configuration_and_result_boundaries_fail_closed() {
    let mut input = valid_input();
    input["request"]["configuration_digest"] =
        json!("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    input["config"]["temperature_milli"] = json!(2001);
    input["result"]["score_milli"] = json!(1001);
    input["result"]["result_digest"] = Value::Null;

    let findings = SemanticJudgeEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"judge.config_drift"));
    assert!(codes.contains(&"judge.config_invalid"));
    assert!(codes.contains(&"judge.result_invalid"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut input = valid_input();
    input["unexpected"] = json!(true);
    assert!(SemanticJudgeEvaluator.evaluate(&input).is_err());
}
