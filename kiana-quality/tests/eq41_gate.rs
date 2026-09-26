use kiana_quality::{
    DeterministicEvaluator, GateEvaluator, GateInput, GateVerdict, QualityGateConfig,
    QualityGateDecision, GATE_INPUT_SCHEMA,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn config(version: u64, thresholds: &str) -> QualityGateConfig {
    let mut config = QualityGateConfig {
        schema: "kiana.quality-gate-config.v1".to_owned(),
        gate_id: "gate:quality".to_owned(),
        version,
        suite_digest: DIGEST.to_owned(),
        thresholds_digest: thresholds.to_owned(),
        blocking_rules: vec!["infra".to_owned(), "safety".to_owned()],
        config_digest: String::new(),
    };
    config.config_digest = config.digest();
    config
}

fn input() -> (serde_json::Value, QualityGateConfig) {
    let old = config(1, DIGEST);
    let new = config(2, OTHER);
    let mut decision = QualityGateDecision {
        schema: "kiana.quality-gate-decision.v1".to_owned(),
        decision_id: "decision:one".to_owned(),
        gate_id: old.gate_id.clone(),
        config_digest: old.config_digest.clone(),
        candidate_digest: DIGEST.to_owned(),
        verdict: GateVerdict::Pass,
        blocking_findings: Vec::new(),
        decided_at_unix_ms: 100,
        decision_digest: String::new(),
    };
    decision.decision_digest = decision.digest();
    (
        serde_json::to_value(GateInput {
            schema: GATE_INPUT_SCHEMA.to_owned(),
            config: old,
            decision,
            updated_config: Some(new),
        })
        .unwrap(),
        config(2, OTHER),
    )
}

#[test]
fn gate_config_and_decision_are_separate_and_immutable() {
    let (value, updated) = input();
    let findings = GateEvaluator.evaluate(&value).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    assert_eq!(updated.version, 2);
    assert_ne!(
        value["config"]["config_digest"],
        value["updated_config"]["config_digest"]
    );
}

#[test]
fn gate_config_update_does_not_mutate_old_decision() {
    let (mut value, old_updated) = input();
    let mut invalid = old_updated;
    invalid.version = 1;
    invalid.config_digest = invalid.digest();
    value["updated_config"] = serde_json::to_value(invalid).unwrap();
    let findings = GateEvaluator.evaluate(&value).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.code == "gate.config_update_mutated_old_version"));

    let (mut blocker, _) = input();
    let mut typed: GateInput = serde_json::from_value(blocker).unwrap();
    typed.decision.blocking_findings = vec!["safety".to_owned()];
    typed.decision.decision_digest = typed.decision.digest();
    blocker = serde_json::to_value(typed).unwrap();
    let findings = GateEvaluator.evaluate(&blocker).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.code == "gate.pass_with_blocking_findings"));
}

#[test]
fn decision_digest_tamper_and_unknown_fields_fail_closed() {
    let (mut value, _) = input();
    value["decision"]["decision_digest"] = json!(OTHER);
    assert!(GateEvaluator
        .evaluate(&value)
        .unwrap()
        .iter()
        .any(|finding| finding.code == "gate.decision_invalid"));
    let mut unknown = input().0;
    unknown["unexpected"] = Value::Bool(true);
    assert!(GateEvaluator.evaluate(&unknown).is_err());
}
