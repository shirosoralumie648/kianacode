//! UI-06 source guard for bounded feed replay, explicit gaps and backpressure.

#[test]
fn feed_replay_stays_a_bounded_projection_and_never_an_execution_path() {
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let facade = include_str!("../../kiana-daemon/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/ui06-feed-replay-baseline.md");

    for marker in [
        "UI_FEED_CURSOR_SCHEMA",
        "UI_FEED_FRAME_SCHEMA",
        "UI_FEED_GAP_SCHEMA",
        "UiFeedCursorV1",
        "UiFeedGapReason",
        "UiFeedFrameKind",
        "UiFeedFrameV1",
        "cursor_digest",
        "snapshot_required",
        "SnapshotBoundary",
        "FEED_REPLAY_WINDOW",
        "UI_FEED_QUEUE_CAPACITY",
        "subscribe_feed_after",
        "RunStreamFeedError",
        "Backpressure",
        "ReplayExpired",
        "OldEpoch",
        "SequenceAhead",
        "feed_terminal_delta_forbidden",
        "feed_terminal_duplicate",
        "feed_backpressure_metrics",
        "feed_heartbeat",
    ] {
        assert!(
            protocol.contains(marker) || daemon.contains(marker) || facade.contains(marker),
            "UI-06 source marker missing: {marker}"
        );
    }

    for marker in [
        "snapshot boundary",
        "feed sequence",
        "replay window",
        "heartbeat",
        "backpressure",
        "slow_consumer",
        "terminal",
        "gap",
        "feature_status",
        "proof_level",
        "source",
    ] {
        assert!(baseline.contains(marker), "UI-06 baseline marker missing: {marker}");
    }

    assert!(daemon.contains("broadcast::channel(RUN_STREAM_CAPACITY)"));
    assert!(daemon.contains("while channel.history.len() > FEED_REPLAY_WINDOW"));
    assert!(daemon.contains("state.gap_frames = state.gap_frames.saturating_add(1)"));
    assert!(!daemon.contains("CapabilityBroker::new"));
    assert!(!facade.contains("KianaHarness::new"));
}
