use kiana_quality::{
    canonical_findings, digest_findings, evaluate_semantic_judge, DeterministicEvaluator,
    EvaluatorRegistry, Finding, JudgeAvailability, JudgeVerdict, SemanticJudgeConfig,
    SemanticJudgeEvaluator, SemanticJudgeInput, SemanticJudgeRequest, SemanticJudgeResult,
    EVALUATION_RESULT_SCHEMA, MAX_JUDGE_REASON_BYTES, MAX_JUDGE_SCORE_MILLI,
    MAX_JUDGE_TEMPERATURE_MILLI, MAX_JUDGE_TEXT_BYTES, SEMANTIC_JUDGE_EVALUATOR_ID,
    SEMANTIC_JUDGE_INPUT_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const CONFIG_SCHEMA: &str = "kiana.quality-semantic-judge-config.v1";
const REQUEST_SCHEMA: &str = "kiana.quality-semantic-judge-request.v1";
const RESULT_SCHEMA: &str = "kiana.quality-semantic-judge-result.v1";
const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn config() -> Value {
    json!({
        "schema": CONFIG_SCHEMA,
        "provider_id": "offline-judge",
        "model_version": "judge-model-v1",
        "prompt_version": "rubric-v1",
        "prompt_digest": DIGEST,
        "temperature_milli": 0,
        "configuration_digest": DIGEST,
    })
}

fn request() -> Value {
    json!({
        "schema": REQUEST_SCHEMA,
        "configuration_digest": DIGEST,
        "prompt_digest": DIGEST,
        "input_digest": DIGEST,
        "reference_digest": DIGEST,
        "output_digest": DIGEST,
    })
}

fn result() -> Value {
    json!({
        "schema": RESULT_SCHEMA,
        "verdict": "pass",
        "score_milli": 900,
        "result_digest": DIGEST,
    })
}

fn valid_input() -> Value {
    json!({
        "schema": SEMANTIC_JUDGE_INPUT_SCHEMA,
        "required": false,
        "availability": "available",
        "config": config(),
        "request": request(),
        "result": result(),
        "unavailable_reason": null,
    })
}

fn evaluate(input: &Value) -> Vec<Finding> {
    SemanticJudgeEvaluator
        .evaluate(input)
        .expect("semantic judge fixture decodes")
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
fn judge_unavailable_is_not_a_success() {
    // Card acceptance: a required judge that is unavailable blocks, and a forged pass verdict
    // from an unavailable judge is never read as success.
    let mut input = valid_input();
    input["required"] = json!(true);
    input["availability"] = json!("unavailable");
    input["unavailable_reason"] = json!("judge_not_configured_in_ci");
    input["result"]["score_milli"] = json!(950);

    let findings = evaluate(&input);
    assert_eq!(
        codes(&findings),
        set(&["judge.unavailable", "judge.unavailable_not_pass"])
    );
    for finding in &findings {
        assert_eq!(finding.evidence_ref, "judge:availability");
    }
}

#[test]
fn available_fixed_judge_evidence_is_valid() {
    // The success path stays clean, and the `required` flag only matters when the judge is absent.
    assert!(evaluate(&valid_input()).is_empty());

    let mut required = valid_input();
    required["required"] = json!(true);
    assert!(evaluate(&required).is_empty());
}

#[test]
fn optional_unavailable_judge_is_never_a_silent_pass() {
    let unavailable = SemanticJudgeInput::unavailable(false, "judge_backend_unreachable");
    let findings = evaluate(&unavailable.as_value());
    assert_eq!(codes(&findings), set(&["judge.not_evaluated"]));
    assert_eq!(
        findings[0].expected,
        json!("explicit_not_evaluated_not_pass")
    );
    assert_eq!(findings[0].evidence_ref, "judge:availability");

    let not_configured = SemanticJudgeInput::not_configured(false, "no_semantic_judge_wired");
    let findings = evaluate(&not_configured.as_value());
    assert_eq!(codes(&findings), set(&["judge.not_evaluated"]));
}

#[test]
fn not_configured_judge_blocks_when_required() {
    let input = SemanticJudgeInput::not_configured(true, "no_semantic_judge_wired");
    let findings = evaluate(&input.as_value());
    assert_eq!(codes(&findings), set(&["judge.not_configured"]));
    assert_eq!(findings[0].evidence_ref, "judge:availability");
}

#[test]
fn unavailable_judge_requires_a_bounded_reason() {
    let mut missing = valid_input();
    missing["required"] = json!(true);
    missing["availability"] = json!("unavailable");
    missing["unavailable_reason"] = Value::Null;
    missing["result"] = Value::Null;
    assert_eq!(
        codes(&evaluate(&missing)),
        set(&["judge.unavailable", "judge.unavailable_reason_missing"])
    );

    for reason in ["", "   ", "judge\nunavailable", "judge unavailable"] {
        let mut shaped = missing.clone();
        shaped["unavailable_reason"] = json!(reason);
        assert_eq!(
            codes(&evaluate(&shaped)),
            set(&["judge.unavailable", "judge.unavailable_reason_missing"]),
            "reason shape must fail closed: {reason:?}"
        );
    }

    // The envelope bound is structural: an oversized reason never reaches evaluation.
    let mut oversized = missing.clone();
    oversized["unavailable_reason"] = json!("a".repeat(MAX_JUDGE_REASON_BYTES + 1));
    let error = SemanticJudgeEvaluator
        .evaluate(&oversized)
        .expect_err("oversized reason must fail closed");
    assert!(
        error
            .to_string()
            .contains("semantic_judge_reason_limit_exceeded"),
        "unexpected error: {error}"
    );

    // A reason exactly at the documented bound is accepted.
    let mut bounded = missing;
    bounded["unavailable_reason"] = json!("a".repeat(MAX_JUDGE_REASON_BYTES));
    assert_eq!(codes(&evaluate(&bounded)), set(&["judge.unavailable"]));
}

#[test]
fn available_judge_requires_fixed_config_request_and_result() {
    for section in ["config", "request", "result"] {
        let mut input = valid_input();
        input[section] = Value::Null;
        let findings = evaluate(&input);
        assert_eq!(
            codes(&findings),
            set(&["judge.evidence_missing"]),
            "missing {section} must fail closed"
        );
        assert_eq!(findings[0].evidence_ref, "judge:availability");
    }
}

#[test]
fn available_judge_must_not_carry_an_unavailable_reason() {
    let mut input = valid_input();
    input["unavailable_reason"] = json!("stale_unavailable_reason");
    let findings = evaluate(&input);
    assert_eq!(
        codes(&findings),
        set(&["judge.unavailable_reason_unexpected"])
    );
}

#[test]
fn unavailable_judge_cannot_carry_a_result() {
    let mut input = valid_input();
    input["required"] = json!(true);
    input["availability"] = json!("unavailable");
    input["unavailable_reason"] = json!("judge_timeout");
    input["result"] = json!({
        "schema": RESULT_SCHEMA,
        "verdict": "fail",
        "score_milli": 100,
        "result_digest": DIGEST,
    });
    assert_eq!(
        codes(&evaluate(&input)),
        set(&["judge.unavailable", "judge.evidence_unexpected"])
    );
}

#[test]
fn configuration_and_prompt_drift_are_visible() {
    let mut config_drift = valid_input();
    config_drift["request"]["configuration_digest"] = json!(OTHER_DIGEST);
    assert_eq!(
        codes(&evaluate(&config_drift)),
        set(&["judge.config_drift"])
    );

    let mut prompt_drift = valid_input();
    prompt_drift["request"]["prompt_digest"] = json!(OTHER_DIGEST);
    assert_eq!(
        codes(&evaluate(&prompt_drift)),
        set(&["judge.prompt_drift"])
    );

    let mut both = valid_input();
    both["request"]["configuration_digest"] = json!(OTHER_DIGEST);
    both["request"]["prompt_digest"] = json!(OTHER_DIGEST);
    assert_eq!(
        codes(&evaluate(&both)),
        set(&["judge.config_drift", "judge.prompt_drift"])
    );
}

#[test]
fn fixed_identity_requires_bounded_fields_and_digests() {
    let invalid_identities = [
        ("provider_id", json!("offline judge")),
        ("provider_id", json!("")),
        ("model_version", json!("judge-model\nv1")),
        (
            "prompt_version",
            json!("r".repeat(MAX_JUDGE_TEXT_BYTES + 1)),
        ),
        ("prompt_digest", json!("sha256:short")),
        ("temperature_milli", json!(MAX_JUDGE_TEMPERATURE_MILLI + 1)),
        ("configuration_digest", json!("deadbeef")),
    ];
    for (field, value) in invalid_identities {
        let mut input = valid_input();
        input["config"][field] = value;
        let findings = evaluate(&input);
        let config_invalid = findings
            .iter()
            .find(|finding| finding.code == "judge.config_invalid")
            .unwrap_or_else(|| panic!("field {field} must fail closed: {findings:?}"));
        // A digest field also drifts from the request, which is reported separately; the
        // configuration finding itself always cites the fixed-identity section.
        assert_eq!(config_invalid.evidence_ref, "judge:config");
    }

    // The documented temperature and text bounds are inclusive.
    let mut bounded = valid_input();
    bounded["config"]["temperature_milli"] = json!(MAX_JUDGE_TEMPERATURE_MILLI);
    bounded["config"]["model_version"] = json!("m".repeat(MAX_JUDGE_TEXT_BYTES));
    assert!(evaluate(&bounded).is_empty());

    let mut schema_drift = valid_input();
    schema_drift["config"]["schema"] = json!("kiana.quality-semantic-judge-config.v0");
    assert_eq!(
        codes(&evaluate(&schema_drift)),
        set(&["judge.config_schema_invalid"])
    );
}

#[test]
fn request_digests_must_be_strict_sha256() {
    for field in [
        "configuration_digest",
        "prompt_digest",
        "input_digest",
        "reference_digest",
        "output_digest",
    ] {
        let mut input = valid_input();
        input["request"][field] = json!("sha256:not-a-digest");
        let findings = evaluate(&input);
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "judge.request_invalid"),
            "field {field} must fail closed: {findings:?}"
        );
        assert_eq!(
            findings
                .iter()
                .find(|finding| finding.code == "judge.request_invalid")
                .unwrap()
                .evidence_ref,
            "judge:request"
        );
    }

    let mut schema_drift = valid_input();
    schema_drift["request"]["schema"] = json!("kiana.quality-semantic-judge-request.v0");
    assert_eq!(
        codes(&evaluate(&schema_drift)),
        set(&["judge.request_schema_invalid"])
    );
}

