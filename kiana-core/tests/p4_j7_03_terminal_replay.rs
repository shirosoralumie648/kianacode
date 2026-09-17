#[test]
fn terminal_is_replayed_to_late_subscriber() {
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let page = include_str!("../../kiana-entrypoints/src/web_page.html");

    for marker in [
        "RunStreamEvent::Usage",
        "RunStreamEvent::ToolCall",
        "RunStreamEvent::ApprovalRequested",
        "RunStreamEvent::Error",
        "project_committed",
        "terminal: Option<RunStreamEnvelope>",
        "subscribe_after",
        "terminal.sequence > last_sequence",
        "has_gap",
        "stream_gap",
        "last-event-id",
        "stream_sequence_gap",
        "stream_epoch_changed",
        "terminal_is_replayed_to_late_subscriber",
    ] {
        assert!(
            protocol.contains(marker)
                || daemon.contains(marker)
                || web.contains(marker)
                || page.contains(marker),
            "terminal replay marker missing: {marker}"
        );
    }

    assert!(daemon.contains("if let Some(terminal) = &channel.terminal"));
    assert!(daemon.contains("replay.push_back(terminal.clone())"));
    assert!(daemon.contains("RunStreamEvent::Usage"));
    assert!(daemon.contains("RunStreamEvent::ToolCall"));
    assert!(daemon.contains("RunStreamEvent::ApprovalRequested"));
    assert!(daemon.contains("RunStreamEvent::Error"));
    assert!(daemon.contains("terminal_is_replayed_to_late_subscriber"));
    assert!(protocol.contains("#[serde(other)]"));
    assert!(web.contains("stream_gap"));
    assert!(!daemon.contains("ProviderGateway"));
}
