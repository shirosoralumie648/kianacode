#[test]
fn aggregation_evaluator_keeps_sample_and_confidence_gates_explicit() {
    let source = include_str!("../src/aggregation.rs");
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
            "forbidden aggregation evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "AGGREGATION_INPUT_SCHEMA",
        "AggregationEvaluator",
        "absolute_limit_milli",
        "relative_limit_milli",
        "minimum_samples",
        "confidence_required",
        "aggregation.insufficient_sample",
        "aggregation.false_pass",
        "aggregation.confidence_missing",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-36 boundary marker: {required}"
        );
    }
}
