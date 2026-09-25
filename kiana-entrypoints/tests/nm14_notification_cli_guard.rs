#[test]
fn notification_cli_presenter_has_no_execution_authority() {
    let source = include_str!("../src/notification_cli.rs");
    let cli = include_str!("../src/cli.rs");
    for marker in [
        "CLI_NOTIFICATION_VIEW_SCHEMA",
        "CLI_RUN_STATUS_VIEW_SCHEMA",
        "present_notification_page",
        "present_run_status",
        "query_original",
        "actions_are_display_only",
        "CLI_NOTIFICATION_MAX_ITEMS",
    ] {
        assert!(source.contains(marker), "NM-14 marker missing: {marker}");
    }
    assert!(!cli.contains("NotificationDeliveryWorker"));
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "reqwest::Client",
        "std::process::Command",
        "send(",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-14 presenter authority widened: {forbidden}"
        );
    }
}
