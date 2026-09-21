#[test]
fn p4_j7_14_uses_one_accumulator_and_requires_complete_protocol_terminals() {
    let response = include_str!("../../kiana-provider/src/response.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-14-accumulator-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-14-accumulator.yml");
    for marker in [
        "pub(crate) struct Accumulator",
        "provider_frame_after_terminal",
        "provider_event_limit",
        "provider_delta_after_block_stop",
        "provider_finish_with_open_blocks",
        "provider_duplicate_finish",
        "provider_stream_incomplete",
        "provider_final_text_mismatch",
        "provider_final_tool_mismatch",
        "ModelProtocol::AnthropicMessages",
        "ModelProtocol::OpenAiChat",
        "ModelProtocol::OpenAiResponses",
        "ModelProtocol::OllamaChat",
        "ModelProtocol::GeminiInteractions",
    ] {
        assert!(
            response.contains(marker),
            "accumulator marker missing: {marker}"
        );
    }
    for marker in [
        "ModelFinish::parse",
        "require_complete",
        "model_output_truncated",
        "model_transport_incomplete",
    ] {
        assert!(
            model.contains(marker),
            "finish contract marker missing: {marker}"
        );
    }
    assert!(transport.contains("provider_stream_incomplete"));
    assert!(transport.contains("if accumulator.push(&data, sink)?"));
    for marker in [
        "closed_tool_block_cannot_receive_more_arguments",
        "terminal_with_open_blocks_is_rejected",
        "length_finish_never_authorizes_partial_tool_arguments",
        "stream_eof_is_not_completion",
        "terminal_finishes_without_socket_eof",
        "cargo test -p kiana-provider --lib",
        "cargo fmt --all --check",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "P4-J7-14 evidence marker missing: {marker}"
        );
    }
    assert_eq!(response.matches("pub(crate) struct Accumulator").count(), 1);
}
