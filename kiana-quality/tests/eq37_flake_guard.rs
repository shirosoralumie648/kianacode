#[test]
fn flake_classifier_stays_retry_once_and_quarantine_first() {
    let source = include_str!("../src/flake.rs");
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
            "forbidden flake classifier dependency: {forbidden}"
        );
    }
    for required in [
        "FLAKE_INPUT_SCHEMA",
        "FlakeClassifierEvaluator",
        "retry_once",
        "flake.flaky_counted_as_pass",
        "flake.quarantine_missing",
        "flake.infra_unclassified",
        "flake.infra_counted_as_result",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-37 boundary marker: {required}"
        );
    }
}
