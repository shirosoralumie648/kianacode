use kiana_quality::{
    build_gate_blocking_decision, evaluate_blocking_rules, verify_gate_blocking_decision,
    BlockingRuleDecisionInput, BlockingRuleEvaluator, BlockingRuleEvidence, BlockingRuleInput,
    DeterministicEvaluator, GateVerdict, QualityGateConfig, BLOCKING_RULE_BELOW_THRESHOLD,
    BLOCKING_RULE_CONFIG_DRIFT, BLOCKING_RULE_CONFIG_RULE_UNKNOWN, BLOCKING_RULE_DECISION_FORGED,
    BLOCKING_RULE_DECISION_INPUT_SCHEMA, BLOCKING_RULE_DUPLICATE, BLOCKING_RULE_EVIDENCE_INVALID,
    BLOCKING_RULE_EVIDENCE_SCHEMA, BLOCKING_RULE_FINDING_REF_INVALID,
    BLOCKING_RULE_FINDING_REF_MISSING, BLOCKING_RULE_INPUT_SCHEMA, BLOCKING_RULE_MISSING,
    BLOCKING_RULE_SCORE_OUT_OF_BOUNDS, BLOCKING_RULE_TRIGGERED_FORCES_REJECT,
    BLOCKING_RULE_UNKNOWN,
};
use serde_json::json;
use std::collections::BTreeMap;

const RULES: [&str; 6] = [
    "safety",
    "evidence",
    "replay",
    "forbidden_effect",
    "fixture_integrity",
    "infra",
];

fn sha(hex: char) -> String {
    format!("sha256:{}", hex.to_string().repeat(64))
}

fn evidence(rule: &str, triggered: bool) -> BlockingRuleEvidence {
    BlockingRuleEvidence {
        schema: BLOCKING_RULE_EVIDENCE_SCHEMA.to_owned(),
        rule: rule.to_owned(),
        triggered,
        finding_refs: if triggered {
            vec![format!("finding:{rule}")]
        } else {
            Vec::new()
        },
    }
}

fn rules() -> Vec<String> {
    // The EQ-41 config requires sorted, unique `blocking_rules`.
    let mut rules = RULES
        .iter()
        .map(|rule| (*rule).to_owned())
        .collect::<Vec<_>>();
    rules.sort();
    rules
}

fn rule_index(input: &BlockingRuleDecisionInput, rule: &str) -> usize {
    input
        .rules
        .iter()
        .position(|row| row.rule == rule)
        .unwrap_or_else(|| panic!("missing rule evidence: {rule}"))
}

fn config(version: u64, rules: Vec<String>) -> QualityGateConfig {
    let thresholds = BTreeMap::from([("policy_safety".to_owned(), 900u64)]);
    QualityGateConfig::new("gate:quality", version, sha('1'), thresholds, rules)
        .expect("valid gate config")
}

fn decision_input(
    config: QualityGateConfig,
    triggered: Option<&str>,
    score: u64,
    threshold: u64,
    verdict: GateVerdict,
) -> BlockingRuleDecisionInput {
    BlockingRuleDecisionInput {
        schema: BLOCKING_RULE_DECISION_INPUT_SCHEMA.to_owned(),
        config,
        decision_id: "decision:eq42".to_owned(),
        candidate_id: "candidate:eq42".to_owned(),
        candidate_digest: sha('9'),
        rules: RULES
            .iter()
            .map(|rule| evidence(rule, triggered == Some(*rule)))
            .collect(),
        weighted_score_milli: score,
        score_threshold_milli: threshold,
        proposed_verdict: verdict,
        evidence_ref: "evidence:eq42:run".to_owned(),
        source_cursor: 42,
        decided_at_unix_ms: 1_000,
    }
}

fn findings(input: BlockingRuleInput) -> Vec<String> {
    input
        .validate()
        .map(|()| evaluate_blocking_rules(&input).expect("findings"))
        .unwrap_or_default()
        .into_iter()
        .map(|finding| finding.code)
        .collect()
}

