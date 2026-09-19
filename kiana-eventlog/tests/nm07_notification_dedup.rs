use kiana_domain::{
    MessageId, Notification, NotificationChannel, NotificationDedupRequest, NotificationStatus,
};
use kiana_eventlog::MemoryNotificationDedupStore;
use kiana_ports::{NotificationDedupOutcome, NotificationDedupStore};

fn notification(message_id: MessageId) -> Notification {
    Notification::new(
        message_id,
        "builder",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        100,
        200,
        7,
        None,
    )
    .expect("valid notification")
}

fn request(key: &str, message_id: MessageId) -> NotificationDedupRequest {
    NotificationDedupRequest::new(key, notification(message_id), None).expect("valid dedup request")
}

#[tokio::test]
async fn at_least_once_claims_fold_to_one_original_notification() {
    let store = MemoryNotificationDedupStore::new();
    let message_id = MessageId::new();
    let (left, right) = tokio::join!(
        store.claim_notification(request("event-1/builder/in-app", message_id)),
        store.claim_notification(request("event-1/builder/in-app", message_id)),
    );

    let mut claimed = 0;
    let mut replayed = 0;
    let mut notification_ids = Vec::new();
    for result in [left, right] {
        match result.expect("claim or replay") {
            NotificationDedupOutcome::Claimed(record) => {
                claimed += 1;
                notification_ids.push(record.notification.notification_id);
            }
            NotificationDedupOutcome::Replayed(record) => {
                replayed += 1;
                notification_ids.push(record.notification.notification_id);
            }
        }
    }
    assert_eq!(claimed, 1);
    assert_eq!(replayed, 1);
    assert_eq!(notification_ids[0], notification_ids[1]);
}

#[tokio::test]
async fn same_key_different_digest_and_stale_revision_are_rejected() {
    let store = MemoryNotificationDedupStore::new();
    let first = match store
        .claim_notification(request("event-2/builder/in-app", MessageId::new()))
        .await
        .expect("first claim")
    {
        NotificationDedupOutcome::Claimed(record) => record,
        NotificationDedupOutcome::Replayed(_) => panic!("first request must claim"),
    };

    let different = request("event-2/builder/in-app", MessageId::new());
    let conflict = store
        .claim_notification(different)
        .await
        .expect_err("different content must not overwrite");
    assert!(conflict.to_string().contains("content_conflict"));

    let stale = NotificationDedupRequest::new(
        "event-2/builder/in-app",
        first.notification.clone(),
        Some(first.revision + 1),
    )
    .expect("stale request is well-formed");
    let stale_error = store
        .claim_notification(stale)
        .await
        .expect_err("stale revision must fail closed");
    assert!(stale_error.to_string().contains("stale_revision"));
}

#[tokio::test]
async fn concurrent_compare_and_swap_has_one_winner() {
    let store = MemoryNotificationDedupStore::new();
    let first = match store
        .claim_notification(request("event-3/builder/in-app", MessageId::new()))
        .await
        .expect("first claim")
    {
        NotificationDedupOutcome::Claimed(record) => record,
        NotificationDedupOutcome::Replayed(_) => panic!("first request must claim"),
    };
    let mut notification = first.notification.clone();
    notification
        .transition_status(NotificationStatus::Queued)
        .expect("pending notification can be queued");
    let next = first
        .advance(first.revision, notification)
        .expect("valid next revision");

    let (left, right) = tokio::join!(
        store.compare_and_swap_notification(&first.dedup_key, first.revision, next.clone()),
        store.compare_and_swap_notification(&first.dedup_key, first.revision, next),
    );
    assert_eq!(left.is_ok() as u8 + right.is_ok() as u8, 1);
    assert_eq!(left.is_err() as u8 + right.is_err() as u8, 1);
}
