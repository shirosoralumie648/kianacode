use kiana_quality::{BlockingRuleEvaluator, DeterministicEvaluator, BLOCKING_RULE_INPUT_SCHEMA};
use serde_json::json;

fn input(triggered_rule: Option<&str>, verdict: &str, score: u64) -> serde_json::Value {
    let names = [
        "safety",
        "evidence",
        "replay",
        "forbidden_effect",
        "fixture_integrity",
        "infra",
    ];
    let rules = names
        .into_iter()
        .map(|rule| {
            json!({
                "schema": "kiana.quality-blocking-rule.v1",
                "rule": rule,
                "triggered": triggered_rule == Some(rule),
                "finding_refs": if triggered_rule == Some(rule) { vec!["finding:one"] } else { Vec::<&str>::new() },
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema": BLOCKING_RULE_INPUT_SCHEMA,
        "rules": rules,
        "weighted_score_milli": score,
        "score_threshold_milli": 800,
        "verdict": verdict,
    })
}

#[test]
fn complete_untriggered_rules_and_passing_score_are_valid() {
    let findings = BlockingRuleEvaluator
        .evaluate(&input(None, "pass", 900))
        .unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn blocking_rule_precedes_weighted_score() {
    let findings = BlockingRuleEvaluator
        .evaluate(&input(Some("safety"), "pass", 999))
        .unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.code == "blocking.rule_precedes_score"));
}

#[test]
fn score_below_threshold_and_missing_matrix_are_not_pass() {
    let low = BlockingRuleEvaluator
        .evaluate(&input(None, "pass", 799))
        .unwrap();
    assert!(low
        .iter()
        .any(|finding| finding.code == "blocking.score_threshold_invalid"));

    let mut missing = input(None, "pass", 900);
    missing["rules"].as_array_mut().unwrap().pop();
    assert!(BlockingRuleEvaluator
        .evaluate(&missing)
        .unwrap()
        .iter()
        .any(|finding| finding.code == "blocking.rule_missing"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut unknown = input(None, "pass", 900);
    unknown["unexpected"] = json!(true);
    assert!(BlockingRuleEvaluator.evaluate(&unknown).is_err());
}
