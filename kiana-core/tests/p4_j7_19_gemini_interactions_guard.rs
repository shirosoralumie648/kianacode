#[test]
fn p4_j7_19_gemini_interactions_stays_stateless_and_fail_closed() {
    let request = include_str!("../../kiana-provider/src/request.rs");
    let response = include_str!("../../kiana-provider/src/response.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-19-gemini-interactions-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-19-gemini-interactions.yml");

    for marker in [
        "ModelProtocol::GeminiInteractions",
        "gemini_body",
        "gemini_generation_config",
        "require_gemini_streaming",
        "body[\"stream\"] = json!(true)",
        "body[\"store\"] = json!(false)",
        "generation_config",
        "max_output_tokens",
        "function_result",
        "call_id",
        "is_error",
        "kiana.tool-observation.v1",
        "gemini_stateless_function_result_round_trip",
        "gemini_interaction_parameters_are_resubmitted_each_turn",
    ] {
        assert!(
            request.contains(marker),
            "Gemini request marker missing: {marker}"
        );
    }
    for marker in [
        "interaction.created",
        "interaction.status_update",
        "step.start",
        "step.delta",
        "step.stop",
        "interaction.completed",
        "arguments_delta",
        "provider_step_index_out_of_order",
        "provider_step_after_terminal_status",
        "provider_usage_total_inconsistent",
        "gemini_requires_action_is_not_run_completion",
        "gemini_incomplete_function_arguments_never_dispatch",
        "gemini_steps_cannot_follow_terminal_status_update",
        "gemini_requires_action_status_must_match_function_steps",
        "gemini_generate_content_events_are_not_accepted_as_interactions",
        "gemini_malformed_or_inconsistent_usage_fails_closed",
        "gemini_optional_step_usage_does_not_override_terminal_usage",
    ] {
        assert!(
            response.contains(marker),
            "Gemini response marker missing: {marker}"
        );
    }
    assert!(!request.contains("generateContent"));
    assert!(!response.contains("generateContent"));
    for marker in [
        "gemini_requires_action_is_not_run_completion",
        "gemini_incomplete_function_arguments_never_dispatch",
        "gemini_generate_content_events_are_not_accepted_as_interactions",
        "gemini_stateless_function_result_round_trip",
        "gemini_interaction_parameters_are_resubmitted_each_turn",
        "cargo fmt --all --check",
        "cargo test -p kiana-provider --lib --locked gemini_",
        "cargo test -p kiana-core --test p4_j7_19_gemini_interactions_guard",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "Gemini CI evidence marker missing: {marker}"
        );
    }
}
