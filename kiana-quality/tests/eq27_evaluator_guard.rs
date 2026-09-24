#[test]
fn evaluator_stays_pure_and_bounded() {
    let source = include_str!("../src/evaluator.rs");
    for forbidden in [
        "std::fs",
        "tokio::",
        "reqwest::",
        "Provider",
        "CapabilityBroker",
        "DaemonHost",
        "KianaHarness",
        "Command::new",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "FINDING_SCHEMA",
        "deny_unknown_fields",
        "MAX_FINDINGS",
        "MAX_FINDING_VALUE_BYTES",
        "MAX_FINDING_VALUE_DEPTH",
        "sort_findings",
        "evidence_ref",
        "redact_value",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-27 boundary marker: {required}"
        );
    }
}