#[test]
fn high_score_cannot_override_a_triggered_safety_rule() {
    let mut input = decision_input(
        config(1, rules()),
        Some("safety"),
        kiana_quality::MAX_GATE_SCORE_MILLI,
        800,
        GateVerdict::Pass,
    );
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_TRIGGERED_FORCES_REJECT),
    );
    // Even the strongest score in the contract cannot buy a Pass over a triggered rule.
    input.weighted_score_milli = 999_999;
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_TRIGGERED_FORCES_REJECT),
    );
    // A shadow verdict is not a lawful downgrade either.
    input.proposed_verdict = GateVerdict::NeedsShadow;
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_TRIGGERED_FORCES_REJECT),
    );
}

#[test]
fn below_threshold_score_cannot_be_a_pass_decision() {
    let input = decision_input(config(1, rules()), None, 799, 800, GateVerdict::Pass);
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_BELOW_THRESHOLD),
    );
    // The same clean, below-threshold evidence may only be shadowed.
    let shadow = BlockingRuleDecisionInput {
        proposed_verdict: GateVerdict::NeedsShadow,
        ..input.clone()
    };
    let decision = build_gate_blocking_decision(&shadow).expect("shadow decision");
    assert_eq!(decision.verdict, GateVerdict::NeedsShadow);
    assert!(decision.blocking_findings.is_empty());
    // A stronger verdict still cannot be recorded without a reason: the EQ-41 matrix refuses a
    // reasonless Reject, so the binder never invents a blocker.
    let reject = BlockingRuleDecisionInput {
        proposed_verdict: GateVerdict::Reject,
        ..input.clone()
    };
    assert_eq!(
        build_gate_blocking_decision(&reject),
        Err(kiana_quality::BLOCKING_RULE_DECISION_INVALID),
    );
}

#[test]
fn missing_rule_evidence_fails_closed() {
    let mut input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    input.rules.retain(|row| row.rule != "replay");
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_MISSING),
    );
}

#[test]
fn unknown_rule_evidence_fails_closed() {
    let mut input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    input.rules.push(evidence("vibes", false));
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_UNKNOWN),
    );
}

#[test]
fn duplicate_rule_evidence_fails_closed() {
    let mut input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    let duplicate = input.rules[0].clone();
    input.rules.push(duplicate);
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_DUPLICATE),
    );
}

#[test]
fn config_declaring_a_narrower_matrix_fails_closed() {
    // A config declaring a subset of the required matrix is refused: EQ-42 evaluates all six
    // rules, so a narrower declaration can never yield a decision.  The evidence matrix is
    // trimmed to the declared rule so the request is otherwise self-consistent.
    let partial = config(1, vec!["safety".to_owned()]);
    let mut input = decision_input(partial, None, 950, 800, GateVerdict::Pass);
    input.rules.retain(|row| row.rule == "safety");
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_MISSING),
    );
}

#[test]
fn evidence_for_an_undeclared_rule_is_config_drift() {
    // The config drops one declared rule while the evidence still reports it: the evidence set and
    // the declared set disagree, which is drift rather than a silently ignored row.
    let mut input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    input.config.blocking_rules.retain(|rule| rule != "infra");
    input.config.config_digest = input.config.digest();
    input
        .config
        .validate()
        .expect("resealed five-rule config is still a valid EQ-41 value");
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_CONFIG_DRIFT),
    );
}

#[test]
fn config_declaring_an_unknown_rule_fails_closed() {
    // EQ-41 already refuses unknown rules at config construction, so this refusal is reached by
    // decoding a digest-correct config that smuggles an unknown declared rule past `new`.
    let mut config = config(1, rules());
    config.blocking_rules.push("vibes".to_owned());
    config.blocking_rules.sort();
    config.config_digest = config.digest();
    assert_eq!(
        config.validate(),
        Err("gate_config_blocking_rule_unknown"),
        "the EQ-41 contract must refuse the unknown rule first"
    );
    let input = decision_input(config, None, 950, 800, GateVerdict::Pass);
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_CONFIG_RULE_UNKNOWN),
    );
}

