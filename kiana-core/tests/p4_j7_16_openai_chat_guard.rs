#[test]
fn p4_j7_16_openai_chat_keeps_native_sse_identity_and_finish_boundaries() {
    let request = include_str!("../../kiana-provider/src/request.rs");
    let response = include_str!("../../kiana-provider/src/response.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-16-openai-chat-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-16-openai-chat.yml");
    for marker in [
        "ModelProtocol::OpenAiChat",
        "max_completion_tokens",
        "stream_options",
        "provider_choice_count_invalid",
        "provider_tool_id_changed",
        "provider_tool_name_changed",
        "provider_tool_json_invalid",
        "provider_done_without_finish",
        "choices.is_empty()",
        "provider_hosted_tool_denied",
    ] {
        assert!(
            request.contains(marker) || response.contains(marker),
            "OpenAI Chat source marker missing: {marker}"
        );
    }
    for marker in [
        "chat_done_without_valid_choice_finish_is_incomplete",
        "chat_interleaved_tools_cannot_swap_ids",
        "chat_malformed_arguments_never_become_empty_object",
        "chat_usage_only_chunk_is_retained",
        "cargo test -p kiana-provider --lib",
        "cargo fmt --all --check",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "P4-J7-16 evidence marker missing: {marker}"
        );
    }
    assert!(!request.contains("previous_response_id"));
    assert!(!request.contains("hosted_tools"));
}
