#[test]
fn h11_tool_results_are_classified_before_model_feedback() {
    let domain = include_str!("../../kiana-domain/src/capabilities.rs");
    let harness = include_str!("../src/harness.rs");
    let fixtures = include_str!("h11_tool_observation.rs");
    let errors = include_str!("../../kiana-domain/src/errors.rs");
    for marker in [
        "ToolObservationStatus",
        "ToolObservationRepair",
        "TOOL_OBSERVATION_SCHEMA",
        "ToolObservation::from_result",
        "tool_denied_no_retry",
        "tool_cancelled_not_started",
        "result_unknown:tool_observation_unknown",
        "untrusted",
        "output_digest",
        "full_output_ref",
        "error_code",
        "unknown_result_never_triggers_automatic_retry",
        "tool_output_cannot_grant_permissions",
        "failed_test_observation_allows_bounded_model_fix",
        "requires_new_authorization",
    ] {
        assert!(
            domain.contains(marker)
                || harness.contains(marker)
                || fixtures.contains(marker)
                || errors.contains(marker),
            "H11 marker missing: {marker}"
        );
    }
    assert!(harness.contains("observation.model_text()"));
    assert!(harness.contains("ToolObservationStatus::Denied"));
    assert!(domain.contains("TOOL_OBSERVATION_MAX_SUMMARY"));
    assert!(domain.contains("!self.untrusted"));
    for forbidden in [
        "retry_unknown_result",
        "tool_output_grants_permission",
        "raw_capability_output_as_instruction",
        "unknown_effect_auto_retry",
    ] {
        assert!(
            !domain.contains(forbidden) && !harness.contains(forbidden),
            "forbidden H11 observation bypass: {forbidden}"
        );
    }
}
