#[test]
fn p4_j7_15_anthropic_messages_keeps_native_terminal_and_usage_boundaries() {
    let response = include_str!("../../kiana-provider/src/response.rs");
    let request = include_str!("../../kiana-provider/src/request.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-15-anthropic-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-15-anthropic.yml");
    for marker in [
        "ModelProtocol::AnthropicMessages",
        "message_start",
        "content_block_start",
        "content_block_delta",
        "message_delta",
        "message_stop",
        "provider_usage_regressed",
        "provider_private_replay_not_configured",
        "provider_unknown_required_event",
        "anthropic_json_object_requires_schema",
    ] {
        assert!(
            response.contains(marker) || request.contains(marker),
            "Anthropic source marker missing: {marker}"
        );
    }
    for marker in [
        "anthropic_stop_reason_without_message_stop_is_incomplete",
        "anthropic_stream_error_after_text_never_dispatches_tools",
        "anthropic_unknown_required_block_fails_closed",
        "anthropic_text_tool_result_and_final_answer_round_trip",
        "anthropic_usage_updates_are_cumulative",
        "cargo test -p kiana-provider --lib",
        "cargo fmt --all --check",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "P4-J7-15 evidence marker missing: {marker}"
        );
    }
    assert!(!response.contains("DeepSeek"));
}