#[test]
fn result_score_and_digest_bounds_fail_closed() {
    let mut score = valid_input();
    score["result"]["score_milli"] = json!(MAX_JUDGE_SCORE_MILLI + 1);
    let findings = evaluate(&score);
    assert_eq!(codes(&findings), set(&["judge.result_invalid"]));
    assert_eq!(findings[0].evidence_ref, "judge:result");

    let mut digest = valid_input();
    digest["result"]["result_digest"] = json!("sha256:0000");
    assert_eq!(codes(&evaluate(&digest)), set(&["judge.result_invalid"]));

    let mut bounded = valid_input();
    bounded["result"]["score_milli"] = json!(MAX_JUDGE_SCORE_MILLI);
    assert!(evaluate(&bounded).is_empty());

    let mut schema_drift = valid_input();
    schema_drift["result"]["schema"] = json!("kiana.quality-semantic-judge-result.v0");
    assert_eq!(
        codes(&evaluate(&schema_drift)),
        set(&["judge.result_schema_invalid"])
    );
}

#[test]
fn failed_judge_verdict_is_not_silently_passed() {
    let mut failed = valid_input();
    failed["result"]["verdict"] = json!("fail");
    assert_eq!(codes(&evaluate(&failed)), set(&["judge.verdict_failed"]));

    let mut review = valid_input();
    review["result"]["verdict"] = json!("needs_review");
    assert_eq!(
        codes(&evaluate(&review)),
        set(&["judge.verdict_needs_review"])
    );
}

