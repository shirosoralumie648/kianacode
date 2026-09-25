#[test]
fn notification_priority_is_a_server_time_projection_not_a_permission_path() {
    let priority = include_str!("../src/notification_priority.rs");
    let store = include_str!("../src/notification_store.rs");
    for marker in [
        "NotificationUrgency",
        "due_at_unix_ms",
        "source_cursor",
        "digest_group",
        "compare_notification_priority",
        "snooze",
        "snoozed_until_unix_ms",
    ] {
        assert!(
            priority.contains(marker) || store.contains(marker),
            "NM-11 marker missing: {marker}"
        );
    }
    for source in [priority, store] {
        for forbidden in [
            "CapabilityBroker",
            "KianaHarness",
            "tokio::spawn",
            "EventStorePort::append",
            "HumanTaskStatus::Decided",
            "approve(",
            "retry(",
        ] {
            assert!(
                !source.contains(forbidden),
                "NM-11 authority widened: {forbidden}"
            );
        }
    }
}
