#[test]
fn blocking_rules_stay_precedence_first_and_pure() {
    let source = include_str!("../src/rules.rs");
    for forbidden in [
        "std::fs",
        "tokio::",
        "reqwest::",
        "Provider",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "Command::new",
        "promote",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden blocking evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "BLOCKING_RULE_INPUT_SCHEMA",
        "BlockingRuleEvaluator",
        "REQUIRED_RULES",
        "blocking.rule_precedes_score",
        "blocking.rule_missing",
        "blocking.finding_ref_missing",
        "blocking.score_threshold_invalid",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-42 boundary marker: {required}"
        );
    }
}
