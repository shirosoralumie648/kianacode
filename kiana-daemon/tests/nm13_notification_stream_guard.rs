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
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-13 second-loop/effect marker: {forbidden}"
        );
    }
}
