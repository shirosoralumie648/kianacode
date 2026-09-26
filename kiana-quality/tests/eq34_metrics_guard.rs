#[test]
fn performance_cost_evaluator_stays_pure_and_bucketed() {
    let source = include_str!("../src/metrics.rs");
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
        "CostLedger",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden metrics evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "PERFORMANCE_COST_INPUT_SCHEMA",
        "PerformanceCostEvaluator",
        "CostBucket",
        "metrics.threshold_missing",
        "metrics.duration_exceeded",
        "metrics.tokens_exceeded",
        "metrics.tool_calls_exceeded",
        "metrics.cache_key_missing",
        "metrics.cost_unmeasured",
        "metrics.cost_unknown",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-34 boundary marker: {required}"
        );
    }
}
