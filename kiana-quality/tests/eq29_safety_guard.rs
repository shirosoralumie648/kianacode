#[test]
fn capability_safety_evaluator_stays_pure_and_deny_first() {
    let source = include_str!("../src/safety.rs");
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
            "forbidden capability evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "CAPABILITY_SAFETY_INPUT_SCHEMA",
        "CapabilitySafetyEvaluator",
        "safety.action_schema_invalid",
        "safety.grant_capability_missing",
        "safety.grant_scope_expanded",
        "safety.policy_not_allow",
        "safety.hook_not_allow",
        "safety.effect_forbidden",
        "safety.secret_effect",
        "SafetyFinalStatus",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-29 boundary marker: {required}"
        );
    }
}
