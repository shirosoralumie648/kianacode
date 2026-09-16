#[test]
fn cap04_uses_typed_state_outcome_and_error_boundaries() {
    let states = include_str!("../../kiana-domain/src/states.rs");
    let domain_capabilities = include_str!("../../kiana-domain/src/capabilities.rs");
    let normalizer = include_str!("../../kiana-domain/src/actions.rs");
    let invocation = include_str!("../src/invocation_projection.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    let events = include_str!("../src/events.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let baseline = include_str!("../../docs/roadmap/capability-state-baseline.md");

    for marker in [
        "CapabilityExecutionState",
        "can_transition_via",
        "CapabilityProcessState",
        "CapabilityResultDimensions",
        "dimensions",
        "execution_state",
    ] {
        assert!(
            states.contains(marker)
                || domain_capabilities.contains(marker)
                || normalizer.contains(marker),
            "missing CAP-04 typed marker {marker}"
        );
    }
    assert!(invocation.contains("result_from_value"));
    assert!(invocation.contains("invocation_state_transition_invalid"));
    assert!(attempts.contains("ForeignAttemptResult"));
    assert!(attempts.contains("is_terminal_result_event"));
    assert!(normalizer.contains("shell_exit"));
    assert!(contracts.contains("kiana.capability-result-dimensions.v1"));
    assert!(protocol.contains("CapabilityResultDimensions"));
    assert!(protocol.contains("failure_policy"));
    assert!(daemon.contains("CapabilityErrorCode::from_reason"));
    for source in [events, attempts, lifecycle] {
        assert!(
            !source.contains("contains(\"result_unknown")
                && !source.contains("starts_with(\"result_unknown"),
            "control state classification must not use result_unknown substring matching"
        );
    }
    for fixture in [
        "terminal_execution_cannot_transition_to_success_again",
        "unknown_effect_cannot_be_projected_as_cancelled",
        "foreign_attempt_result_is_rejected",
        "nonzero_shell_exit_remains_a_structured_tool_result",
    ] {
        assert!(
            include_str!("cap04_state.rs").contains(fixture)
                || include_str!("../../kiana-domain/tests/cap04_state.rs").contains(fixture),
            "missing CAP-04 fixture {fixture}"
        );
    }
    assert!(baseline.contains("CapabilityResultDimensions"));
    assert!(baseline.contains("result_unknown"));
    assert!(baseline.contains("CLI"));
}
