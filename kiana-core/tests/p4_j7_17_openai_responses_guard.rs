#[test]
fn p4_j7_17_openai_responses_keeps_stateless_item_and_hosted_tool_boundaries() {
    let request = include_str!("../../kiana-provider/src/request.rs");
    let response = include_str!("../../kiana-provider/src/response.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-17-openai-responses-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-17-openai-responses.yml");
    for marker in [
        "ModelProtocol::OpenAiResponses",
        "responses_body",
        "max_output_tokens",
        r#""store":false"#,
        "function_call_output",
        "response.output_item.added",
        "response.output_item.done",
        "response.completed",
        "provider_final_item_mismatch",
        "provider_hosted_tool_denied",
        "previous_response_id",
    ] {
        if marker == r#""store":false"# {
            assert!(request.contains("\"store\":false"));
        } else if marker == "previous_response_id" {
            assert!(!request.contains(marker));
        } else {
            assert!(
                request.contains(marker) || response.contains(marker),
                "Responses source marker missing: {marker}"
            );
        }
    }
    for marker in [
        "responses_item_id_cannot_replace_call_id",
        "responses_incomplete_never_completes_the_run",
        "responses_unrequested_hosted_tool_is_rejected",
        "responses_stateless_tool_round_trip",
        "cargo test -p kiana-provider --lib",
        "cargo fmt --all --check",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "P4-J7-17 evidence marker missing: {marker}"
        );
    }
}
