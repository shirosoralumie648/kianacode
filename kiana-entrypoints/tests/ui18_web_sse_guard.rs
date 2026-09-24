#[test]
fn ui18_sse_is_deny_first_and_does_not_retry_side_effect_commands() {
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let events = web
        .split("async fn events(")
        .nth(1)
        .and_then(|rest| rest.split("fn stream_attach_state").next())
        .expect("events handler");
    for marker in [
        "authorize_sse(&app",
        "stream_cursor_from_request",
        "subscribe_run_after",
        "WEB_SSE_HEARTBEAT_INTERVAL",
        "stream_gap_sse_event_with_cursor",
        "stream_error_sse_event_with_cursor",
        "stream_heartbeat_sse_event",
    ] {
        assert!(events.contains(marker), "UI-18 event guard marker missing: {marker}");
    }
    assert!(!events.contains("harness_run::run_envelope_on_host"));
    assert!(!events.contains("harness_run::continue_envelope_on_host"));
    assert!(!events.contains("harness_run::cancel_envelope_on_host"));

    let reconnect = page
        .split("function scheduleEventStreamReconnect")
        .nth(1)
        .and_then(|rest| rest.split("async function requestStreamHydrate").next())
        .expect("reconnect state machine");
    assert!(reconnect.contains("SSE_MAX_RECONNECTS"));
    assert!(reconnect.contains("setTimeout"));
    assert!(reconnect.contains("ensureEventStream(session, false, true)"));
    assert!(!reconnect.contains("/api/run"));
    assert!(!reconnect.contains("submitCommand"));
    assert!(!reconnect.contains("api('/api/cancel'"));

    let send = page
        .split("async function sendPrompt")
        .nth(1)
        .and_then(|rest| rest.split("document.getElementById('send').onclick").next())
        .expect("command submission");
    assert!(send.contains("await ensureEventStream(sessionId, true)"));
    assert!(send.contains("api('/api/run'"));
}

#[test]
fn ui18_stream_payload_never_embeds_web_token_or_unbounded_buffer() {
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let helpers = web
        .split("fn run_stream_sse_event")
        .nth(1)
        .and_then(|rest| rest.split("async fn run_turn").next())
        .expect("SSE payload helpers");
    assert!(helpers.contains("MAX_WEB_SSE_EVENT_BYTES"));
    assert!(helpers.contains("stream_payload_too_large"));
    assert!(helpers.contains("WEB_SSE_SCHEMA"));
    assert!(!helpers.contains("web_token"));
    assert!(!helpers.contains("println!"));
    assert!(!page.contains("console.log(window.__KIANA_WEB_TOKEN__"));
    assert!(!page.contains("history.pushState({}, '', window.__KIANA_WEB_TOKEN__"));
}
