#[test]
fn notification_stream_reuses_run_stream_without_a_second_bus() {
    let source = include_str!("../src/notification_stream.rs");
    let run_stream = include_str!("../src/run_stream.rs");
    for marker in [
        "NotificationStreamBridge",
        "NotificationStreamCursor",
        "SnapshotBoundary",
        "SnapshotRequired",
        "UiFeedGapReason",
        "Heartbeat",
        "Disposed",
        "RunStreamBus",
        "UiFeedFrameV1",
        // NM-13 completion slice: the reconnect window, the event id memory, the foreign run
        // refusal and the terminal query alarm must exist in the bridge itself.
        "NOTIFICATION_STREAM_MERGE_WINDOW",
        "NOTIFICATION_STREAM_SEEN_EVENT_IDS",
        "merge window",
        "pending_merge",
        "seen_event_ids",
        "ForeignRun",
        "terminal_query_required",
        "take_merged_frames",
        "Replayed",
    ] {
        assert!(
            source.contains(marker) || run_stream.contains(marker),
            "NM-13 marker missing: {marker}"
        );
    }
    for forbidden in [
        "broadcast::channel",
        "tokio::spawn",
        "CapabilityBroker",
        "KianaHarness",
        "reqwest",
        "send(",
        "EventStore",
        "ControlPlane",
        "std::fs",
        "std::process",
        "Command::new",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-13 second-loop/effect marker: {forbidden}"
        );
    }
}

/// NM-13 completion slice: the merge window, the dedupe memory and the terminal query alarm must
/// stay wired to the fixtures, the manual lane and the automatic shard that prove them.
#[test]
fn notification_stream_lane_keeps_the_bridge_and_its_guard_wired() {
    let source = include_str!("../src/notification_stream.rs");
    let fixture = include_str!("nm13_notification_stream.rs");
    let plan = include_str!("fixtures/nm13-notification-stream.json");
    let lane = include_str!("../../.github/workflows/nm13-notification-stream.yml");
    let baseline = include_str!("../../docs/roadmap/nm13-notification-stream-baseline.md");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // The deny-first fixtures stay named and enabled.
    for name in [
        "the_first_attach_hydrates_before_it_renders_and_keeps_no_window",
        "lagged_and_retired_gaps_never_report_completion",
        "a_late_socket_keeps_the_retained_terminal_and_a_retired_window_stays_visible",
        "a_recovery_window_merges_only_the_frames_the_snapshot_does_not_contain",
        "a_recovery_window_never_delivers_an_event_twice",
        "an_overflowing_merge_window_fails_closed_instead_of_reporting_a_partial_merge",
        "a_foreign_run_payload_is_refused_before_any_state_moves",
        "a_feed_that_ends_without_a_terminal_raises_the_query_alarm",
        "the_event_id_memory_is_bounded_and_the_oldest_id_is_forgotten_first",
    ] {
        assert!(fixture.contains(name), "NM-13 fixture missing: {name}");
    }
    assert!(!fixture.contains("#[ignore"));
    assert!(!fixture.contains("--ignored"));
    assert!(plan.contains("\"schema\": \"kiana.notification-stream-fixture.v1\""));
    for marker in [
        "merge window",
        "foreign run",
        "terminal",
        "proof_level",
        "source",
    ] {
        assert!(
            plan.contains(marker) || baseline.contains(marker),
            "NM-13 fixture/baseline marker missing: {marker}"
        );
    }
    assert!(baseline.contains("NM-13"));

    // The manual lane is dispatch-only, read-only and runs both targets plus their compile steps.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(!lane.contains("  push:"));
    assert!(!lane.contains("  pull_request:"));
    assert!(lane.contains("permissions:\n  contents: read"));
    assert!(lane.contains("cargo fetch --locked"));
    assert!(lane.contains("cargo fmt --all --check"));
    assert!(lane
        .contains("-p kiana-daemon --test nm13_notification_stream --locked -- --test-threads=1"));
    assert!(lane.contains(
        "-p kiana-daemon --test nm13_notification_stream_guard --locked -- --test-threads=1"
    ));
    assert!(lane.contains("-p kiana-daemon --test nm13_notification_stream --no-run --locked"));
    assert!(
        lane.contains("-p kiana-daemon --test nm13_notification_stream_guard --no-run --locked")
    );
    assert!(!lane.contains("continue-on-error"));

    // Automatic coverage stays the whole-crate kiana-daemon shard, which discovers both targets.
    assert!(shards
        .contains("\"kiana-daemon\": {\n  \"crate\": \"kiana-daemon\",\n  \"targets\": null\n }"));
    assert!(source.contains("NOTIFICATION_STREAM_MERGE_WINDOW: usize = 64;"));
    assert!(source.contains("NOTIFICATION_STREAM_SEEN_EVENT_IDS: usize = 256;"));
}
