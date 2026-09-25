#[test]
fn web_notification_routes_reuse_auth_feed_and_have_no_execution_authority() {
    let web = include_str!("../src/web.rs");
    let presenter = include_str!("../src/web_notifications.rs");
    for marker in [
        "/api/notifications",
        "/api/notifications/events",
        "async fn notifications(",
        "async fn notification_events(",
        "authorize_mutation(&app",
        "authorize_sse(&app",
        "subscribe_run_feed_after",
        "notification_stream_cursor_from_request",
        "Last-Event-ID",
        "WEB_NOTIFICATION_SSE_HEARTBEAT_INTERVAL",
        "notification_gap_frame",
    ] {
        assert!(web.contains(marker), "NM-15 web marker missing: {marker}");
    }
    for marker in [
        "present_notification_page",
        "present_notification_frame",
        "encode_notification_cursor",
        "decode_notification_cursor",
        "actions_are_display_only_until_re_admitted_by_control_plane",
        "feed_payload_redacted",
        "MAX_WEB_NOTIFICATION_SSE_EVENT_BYTES",
    ] {
        assert!(
            presenter.contains(marker),
            "NM-15 presenter marker missing: {marker}"
        );
    }
    let page_handler = web
        .split("async fn notifications(")
        .nth(1)
        .and_then(|rest| rest.split("async fn artifact(").next())
        .expect("notification page handler");
    assert!(page_handler.contains("notification_page"));
    assert!(!page_handler.contains("harness_run::run_envelope_on_host"));
    assert!(!page_handler.contains("harness_run::cancel_envelope_on_host"));
    let stream_handler = web
        .split("async fn notification_events(")
        .nth(1)
        .and_then(|rest| rest.split("fn notification_sse_frame_event").next())
        .expect("notification SSE handler");
    assert!(!stream_handler.contains("harness_run::run_envelope_on_host"));
    assert!(!stream_handler.contains("harness_run::continue_envelope_on_host"));
    assert!(!stream_handler.contains("harness_run::cancel_envelope_on_host"));
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "reqwest::Client",
        "std::process::Command",
    ] {
        assert!(
            !presenter.contains(forbidden),
            "NM-15 presenter authority widened: {forbidden}"
        );
    }
}