#[test]
fn triggered_rule_without_finding_refs_fails_closed() {
    let mut input = decision_input(
        config(1, rules()),
        Some("safety"),
        950,
        800,
        GateVerdict::Reject,
    );
    let safety = rule_index(&input, "safety");
    input.rules[safety].finding_refs.clear();
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_FINDING_REF_MISSING),
    );

    let mut input = decision_input(
        config(1, rules()),
        Some("safety"),
        950,
        800,
        GateVerdict::Reject,
    );
    let safety = rule_index(&input, "safety");
    input.rules[safety].finding_refs = vec!["bad ref".to_owned()];
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_FINDING_REF_INVALID),
    );
}

#[test]
fn malformed_headers_scores_and_schemas_fail_closed() {
    let mut input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    input.score_threshold_milli = 0;
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_SCORE_OUT_OF_BOUNDS),
    );

    let mut input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    input.weighted_score_milli = kiana_quality::MAX_GATE_SCORE_MILLI + 1;
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_SCORE_OUT_OF_BOUNDS),
    );

    let mut input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    input.schema = "kiana.quality-blocking-rule-decision-input.v0".to_owned();
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(kiana_quality::BLOCKING_RULE_INPUT_INVALID),
    );

    let mut input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    let safety = rule_index(&input, "safety");
    input.rules[safety].schema = "kiana.quality-blocking-rule.v0".to_owned();
    assert_eq!(
        build_gate_blocking_decision(&input),
        Err(BLOCKING_RULE_EVIDENCE_INVALID),
    );
}

#[test]
fn forged_decision_and_unbound_version_are_refused() {
    let input = decision_input(config(1, rules()), None, 950, 800, GateVerdict::Pass);
    let genuine = build_gate_blocking_decision(&input).expect("pass decision");

    // A forged Pass over a triggered rule can never be built, and a hand-edited verdict is caught.
    let mut forged = genuine.clone();
    forged.verdict = GateVerdict::Reject;
    forged.decision_digest = forged.digest();
    assert_eq!(
        verify_gate_blocking_decision(&input, &forged),
        Err(BLOCKING_RULE_DECISION_FORGED),
    );

    let mut forged = genuine.clone();
    forged.blocking_findings = vec!["blocking:safety".to_owned()];
    forged.decision_digest = forged.digest();
    assert_eq!(
        verify_gate_blocking_decision(&input, &forged),
        Err(BLOCKING_RULE_DECISION_FORGED),
    );

    // A decision built under a different, valid config version is refused: the binding axes and
    // the derived digest both differ from the request's config version.
    let newer = config(2, rules());
    let mut other_input = decision_input(newer.clone(), None, 950, 800, GateVerdict::Pass);
    other_input.decision_id = "decision:eq42".to_owned();
    let other = build_gate_blocking_decision(&other_input).expect("other decision");
    assert_eq!(other.gate_version, 2);
    assert_eq!(other.config_digest, newer.config_digest);
    assert_eq!(
        verify_gate_blocking_decision(&input, &other),
        Err(BLOCKING_RULE_DECISION_FORGED),
    );
}

#[test]
fn triggered_rule_forces_reject_or_rollback_never_pass() {
    let input = decision_input(
        config(1, rules()),
        Some("forbidden_effect"),
        950,
        800,
        GateVerdict::Reject,
    );
    let decision = build_gate_blocking_decision(&input).expect("reject decision");
    assert_eq!(decision.verdict, GateVerdict::Reject);
    assert_eq!(
        decision.blocking_findings,
        vec!["blocking:forbidden_effect".to_owned()]
    );
    assert_eq!(decision.known_regressions, decision.blocking_findings);
    assert!(decision
        .blocking_findings
        .iter()
        .all(|code| !code.is_empty()));
    // Every verdict field of a decodable decision still has to satisfy the EQ-41 matrix.
    decision
        .validate()
        .expect("EQ-41 verdict matrix accepts the reject");

    let rollback = BlockingRuleDecisionInput {
        proposed_verdict: GateVerdict::Rollback,
        ..input
    };
    assert_eq!(
        build_gate_blocking_decision(&rollback)
            .expect("rollback")
            .verdict,
        GateVerdict::Rollback,
    );
}

