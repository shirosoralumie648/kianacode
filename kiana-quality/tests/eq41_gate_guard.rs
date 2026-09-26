#[test]
fn quality_gate_keeps_config_and_decision_separate() {
    let source = include_str!("../src/gate.rs");
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
        "rollback",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden quality gate dependency: {forbidden}"
        );
    }
    for required in [
        "GATE_INPUT_SCHEMA",
        "QualityGateConfig",
        "QualityGateDecision",
        "config_digest",
        "decision_digest",
        "gate.config_update_mutated_old_version",
        "gate.pass_with_blocking_findings",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-41 boundary marker: {required}"
        );
    }
}