#[test]
fn pass_verdict_with_zero_score_is_inconsistent() {
    let mut inconsistent = valid_input();
    inconsistent["result"]["score_milli"] = json!(0);
    assert_eq!(
        codes(&evaluate(&inconsistent)),
        set(&["judge.result_inconsistent"])
    );

    // A zero score with a non-pass verdict carries no such contradiction.
    let mut failed = valid_input();
    failed["result"]["score_milli"] = json!(0);
    failed["result"]["verdict"] = json!("fail");
    assert_eq!(codes(&evaluate(&failed)), set(&["judge.verdict_failed"]));
}

#[test]
fn unknown_fields_and_envelope_drift_fail_closed() {
    let mut unknown_envelope = valid_input();
    unknown_envelope["unexpected"] = json!(true);
    assert!(SemanticJudgeEvaluator.evaluate(&unknown_envelope).is_err());

    for section in ["config", "request", "result"] {
        let mut unknown_section = valid_input();
        unknown_section[section]["unexpected"] = json!(true);
        assert!(
            SemanticJudgeEvaluator.evaluate(&unknown_section).is_err(),
            "unknown {section} field must fail closed"
        );
    }

    let mut envelope_drift = valid_input();
    envelope_drift["schema"] = json!("kiana.quality-semantic-judge-input.v0");
    let error = SemanticJudgeEvaluator
        .evaluate(&envelope_drift)
        .expect_err("envelope drift must fail closed");
    assert!(
        error
            .to_string()
            .contains("semantic_judge_input_schema_invalid"),
        "unexpected error: {error}"
    );
}

