#[test]
fn web_sync_source_contract() {
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/p2-m5-01-web-sync-baseline.md");

    for marker in [
        "/api/state",
        "/api/events",
        "ui_snapshot",
        "subscribe_run_after",
        "stream_cursor_from_request",
        "last-event-id",
        "stream_gap",
        "stream_error",
        "snapshot_required_after_stream_gap",
        "subscription_attached_after_run_started",
        "stream_sequence_gap",
        "stream_subscription_lagged",
        "stream_closed_before_terminal",
        "ensureEventStream",
        "EventSource",
        "onopen",
        "last_event_id",
        "streamCursors",
        "observedUiCursor",
        "refreshIssued",
        "refreshApplied",
        "clearStreamProjection",
        "streamIncomplete",
        "markStreamIncomplete",
        "receipt authoritative",
        "RunStreamSubscription",
        "gap",
        "epoch",
        "sequence",
        "UiCursor",
    ] {
        assert!(
            web.contains(marker)
                || page.contains(marker)
                || daemon.contains(marker)
                || protocol.contains(marker)
                || baseline.contains(marker),
            "web sync marker missing: {marker}"
        );
    }

    assert!(web.contains("Subscribe before returning the SSE response headers"));
    assert!(web.contains("snapshot_required_after_stream_gap"));
    assert!(web.contains("stream_cursor_from_request"));
    assert!(page.contains("source.addEventListener('stream_gap', handleStreamGap)"));
    assert!(page.contains("refresh().catch(() => {})"));
    assert!(page.contains(
        "if (envelope.sequence !== old.sequence + 1) markStreamIncomplete('stream_sequence_gap')"
    ));
    assert!(daemon.contains("Missing deltas are signalled as a gap"));
    assert!(daemon.contains("terminal = Some(envelope.clone())"));
    assert!(!web.contains("ModelClient"));
    assert!(!web.contains("CapabilityBroker"));
}
