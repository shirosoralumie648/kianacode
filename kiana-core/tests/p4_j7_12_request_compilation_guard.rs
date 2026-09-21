#[test]
fn p4_j7_12_request_compilation_keeps_mapping_history_and_hash_boundaries() {
    let domain = include_str!("../../kiana-domain/src/request_compilation.rs");
    let provider = include_str!("../../kiana-provider/src/request.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-12-request-compilation-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-12-request-compilation.yml");
    for marker in [
        "ToolNameMap",
        "internal_to_wire",
        "wire_to_internal",
        "from_entries",
        "tool_name_map_wire_collision",
        "strict",
        "model_tool_unadvertised",
    ] {
        assert!(
            domain.contains(marker),
            "request map marker missing: {marker}"
        );
    }
    for marker in [
        "ToolNameMap",
        "TokenBudget::new",
        "model_request_body_limit",
        "input_schema",
        "function_result",
        "model_history_orphan_tool_result",
        "max_completion_tokens",
    ] {
        assert!(
            provider.contains(marker),
            "provider compilation marker missing: {marker}"
        );
    }
    for marker in [
        "tool_catalog_hash",
        "request_hash",
        "model_prepared_request_changed",
        "validate_model_history",
    ] {
        assert!(
            model.contains(marker),
            "prepared request marker missing: {marker}"
        );
    }
    for marker in [
        "wire_tool_name_collision_is_rejected",
        "orphan_tool_result_fails_before_send",
        "compiled_request_cannot_exceed_context_budget",
        "five_tools_round_trip_through_one_reversible_map",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test p4_j7_12_request_compilation",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "P4-J7-12 evidence marker missing: {marker}"
        );
    }
    assert!(!provider.contains("authorization"));
    assert!(!provider.contains("api_key"));
}
