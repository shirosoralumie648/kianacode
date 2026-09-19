#[test]
fn cli_tty_web_share_the_versioned_stream_and_snapshot_contract() {
    let workbench = include_str!("../src/workbench_chat.rs");
    let tty = include_str!("../src/stream_render.rs");
    let web = include_str!("../src/web.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    for surface in [workbench, tty, web] {
        assert!(surface.contains("RunStreamEnvelope"));
        assert!(surface.contains("RunStreamEvent"));
        assert!(surface.contains("ResponseEnvelope"));
    }
    for marker in [
        "advance_cursor",
        "stream_epoch_changed",
        "stream_sequence_gap",
        "RunDisplayState",
        "UiSnapshot",
    ] {
        assert!(
            protocol.contains(marker),
            "missing display contract marker: {marker}"
        );
    }
    assert!(daemon.contains("RunStreamBus"));
    assert!(daemon.contains("RunStreamEnvelope"));
}

#[test]
fn display_surfaces_do_not_turn_local_time_into_completion() {
    let workbench = include_str!("../src/workbench_chat.rs");
    let web = include_str!("../src/web.rs");
    assert!(workbench.contains("ExecutionStatus::Completed"));
    assert!(web.contains("ExecutionStatus::Completed"));
    assert!(workbench.contains("response.status"));
    assert!(web.contains("response.status"));
    assert!(!workbench.contains("Instant::now().is_completed"));
    assert!(!web.contains("Date.now() === completed"));
}
