//! NM-20 daemon source guard: the notification stream bridge is a bounded display/cursor
//! adapter, the channel fixtures drive it directly, and the daemon shard runs every target.
//!
//! This guard is a source check: it fails if the bridge grows an effect path, a second bus, an
//! unbounded buffer, or if the channel fixture target or the manual lane loses its coverage.

#[test]
fn notification_stream_bridge_stays_bounded_and_effect_free() {
    let bridge = include_str!("../src/notification_stream.rs");
    let fixtures = include_str!("nm20_notification_channel_faults.rs");

    for marker in [
        "NotificationStreamBridge",
        "NOTIFICATION_STREAM_MERGE_WINDOW",
        "NOTIFICATION_STREAM_SEEN_EVENT_IDS",
        "NotificationStreamDisposition::SnapshotRequired",
        "NotificationStreamDisposition::Unknown",
        "NotificationStreamDisposition::Disposed",
        "terminal_query_required",
        "ForeignRun",
        "TerminalUpdate",
        "buffer_merge_frame",
        "merge_pending_frames",
    ] {
        assert!(
            bridge.contains(marker),
            "NM-20 bridge marker missing: {marker}"
        );
    }

    for case in ["SlowSubscriber", "MultiSubscriberIsolation"] {
        assert!(
            fixtures.contains(case),
            "NM-20 channel case missing from the daemon fixture: {case}"
        );
    }
    for refusal in [
        "notification_stream_backpressure_fail_closed",
        "notification_stream_subscriber_isolation",
        "nm20_slow_subscriber_partial_merge_reported",
        "nm20_isolation_gap_became_a_terminal",
    ] {
        assert!(
            fixtures.contains(refusal),
            "NM-20 channel refusal missing: {refusal}"
        );
    }

    // The bridge renders and asks for hydration; it must never execute, deliver or authorize.
    for forbidden in [
        "CapabilityBroker",
        "reqwest::Client",
        "std::process::Command",
        "write_event(",
        "send(",
        "tokio::spawn",
        "EventStore",
        "ControlPlane",
        "std::fs::write",
    ] {
        assert!(
            !bridge.contains(forbidden),
            "NM-20 bridge widened the execution surface: {forbidden}"
        );
    }
    // Bounded window: an overflow clears the buffer instead of keeping an unbounded queue.
    assert!(bridge.contains("pending_merge.clear()"));
    assert!(bridge.contains("self.pending_merge.len() >= NOTIFICATION_STREAM_MERGE_WINDOW"));
}

#[test]
fn notification_channel_fixtures_are_included_in_the_daemon_shard_and_lane() {
    let shards = include_str!("../../scripts/ci/test-shards.json");
    assert!(
        shards.contains("\"kiana-daemon\""),
        "the daemon shard must exist for the channel fixtures"
    );

    let lane = include_str!("../../.github/workflows/nm20-notification-faults.yml");
    for marker in [
        "cargo test -p kiana-daemon --test nm20_notification_channel_faults --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test nm20_notification_channel_faults_guard --locked -- --test-threads=1",
        "cargo test -p kiana-core --test nm20_notification_fault_harness --locked -- --test-threads=1",
        "cargo test -p kiana-core --test nm20_notification_fault_harness_guard --locked -- --test-threads=1",
    ] {
        assert!(
            lane.contains(marker),
            "NM-20 manual lane marker missing: {marker}"
        );
    }
}
