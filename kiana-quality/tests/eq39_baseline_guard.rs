#[test]
fn baseline_registry_stays_expiry_and_digest_bound() {
    let source = include_str!("../src/baseline.rs");
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
            "forbidden baseline evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "BASELINE_INPUT_SCHEMA",
        "BaselineRegistry",
        "BaselineEvaluator",
        "baseline.stale",
        "baseline.incompatible",
        "refresh_provenance",
        "expires_at_unix_ms",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-39 boundary marker: {required}"
        );
    }
}
