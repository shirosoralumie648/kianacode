#[test]
fn capability_input_boundary_is_shared_by_runner_core_broker_and_daemon() {
    let catalog = include_str!("../../kiana-domain/src/tool_catalog.rs");
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let runner = include_str!("../../kiana-runner/src/tools.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let provider = include_str!("../../kiana-provider/src/response.rs");
    let provider_transport = include_str!("../../kiana-provider/src/transport.rs");
    let baseline = include_str!("../../docs/roadmap/capability-input-baseline.md");

    for marker in [
        "TOOL_JSON_MAX_BYTES",
        "TOOL_JSON_MAX_DEPTH",
        "TOOL_JSON_MAX_ITEMS",
        "parse_bounded_json",
        "json_duplicate_key",
        "validate_schema_contract",
        "additionalProperties",
    ] {
        assert!(
            catalog.contains(marker),
            "missing input/schema marker {marker}"
        );
    }
    assert!(actions.contains("canonical_action_input_digest"));
    assert!(actions.contains("PreparedAction"));
    assert!(actions.contains("action_tool_alias_conflict"));
    assert!(actions.contains("action_command_required"));
    assert!(actions.contains("action_numeric_argument_invalid"));
    assert!(runner.contains("validate_tool_arguments"));
    assert!(runner.contains("TOOL_SHELL =>"));
    assert!(capabilities.contains("stamp_request_identity"));
    assert!(capabilities.contains("PreparedAction::new"));
    assert!(broker.contains("capability_action_not_prepared"));
    assert!(daemon.contains("command_argv"));
    assert!(daemon.contains("NUL") || daemon.contains("\\0"));
    assert!(provider.contains("kiana_domain::parse_bounded_json(raw.as_bytes())"));
    assert!(provider.contains("struct UniqueProviderJson(Value)"));
    assert!(provider.contains("fn parse_provider_json(raw: &[u8], invalid_code: &str)"));
    assert!(provider
        .contains("duplicate_openai_tool_argument_fields_are_rejected_before_value_collapse"));
    assert!(provider_transport.contains("response::{decode_json, Accumulator}"));
    assert!(provider_transport.contains("decode_json(&bytes, &prepared)"));
    for fixture in [
        "object_form_tool_arguments_reject_duplicate_keys_before_model_tool_call",
        "streamed_object_form_tool_arguments_reject_duplicate_keys_before_model_tool_call",
        "object_form_tool_arguments_accept_nested_values_and_keep_envelope_limit_separate",
        "object_form_tool_arguments_over_the_bounded_input_limit_are_rejected",
        "streamed_object_form_tool_arguments_accept_valid_nested_values",
    ] {
        assert!(provider.contains(fixture), "missing provider fixture {fixture}");
        assert!(baseline.contains(fixture), "missing baseline fixture {fixture}");
    }
    assert!(baseline.contains("reserved_authority_fields_cannot_change_execution_scope"));
    assert!(baseline.contains("schema_depth_and_reference_limits_fail_before_dispatch"));
    assert!(baseline.contains("conflicting_mcp_tool_aliases_are_rejected"));
    assert!(baseline.contains("equivalent_json_inputs_have_the_same_digest"));
    assert!(baseline.contains("execution_affecting_input_changes_change_digest"));
}
