#[test]
fn runtime_evaluator_stays_pure_and_bounded() {
    let source = include_str!("../src/runtime.rs");
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
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden runtime evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "RUNTIME_CORRECTNESS_INPUT_SCHEMA",
        "RuntimeCorrectnessEvaluator",
        "runtime.event_order_invalid",
        "runtime.invocation_correlation_mismatch",
        "runtime.terminal_duplicate",
        "runtime.retry_order_invalid",
        "runtime.approval_unmatched",
        "runtime.cancel_unfenced",
        "runtime.unknown_retry_forbidden",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-28 boundary marker: {required}"
        );
    }
}
