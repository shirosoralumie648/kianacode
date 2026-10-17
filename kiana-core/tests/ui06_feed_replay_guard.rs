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
        "snapshot_boundary_after_gap",
        "current_sequence, current_ui_cursor",
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
        assert!(
            baseline.contains(marker),
            "UI-06 baseline marker missing: {marker}"
        );
    }

    assert!(daemon.contains("broadcast::channel(RUN_STREAM_CAPACITY)"));
    assert!(daemon.contains("while channel.history.len() > FEED_REPLAY_WINDOW"));
    assert!(daemon.contains("state.gap_frames = state.gap_frames.saturating_add(1)"));
    assert!(!daemon.contains("CapabilityBroker::new"));
    assert!(!facade.contains("KianaHarness::new"));
}

/// UI-06 completion slice: the fault matrix, the bounded budget and the manual lane must stay
/// wired to the fixtures that prove them, and the feed must never grow a second execution path.
#[test]
fn ui06_feed_fault_matrix_stays_wired_to_the_lane_and_the_auto_shards() {
    let run_stream = include_str!("../../kiana-daemon/src/run_stream.rs");
    let bridge = include_str!("../../kiana-daemon/src/notification_stream.rs");
    let daemon_fixture = include_str!("../../kiana-daemon/tests/ui06_feed_replay.rs");
    let bridge_fixture = include_str!("../../kiana-daemon/tests/nm13_notification_stream.rs");
    let fault_plan = include_str!("../../kiana-daemon/tests/fixtures/ui06-feed-replay.json");
    let baseline = include_str!("../../docs/roadmap/ui06-feed-replay-baseline.md");
    let lane = include_str!("../../.github/workflows/ui06-feed-replay.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // 1. deny-first classification stays in the daemon source, not just in the fixtures.
    for marker in [
        "UiFeedGapReason::TerminalRetention",
        "UiFeedGapReason::SequenceAhead",
        "UiFeedGapReason::Backpressure",
        "UiFeedGapReason::ReplayExpired",
        "feed_terminal_delta_forbidden",
        "feed_terminal_duplicate",
        "must never fail a committed fact",
    ] {
        assert!(
            run_stream.contains(marker),
            "UI-06 daemon marker missing: {marker}"
        );
    }
    for marker in [
        "NotificationStreamDisposition::Unknown",
        "Unknown,",
        "cursor.terminal = true",
    ] {
        assert!(
            bridge.contains(marker) || run_stream.contains(marker),
            "UI-06 state-machine marker missing: {marker}"
        );
    }

    // 2. every fixture this slice relies on is present in its file and named in the baseline.
    for fixture in [
        "resumed_feed_replays_nothing_when_the_cursor_is_already_current",
        "retired_feed_window_reports_terminal_retention_and_requires_snapshot",
        "stalled_consumer_keeps_the_queue_and_history_bounded",
    ] {
        assert!(
            run_stream.contains(fixture),
            "UI-06 daemon unit fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "UI-06 baseline must name the daemon unit fixture: {fixture}"
        );
    }
    for fixture in [
        "ui06_feed_fault_plan_binds_the_bounded_budget_and_the_deny_matrix",
        "live_feed_keeps_the_boundary_the_sequence_and_the_retained_terminal",
        "gap_frames_use_the_server_cursor_and_hydrate_through_the_snapshot_page",
        "a_stalled_consumer_gaps_but_never_blocks_committed_facts",
        "restarted_host_requires_hydration_and_keeps_the_same_ledger",
    ] {
        assert!(
            daemon_fixture.contains(fixture),
            "UI-06 daemon fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "UI-06 baseline must name the daemon fixture: {fixture}"
        );
    }
    assert!(bridge_fixture
        .contains("terminal_retention_and_unknown_frames_never_collapse_into_a_delta"));
    assert!(baseline.contains("terminal_retention_and_unknown_frames_never_collapse_into_a_delta"));

    // 3. the declared budget is the source budget; a drift in either direction fails here.
    assert!(run_stream.contains("const RUN_STREAM_CAPACITY: usize = 256;"));
    assert!(run_stream.contains("const FEED_REPLAY_WINDOW: usize = 128;"));
    assert!(fault_plan.contains("\"queue_capacity\": 256"));
    assert!(fault_plan.contains("\"replay_window\": 128"));
    for reason in [
        "sequence_gap",
        "replay_expired",
        "old_epoch",
        "instance_changed",
        "sequence_ahead",
        "backpressure",
        "terminal_retention",
    ] {
        assert!(
            fault_plan.contains(reason),
            "UI-06 fault plan must keep the {reason} denial"
        );
    }

    // 4. the daemon fixtures exercise the composed host instead of a second store or loop.
    assert!(daemon_fixture.contains("host.handle("));
    assert!(daemon_fixture.contains("host.subscribe_run_feed_after("));
    assert!(daemon_fixture.contains(".ui_snapshot_page(&hydration)"));
    assert!(daemon_fixture.contains("host.persisted_events()"));
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        ".append(",
        "run_stream::",
        "project_ui_snapshot",
        "ControlPlane::new",
    ] {
        assert!(
            !daemon_fixture.contains(forbidden),
            "UI-06 daemon fixture must not open a second path: {forbidden}"
        );
    }
    for fixture_source in [daemon_fixture, bridge_fixture, run_stream] {
        assert!(!fixture_source.contains("#[ignore"));
        assert!(!fixture_source.contains("--ignored"));
    }

    // 5. the manual lane stays dispatch-only, read-only and runs the four UI-06 targets.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(!lane.contains("  push:"));
    assert!(!lane.contains("  pull_request:"));
    assert!(lane.contains("permissions:\n  contents: read"));
    assert!(lane.contains("cargo fmt --all --check"));
    assert!(lane.contains("cargo fetch --locked"));
    for target in [
        "-p kiana-protocol --test ui06_feed_contract",
        "-p kiana-daemon --lib run_stream::tests",
        "-p kiana-daemon --test ui06_feed_replay",
        "-p kiana-daemon --test nm13_notification_stream",
        "-p kiana-core --test ui06_feed_replay_guard",
        "--no-run",
    ] {
        assert!(lane.contains(target), "UI-06 lane target missing: {target}");
    }
    assert!(lane.contains("--test-threads=1"));
    assert!(!lane.contains("continue-on-error"));

    // 6. auto coverage: the guard target stays in a core shard and kiana-daemon keeps running
    //    every test target in the crate, which is how ui06_feed_replay is discovered.
    assert!(shards.contains("\"ui06_feed_replay_guard\""));
    assert!(shards
        .contains("\"kiana-daemon\": {\n  \"crate\": \"kiana-daemon\",\n  \"targets\": null\n }"));
    assert!(baseline.contains(".github/workflows/ui06-feed-replay.yml"));
}
