#[test]
fn daemon_does_not_gain_a_second_ui_execution_loop() {
    let daemon = include_str!("../src/lib.rs");
    let client = include_str!("../../kiana-client/src/typed.rs");
    for marker in [
        "ui_snapshot_page",
        "subscribe_run_feed_after",
        "admit_ui_action",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon facade marker missing: {marker}"
        );
    }
    for marker in [
        "QueryClient",
        "FeedClient",
        "ActionClient",
        "ArtifactClient",
    ] {
        assert!(
            client.contains(marker),
            "typed facade marker missing: {marker}"
        );
    }
    assert!(!client.contains("DaemonHost"));
    assert!(!client.contains("ControlPlane::new"));
}
