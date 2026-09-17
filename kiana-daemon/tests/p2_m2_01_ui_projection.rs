#[test]
fn stale_ui_action_is_rejected_by_epoch() {
    let protocol_ui = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon_stream = include_str!("../src/run_stream.rs");
    let daemon = include_str!("../src/lib.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let web_page = include_str!("../../kiana-entrypoints/src/web_page.html");
    let baseline = include_str!("../../docs/roadmap/p2-m2-01-ui-projection-baseline.md");

    for marker in [
        "UiSnapshot",
        "UiAction",
        "UiCursor",
        "UiSnapshotV1",
        "UiActionV1",
        "UiCursorV1",
        "snapshot_cursor",
        "expected_epoch",
        "expected_cursor",
        "pending_actions",
        "stream_cursor",
        "ui_cursor",
        "claim_ui_action",
        "stale_ui_action",
        "ui_action_replayed",
        "ui_action_invalid",
        "subscribe_after",
        "snapshot_required_after_stream_gap",
        "stream_sequence_gap",
        "ui_snapshot",
        "ui_events",
        "run_terminal_conflict",
        "project_run_state",
        "pending",
        "active_session_id",
        "authority_epoch",
        "snapshot_cursor",
        "UI_SNAPSHOT_SCHEMA",
        "UI_ACTION_SCHEMA",
        "UiActionResult",
        "UiRetryDisposition",
    ] {
        assert!(
            protocol_ui.contains(marker)
                || protocol.contains(marker)
                || daemon_stream.contains(marker)
                || daemon.contains(marker)
                || web.contains(marker)
                || workbench.contains(marker)
                || web_page.contains(marker)
                || baseline.contains(marker),
            "UI projection marker missing: {marker}"
        );
    }

    assert!(daemon_stream.contains("if action.expected_epoch != self.epoch"));
    assert!(daemon_stream.contains("ui_action_stale"));
    assert!(daemon_stream.contains("ui_action_replayed"));
    assert!(daemon.contains("pub async fn ui_snapshot"));
    assert!(daemon.contains("project_run_state"));
    assert!(daemon.contains("pending_actions"));
    assert!(web.contains("claim_ui_headers"));
    assert!(web.contains("snapshot_required_after_stream_gap"));
    assert!(web_page.contains("markStreamIncomplete"));
    assert!(workbench.contains("subscribe_run"));
    assert!(!daemon_stream.contains("CapabilityBroker"));
    assert!(!web.contains("ModelClient"));
}
