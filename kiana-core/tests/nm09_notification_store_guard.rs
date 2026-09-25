#[test]
fn notification_store_is_a_bounded_projection_and_query_boundary() {
    let source = include_str!("../src/notification_store.rs");
    let materializer = include_str!("../src/notification_materializer.rs");
    for marker in [
        "NotificationStore",
        "NotificationListRequest",
        "NotificationPage",
        "NOTIFICATION_STORE_MAX_PAGE_SIZE",
        "ProjectionUnavailable",
        "CursorStale",
        "mark_read",
        "acknowledge",
        "human_inbox_items",
        "apply_committed",
    ] {
        assert!(
            source.contains(marker) || materializer.contains(marker),
            "NM-09 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "EventStorePort::append",
        "tokio::spawn",
        "std::fs",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-09 authority widened: {forbidden}"
        );
    }
}
