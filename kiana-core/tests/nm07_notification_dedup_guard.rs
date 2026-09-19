//! NM-07 source guard: deduplication stops at one notification intent and never dispatches.

#[test]
fn notification_dedup_is_an_atomic_intent_boundary() {
    let domain = include_str!("../../kiana-domain/src/notifications.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/notification_dedup.rs");

    for marker in [
        "NotificationDedupRequest",
        "NotificationDedupRecord",
        "content_digest",
        "subscription_revision",
        "notification_dedup_content_conflict",
    ] {
        assert!(
            domain.contains(marker),
            "NM-07 domain marker missing: {marker}"
        );
    }
    for marker in [
        "NotificationDedupStore",
        "claim_notification",
        "compare_and_swap_notification",
        "NotificationDedupOutcome",
    ] {
        assert!(
            ports.contains(marker),
            "NM-07 port marker missing: {marker}"
        );
    }
    for marker in [
        "MemoryNotificationDedupStore",
        "notification_dedup_stale_revision",
        "notification_dedup_notification_identity_conflict",
        "Mutex<NotificationDedupState>",
    ] {
        assert!(
            eventlog.contains(marker),
            "NM-07 adapter marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBrokerPort::execute",
        "DeliveryWorker",
        "tokio::spawn",
        "send(",
    ] {
        assert!(
            !eventlog.contains(forbidden),
            "NM-07 dedup adapter must not dispatch effects: {forbidden}"
        );
    }
}
