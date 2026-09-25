#[test]
fn notification_delivery_worker_is_a_bounded_plan_not_an_effect_loop() {
    let worker = include_str!("../src/notification_delivery.rs");
    let domain = include_str!("../../kiana-domain/src/notification_outbox.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/notification_outbox.rs");
    for marker in [
        "NotificationDeliveryWorker",
        "NotificationDeliveryPlan",
        "ClaimRequired",
        "Dispatch",
        "AwaitReceipt",
        "ReconcileUnknown",
        "NOTIFICATION_DELIVERY_WORKER_SCHEMA",
    ] {
        assert!(worker.contains(marker), "worker marker missing: {marker}");
    }
    for marker in [
        "NotificationOutboxRecord",
        "NotificationOutboxLease",
        "NotificationDispatchIntent",
        "lease_token",
        "notification_outbox_lease_fence_mismatch",
        "notification_outbox_receipt_mismatch",
    ] {
        assert!(
            domain.contains(marker),
            "outbox contract marker missing: {marker}"
        );
    }
    for marker in [
        "MemoryNotificationOutboxStore",
        "claim_next_notification",
        "mark_notification_submitted",
        "reconcile_notification_unknown",
        "Mutex",
    ] {
        assert!(
            eventlog.contains(marker),
            "outbox adapter marker missing: {marker}"
        );
    }
    for source in [worker, eventlog] {
        for forbidden in [
            "CapabilityBroker",
            "KianaHarness",
            "tokio::spawn",
            "reqwest",
            "std::net",
            "send(",
        ] {
            assert!(
                !source.contains(forbidden),
                "outbox effect boundary widened: {forbidden}"
            );
        }
    }
}
