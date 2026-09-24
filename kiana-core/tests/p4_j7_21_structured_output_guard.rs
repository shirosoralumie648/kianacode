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
        "unsupported_response_schema_fails_before_send",
        "refusal_is_not_an_empty_structured_success",
        "truncated_json_is_never_repaired_silently",
        "structured_output_validates_against_requested_schema",
        "structured_output_and_tools_have_distinct_contracts",
        "cargo fmt --all --check",
        "cargo test -p kiana-provider --lib",
        "cargo test -p kiana-core --test p4_j7_21_structured_output_guard",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "structured-output CI marker missing: {marker}"
        );
    }
    assert!(!request.contains("repair_until_valid"));
    assert!(!response.contains("repair_until_valid"));
}
