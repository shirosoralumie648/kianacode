use kiana_domain::{
    DeliveryReceipt, DeliveryReceiptStatus, MessageId, Notification, NotificationChannel,
    NotificationOutboxRecord, NotificationOutboxState, SubscriptionId,
};
use kiana_eventlog::MemoryNotificationOutboxStore;
use kiana_ports::NotificationOutboxStore;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn notification() -> Notification {
    Notification::new(
        MessageId::new(),
        "actor-08",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        1,
        None,
    )
    .expect("notification")
}

fn record() -> NotificationOutboxRecord {
    NotificationOutboxRecord::new(notification(), SubscriptionId::new(), 7, 100)
        .expect("outbox record")
}

#[test]
fn fixture_declares_durable_outbox_deny_first_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/nm08-notification-outbox.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.notification-outbox-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("crash")));
}

#[tokio::test]
async fn outbox_claim_submit_ack_is_fenced_and_replayable() {
    let store = MemoryNotificationOutboxStore::new();
    let inserted = store.enqueue_notification(record()).await.unwrap();
    assert_eq!(inserted.state, NotificationOutboxState::Pending);
    let (claimed, lease) = store
        .claim_next_notification("worker-a", 7, 200, 100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.state, NotificationOutboxState::Claimed);
    let intent = claimed.dispatch_intent(&lease).unwrap();
    intent.validate().unwrap();

    store
        .mark_notification_submitted(&lease, 210)
        .await
        .unwrap();
    let receipt = DeliveryReceipt::new(
        claimed.notification.notification_id,
        claimed.attempt.delivery_attempt_id,
        claimed.notification.recipient_id.clone(),
        DeliveryReceiptStatus::Acknowledged,
        220,
        None,
    )
    .unwrap();
    let acknowledged = store
        .acknowledge_notification(&lease, &receipt, 220)
        .await
        .unwrap();
    assert_eq!(acknowledged.state, NotificationOutboxState::Acknowledged);
    assert_eq!(
        store
            .claim_next_notification("worker-b", 7, 230, 100)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn stale_worker_and_expired_claim_fail_closed_or_reclaim_safely() {
    let store = MemoryNotificationOutboxStore::new();
    let inserted = store.enqueue_notification(record()).await.unwrap();
    let (_, lease) = store
        .claim_next_notification("worker-a", 7, 200, 10)
        .await
        .unwrap()
        .unwrap();
    let mut forged = lease.clone();
    forged.worker_id = "worker-b".to_owned();
    forged.lease_digest = forged.digest();
    assert!(store
        .mark_notification_submitted(&forged, 205)
        .await
        .is_err());

    let (reclaimed, new_lease) = store
        .claim_next_notification("worker-b", 7, 211, 10)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.state, NotificationOutboxState::Claimed);
    assert_ne!(
        reclaimed.attempt.delivery_attempt_id,
        inserted.attempt.delivery_attempt_id
    );
    assert_ne!(new_lease.lease_token, lease.lease_token);
    assert!(store
        .mark_notification_submitted(&lease, 212)
        .await
        .is_err());
}

#[tokio::test]
async fn submitted_lease_expiry_is_reconcile_unknown_not_automatic_retry() {
    let store = MemoryNotificationOutboxStore::new();
    store.enqueue_notification(record()).await.unwrap();
    let (_claimed, lease) = store
        .claim_next_notification("worker-a", 7, 200, 10)
        .await
        .unwrap()
        .unwrap();
    store
        .mark_notification_submitted(&lease, 205)
        .await
        .unwrap();
    let unknown = store
        .reconcile_notification_unknown(&lease, 209)
        .await
        .unwrap();
    assert_eq!(unknown.state, NotificationOutboxState::Unknown);
}