#[test]
fn judge_contracts_round_trip_through_typed_input() {
    let typed = SemanticJudgeInput::available(
        true,
        SemanticJudgeConfig {
            schema: CONFIG_SCHEMA.to_owned(),
            provider_id: "offline-judge".to_owned(),
            model_version: "judge-model-v1".to_owned(),
            prompt_version: "rubric-v1".to_owned(),
            prompt_digest: DIGEST.to_owned(),
            temperature_milli: 0,
            configuration_digest: DIGEST.to_owned(),
        },
        SemanticJudgeRequest {
            schema: REQUEST_SCHEMA.to_owned(),
            configuration_digest: DIGEST.to_owned(),
            prompt_digest: DIGEST.to_owned(),
            input_digest: DIGEST.to_owned(),
            reference_digest: DIGEST.to_owned(),
            output_digest: DIGEST.to_owned(),
        },
        SemanticJudgeResult {
            schema: RESULT_SCHEMA.to_owned(),
            verdict: JudgeVerdict::Pass,
            score_milli: 900,
            result_digest: DIGEST.to_owned(),
        },
    );
    assert_eq!(typed.availability, JudgeAvailability::Available);
    assert!(typed.validate().is_ok());
    let value = typed.as_value();
    let decoded: SemanticJudgeInput = serde_json::from_value(value.clone()).expect("round-trips");
    assert_eq!(decoded, typed);
    assert!(evaluate_semantic_judge(&decoded).unwrap().is_empty());

    let mut tampered = value;
    tampered["schema"] = json!("kiana.quality-semantic-judge-input.v0");
    assert!(SemanticJudgeEvaluator.evaluate(&tampered).is_err());
}

#[test]
fn redaction_boundary_covers_caller_supplied_values() {
    let mut input = valid_input();
    input["config"]["provider_id"] = json!("token=supersecret");
    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["judge.config_invalid"]));
    let serialized = serde_json::to_string(&findings).unwrap();
    assert!(
        !serialized.contains("supersecret"),
        "secret leaked into findings: {serialized}"
    );
    assert!(serialized.contains("[REDACTED]"));
}

#[test]
fn evidence_sections_are_explicit_per_finding_family() {
    let mut input = valid_input();
    input["config"]["schema"] = json!("kiana.quality-semantic-judge-config.v0");
    input["request"]["schema"] = json!("kiana.quality-semantic-judge-request.v0");
    input["result"]["schema"] = json!("kiana.quality-semantic-judge-result.v0");
    let findings = evaluate(&input);
    assert_eq!(
        codes(&findings),
        set(&[
            "judge.config_schema_invalid",
            "judge.request_schema_invalid",
            "judge.result_schema_invalid",
        ])
    );
    let refs: BTreeSet<&str> = findings
        .iter()
        .map(|finding| finding.evidence_ref.as_str())
        .collect();
    assert_eq!(
        refs,
        BTreeSet::from(["judge:config", "judge:request", "judge:result"])
    );

    let absent = SemanticJudgeInput::unavailable(true, "judge_backend_unreachable");
    let findings = evaluate(&absent.as_value());
    assert_eq!(findings[0].evidence_ref, "judge:availability");
}

#[test]
fn judge_output_is_deterministic_evidence_bound_and_registry_wired() {
    let mut fixture = valid_input();
    fixture["required"] = json!(true);
    fixture["availability"] = json!("unavailable");
    fixture["unavailable_reason"] = json!("judge_backend_unreachable");
    fixture["result"] = Value::Null;

    let first = evaluate(&fixture);
    let second = evaluate(&fixture);
    assert_eq!(first, second);
    assert_eq!(
        digest_findings(&first).unwrap(),
        digest_findings(&second).unwrap()
    );
    assert_eq!(codes(&first), set(&["judge.unavailable"]));
    for finding in &first {
        assert!(
            finding.code.starts_with("judge."),
            "unstable finding code: {}",
            finding.code
        );
        assert!(
            finding.evidence_ref.starts_with("judge:"),
            "missing evidence reference: {}",
            finding.evidence_ref
        );
    }

    let mut registry = EvaluatorRegistry::new();
    registry.register(SemanticJudgeEvaluator).unwrap();
    assert_eq!(registry.evaluator_ids(), vec![SEMANTIC_JUDGE_EVALUATOR_ID]);
    let canonical = canonical_findings(first.clone()).unwrap();
    assert_eq!(registry.evaluate(&fixture).unwrap(), canonical);

    let result = registry.evaluate_with_digest(&fixture).unwrap();
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert_eq!(result.findings, canonical);
    assert_eq!(result.findings_digest, digest_findings(&canonical).unwrap());
    assert!(registry
        .evaluate(&json!({"schema": "kiana.quality-semantic-judge-input.v0"}))
        .is_err());
}