#[test]
fn clean_matrix_below_threshold_cannot_be_reported_as_pass() {
    // Rule-only evaluator (first slice) agrees with the gate binder.
    let mut standalone = BlockingRuleInput {
        schema: BLOCKING_RULE_INPUT_SCHEMA.to_owned(),
        rules: RULES.iter().map(|rule| evidence(rule, false)).collect(),
        weighted_score_milli: 799,
        score_threshold_milli: 800,
        verdict: kiana_quality::BlockingVerdict::Pass,
    };
    assert_eq!(
        findings(standalone.clone()),
        vec!["blocking.score_threshold_invalid".to_owned()]
    );
    standalone.weighted_score_milli = 800;
    assert!(findings(standalone).is_empty());
}

#[test]
fn clean_untriggered_evidence_with_passing_score_builds_a_bound_pass() {
    let config = config(3, rules());
    let input = decision_input(config.clone(), None, 900, 800, GateVerdict::Pass);
    let decision = build_gate_blocking_decision(&input).expect("pass decision");

    assert_eq!(decision.verdict, GateVerdict::Pass);
    assert!(decision.blocking_findings.is_empty());
    assert!(decision.known_regressions.is_empty());
    assert_eq!(decision.gate_id, config.gate_id);
    assert_eq!(decision.gate_version, 3);
    assert_eq!(decision.config_digest, config.config_digest);
    assert_eq!(decision.suite_digest, config.suite_digest);
    assert_eq!(decision.score_delta_milli, 900);
    decision
        .validate()
        .expect("EQ-41 verdict matrix accepts the pass");
    assert_eq!(verify_gate_blocking_decision(&input, &decision), Ok(()));

    // Deterministic: the same evidence derives the same digest.
    let again = build_gate_blocking_decision(&input).expect("same decision");
    assert_eq!(again.decision_digest, decision.decision_digest);

    // The typed request round-trips through JSON without losing the binding.
    let wire = serde_json::to_value(&input).expect("serializable");
    let decoded: BlockingRuleDecisionInput = serde_json::from_value(wire).expect("decodable");
    assert_eq!(decoded, input);
}

#[test]
fn standalone_rule_evaluator_keeps_its_first_slice_matrix() {
    let complete = json!({
        "schema": BLOCKING_RULE_INPUT_SCHEMA,
        "rules": RULES
            .iter()
            .map(|rule| json!({
                "schema": BLOCKING_RULE_EVIDENCE_SCHEMA,
                "rule": rule,
                "triggered": false,
                "finding_refs": Vec::<String>::new(),
            }))
            .collect::<Vec<_>>(),
        "weighted_score_milli": 900,
        "score_threshold_milli": 800,
        "verdict": "pass",
    });
    assert!(BlockingRuleEvaluator
        .evaluate(&complete)
        .unwrap()
        .is_empty());

    let mut missing = complete.clone();
    missing["rules"].as_array_mut().unwrap().pop();
    let codes = BlockingRuleEvaluator
        .evaluate(&missing)
        .unwrap()
        .into_iter()
        .map(|finding| finding.code)
        .collect::<Vec<_>>();
    assert_eq!(codes, vec!["blocking.rule_missing".to_owned()]);

    let mut triggered = complete.clone();
    triggered["rules"][0]["triggered"] = json!(true);
    triggered["rules"][0]["finding_refs"] = json!(["finding:one"]);
    let codes = BlockingRuleEvaluator
        .evaluate(&triggered)
        .unwrap()
        .into_iter()
        .map(|finding| finding.code)
        .collect::<Vec<_>>();
    assert_eq!(codes, vec!["blocking.rule_precedes_score".to_owned()]);

    let mut unknown = complete;
    unknown["unexpected"] = json!(true);
    assert!(BlockingRuleEvaluator.evaluate(&unknown).is_err());
}
