#[test]
fn shadow_admission_keeps_ttl_sample_and_grant_fences() {
    let source = include_str!("../src/shadow.rs");
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
            "forbidden shadow evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "SHADOW_INPUT_SCHEMA",
        "ShadowAdmission",
        "ShadowEvaluator",
        "sample_limit",
        "expires_at_unix_ms",
        "shadow.rollback_missing",
        "shadow.rollback_grant_changed",
        "shadow.ttl_expired",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-44 boundary marker: {required}"
        );
    }
}
