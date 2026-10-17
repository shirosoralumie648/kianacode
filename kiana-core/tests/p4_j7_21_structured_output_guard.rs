#[test]
fn p4_j7_21_structured_output_is_explicit_and_fail_closed() {
    let domain = include_str!("../../kiana-domain/src/model.rs");
    let request = include_str!("../../kiana-provider/src/request.rs");
    let response = include_str!("../../kiana-provider/src/response.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-21-structured-output-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-21-structured-output.yml");

    for marker in [
        "pub enum ModelResponseFormat",
        "JsonObject",
        "JsonSchema { name: String, schema: Value }",
        "pub structured: Option<Value>",
        "model_response_schema_invalid",
    ] {
        assert!(domain.contains(marker), "domain marker missing: {marker}");
    }
    for marker in [
        "spec.response_format.validate()",
        "model_structured_output_unsupported",
        "anthropic_json_object_requires_schema",
        "model_structured_output_protocol_unsupported",
        "check_schema(schema, 0)",
        "parse_structured_output",
        "model_structured_output_empty",
        "model_structured_output_invalid_json",
        "model_structured_output_object_required",
    ] {
        assert!(request.contains(marker), "request marker missing: {marker}");
    }
    for marker in [
        "model_refused",
        "model_output_truncated",
        "result.output.structured",
        "structured_output_validates_and_is_exposed_separately",
        "refusal_length_empty_and_invalid_json_are_distinct",
        "structured_output_and_tool_calls_are_distinct",
    ] {
        assert!(
            response.contains(marker),
            "response marker missing: {marker}"
        );
    }
    assert!(harness.contains("structured"));
    assert!(harness.contains("ModelPurpose::OutputRepair"));
    for marker in [
        "invalid_response_schema_remains_terminal",
        "refusal_length_empty_and_invalid_json_are_distinct",
        "structured_output_errors_are_typed_format_repair_without_running_a_repair",
        "structured_output_validates_and_is_exposed_separately",
        "structured_output_and_tool_calls_are_distinct",
        "cargo fmt --all --check",
        "cargo test -p kiana-provider --lib",
        "cargo test -p kiana-core --test p4_j7_21_structured_output_guard",
    ] {
        assert!(
            baseline.contains(marker)
                || workflow.contains(marker)
                || request.contains(marker)
                || response.contains(marker),
            "structured-output CI marker missing: {marker}"
        );
    }

    let integration = include_str!("../../kiana-provider/tests/p4_j7_21_structured_output.rs");
    for marker in [
        "structured_response_format_wire_matrix_is_recorded_per_protocol",
        "unsupported_structured_format_is_rejected_before_encoding",
        "apply_response_format",
    ] {
        assert!(request.contains(marker), "request marker missing: {marker}");
    }
    for marker in [
        "refusal_is_not_an_empty_structured_success",
        "truncated_json_is_never_repaired_silently",
    ] {
        assert!(
            response.contains(marker),
            "response marker missing: {marker}"
        );
    }
    for marker in [
        "unsupported_response_schema_fails_before_send",
        "structured_output_and_tools_have_distinct_contracts",
        "prepare_call",
    ] {
        assert!(
            integration.contains(marker),
            "integration marker missing: {marker}"
        );
    }
    // The integration fixture stops at request preparation: it must never open a socket,
    // admit or complete a call, or be skipped.
    for marker in [
        "complete_admitted",
        "TcpListener",
        "tokio::net",
        "#[ignore]",
    ] {
        assert!(
            !integration.contains(marker),
            "integration fixture must not contain {marker}"
        );
    }
    for marker in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "--test p4_j7_21_structured_output",
    ] {
        assert!(
            workflow.contains(marker),
            "workflow marker missing: {marker}"
        );
    }
    assert!(!workflow.contains("continue-on-error"));
    assert!(!request.contains("repair_until_valid"));
    assert!(!response.contains("repair_until_valid"));
}
