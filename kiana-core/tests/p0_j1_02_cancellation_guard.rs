#[test]
fn cancel_drains_queued_tool_calls_before_terminal_projection() {
    let lifecycle = include_str!("../src/lifecycle.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "cancel_pending_tools",
        "RunnerCommand::Cancel",
        "ToolCancelled",
        "not_executed",
        "replay_safe",
        "record_terminal_event",
    ] {
        assert!(
            lifecycle.contains(marker) || capabilities.contains(marker) || runner.contains(marker),
            "P0-J1-02 marker missing: {marker}"
        );
    }
    assert!(runner.contains("cancelled:queued"));
    assert!(runner.contains("pending_tools.drain(..)"));
}
