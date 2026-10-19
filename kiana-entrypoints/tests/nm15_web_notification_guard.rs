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

/// The page cursor is consumed before the projection read, so a replay, a foreign tab or a stale
/// source cursor is refused without ever serving a page. The `notification.*` command names the
/// page advertises never gain a Web handler: they remain display-only strings.
#[test]
fn notification_page_cursor_is_consumed_before_the_read_and_grants_no_command_authority() {
    let web = include_str!("../src/web.rs");
    let handler = web
        .split("async fn notifications(")
        .nth(1)
        .and_then(|rest| rest.split("async fn artifact(").next())
        .expect("notification page handler");
    let token_index = handler
        .find("consume_page_cursor")
        .expect("the page handler consumes the single-use cursor");
    let read_index = handler
        .find("notification_page(")
        .expect("the page handler reads the projection");
    assert!(
        token_index < read_index,
        "the single-use cursor must be consumed before the projection read"
    );
    // The server's own next boundary is issued only after a successful read, and only when the
    // page is full: a partial page must not mint a cursor that has nowhere to go.
    let issue_index = handler
        .find("issue_page_cursor")
        .expect("the page handler issues the next cursor");
    assert!(issue_index > read_index);
    assert!(handler.contains("if let Some(next_after)"));
    assert!(handler.contains("payload[\"next_page_token\"] = Value::Null"));
    // A committed source cursor that is not durable is a stable 503, never a raw port string.
    assert!(handler.contains("notification_source_cursor_error"));
    assert!(web.contains("notification_projection_unavailable"));
    assert!(!handler.contains("notification.approve"));
    assert!(!handler.contains("harness_run::run_envelope_on_host"));

    // No route or handler dispatches a notification command: only the Human Inbox command names
    // are recognized by the ControlPlane command path.
    for forbidden in [
        "\"/api/notifications/resolve\"",
        "\"/api/notifications/approve\"",
        "\"notification.resolve\"",
        "notification.approve\" =>",
    ] {
        assert!(
            !web.contains(forbidden),
            "the Web notification surface must not route a command: {forbidden}"
        );
    }
}
