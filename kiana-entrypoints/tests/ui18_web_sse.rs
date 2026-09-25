use serde_json::Value;

#[test]
fn ui18_fixture_captures_bounded_sse_and_gap_hydrate_contract() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ui18-web-sse.json"))
        .expect("valid UI-18 SSE fixture");
    assert_eq!(fixture["schema"], "kiana.web-sse.v1");
    assert_eq!(fixture["transport"], "sse");
    for field in ["id", "event", "schema", "epoch", "sequence"] {
        assert!(fixture["event_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == field));
    }
    assert_eq!(fixture["last_event_id"]["header"], "Last-Event-ID");
    assert_eq!(fixture["last_event_id"]["query_fallback"], "last_event_id");
    assert_eq!(fixture["last_event_id"]["max_bytes"], 256);
    assert_eq!(fixture["limits"]["frame_bytes"], 262_144);
    assert_eq!(fixture["limits"]["browser_text_bytes"], 262_144);
    assert_eq!(fixture["limits"]["reconnect_max_attempts"], 6);
    for event in [
        "delta",
        "heartbeat",
        "stream_gap",
        "stream_error",
        "terminal",
    ] {
        assert!(fixture["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == event));
    }
    for denial in [
        "duplicate_sequence",
        "sequence_gap",
        "old_epoch",
        "cursor_conflict",
        "oversized_frame",
        "oversized_frame_preserves_cursor_context",
        "sse_token_in_payload_or_log",
        "reconnect_resubmits_command",
        "terminal_then_delta",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == denial));
    }
}

#[test]
fn ui18_source_exposes_server_cursor_and_bounded_read_only_stream() {
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    for marker in [
        "WEB_SSE_SCHEMA",
        "MAX_WEB_SSE_EVENT_BYTES",
        "MAX_WEB_SSE_CURSOR_BYTES",
        "WEB_SSE_HEARTBEAT_INTERVAL",
        "last-event-id",
        "stream_cursor_conflict",
        "stream_cursor_invalid",
        "stream_heartbeat_sse_event",
        "stream_gap_sse_event_with_cursor",
        "snapshot_required_after_stream_gap",
        "bounded_sse_data",
        "cursor.epoch.clone()",
        "subscribe_run_after",
        "DaemonHost",
        "receipt",
    ] {
        assert!(web.contains(marker), "UI-18 web marker missing: {marker}");
    }
    for marker in [
        "SSE_RECONNECT_BASE_MS",
        "SSE_RECONNECT_MAX_MS",
        "SSE_MAX_RECONNECTS",
        "MAX_STREAM_TEXT_BYTES",
        "scheduleEventStreamReconnect",
        "requestStreamHydrate",
        "stream_sequence_gap",
        "stream_epoch_changed",
        "stream_reconnect_exhausted",
        "Last-Event-ID",
        "source.addEventListener('heartbeat', handleStreamHeartbeat)",
        "source.addEventListener('stream_gap', handleStreamGap)",
        "source.addEventListener('stream_error', handleStreamError)",
        "x-kiana-web-token",
    ] {
        assert!(page.contains(marker), "UI-18 page marker missing: {marker}");
    }
    assert!(page.contains("await ensureEventStream(sessionId, true)"));
    assert!(!page.contains("scheduleEventStreamReconnect(session, '/api/run'"));
}
