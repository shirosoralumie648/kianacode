#[test]
fn p4_j7_18_ollama_ndjson_keeps_completion_identity_and_local_scope() {
    let request = include_str!("../../kiana-provider/src/request.rs");
    let response = include_str!("../../kiana-provider/src/response.rs");
    let config = include_str!("../../kiana-provider/src/config.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-18-ollama-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-18-ollama.yml");

    for marker in [
        "ModelProtocol::OllamaChat",
        "ollama_body",
        "tool_name",
        "model_history_orphan_tool_result",
    ] {
        assert!(
            request.contains(marker),
            "Ollama request marker missing: {marker}"
        );
    }
    for marker in [
        "provider_message_invalid",
        "provider_done_flag_invalid",
        "provider_stream_incomplete",
        "provider_returned_unadvertised_tool",
        "provider_usage_invalid",
        "provider_timing_invalid",
        "ollama:{}:{ordinal}",
        "load_duration_ns",
        "generation_duration_ns",
        "ollama_streams_before_model_completion",
        "ollama_tool_result_continuation_uses_stable_local_ids",
        "ollama_load_latency_is_distinct_from_generation_latency",
        "ollama_partial_or_malformed_observations_fail_closed",
    ] {
        assert!(
            response.contains(marker),
            "Ollama response marker missing: {marker}"
        );
    }
    for marker in [
        "ollama_load_timeout_ms",
        "ollama_load_timeout_provider_mismatch",
        "ollama_load_timeout_invalid",
        "TransportLimits::default",
        "ollama_load_timeout_is_provider_scoped_and_bounded",
    ] {
        assert!(
            config.contains(marker),
            "Ollama config marker missing: {marker}"
        );
    }
    assert!(harness.contains("\"provider_timing\""));
    assert!(config.contains(".retry(reqwest::retry::never())"));
    assert!(baseline.contains("ollama_same_name_tools_keep_distinct_invocations"));
    for marker in [
        "ollama_eof_without_done_is_incomplete",
        "ollama_same_name_tools_keep_distinct_invocations",
        "ollama_unknown_tools_support_is_not_assumed",
        "ollama_streams_before_model_completion",
        "ollama_tool_result_continuation_uses_stable_local_ids",
        "ollama_load_latency_is_distinct_from_generation_latency",
        "cargo test -p kiana-provider --lib",
        "cargo test -p kiana-provider --test p4_j7_08_config",
        "cargo fmt --all --check",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "P4-J7-18 CI marker missing: {marker}"
        );
    }
    assert!(!request.contains("/api/pull"));
    assert!(!request.contains("/api/create"));
}
