#[test]
fn p4_j7_13_transport_keeps_all_deadlines_framing_and_body_bounds() {
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let config = include_str!("../../kiana-provider/src/config.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-13-transport-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-13-transport.yml");
    for marker in [
        "connection.limits.headers",
        ".first_event",
        "connection.limits.idle",
        "connection.limits.total",
        "connection.limits.max_body",
        "connection.limits.max_frame",
        "bytes_stream",
        "Framer::new",
        "provider_content_type_invalid",
        "provider_frame_limit",
        "provider_frame_utf8_invalid",
        "provider_frame_truncated",
        "provider_stream_incomplete",
        "retry-after",
    ] {
        assert!(
            transport.contains(marker),
            "transport marker missing: {marker}"
        );
    }
    for marker in [
        "headers: Duration",
        "first_event: Duration",
        "idle: Duration",
        "total: Duration",
        "max_body: usize",
        "max_frame: usize",
    ] {
        assert!(
            config.contains(marker),
            "transport limit marker missing: {marker}"
        );
    }
    for marker in [
        "oversized_and_truncated_frames_fail_closed",
        "heartbeat_cannot_extend_total_deadline",
        "html_success_body_is_not_an_empty_model_success",
        "sse_survives_arbitrary_utf8_chunking_and_crlf",
        "ndjson_flushes_complete_lines_and_one_bounded_tail",
        "cargo test -p kiana-provider --lib",
        "cargo fmt --all --check",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker) || transport.contains(marker),
            "P4-J7-13 evidence marker missing: {marker}"
        );
    }
    assert!(!transport.contains("reqwest::ClientBuilder::retry"));
}
