#[test]
fn semantic_judge_evaluator_stays_optional_and_fail_closed() {
    let source = include_str!("../src/judge.rs");
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
        "authorize",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden semantic judge dependency: {forbidden}"
        );
    }
    for required in [
        "SEMANTIC_JUDGE_INPUT_SCHEMA",
        "SemanticJudgeEvaluator",
        "JudgeAvailability",
        "JudgeVerdict",
        "judge.unavailable",
        "judge.unavailable_not_pass",
        "judge.config_drift",
        "judge.result_invalid",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-35 boundary marker: {required}"
        );
    }
}
