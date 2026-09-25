use kiana_core::{
    classify_notification, compare_notification_priority, NotificationListRequest,
    NotificationStore, NotificationStoreError, NotificationUrgency, NOTIFICATION_STORE_SCHEMA,
};
use kiana_domain::{HumanAction, HumanInboxItem, HumanInboxKind};
use serde_json::json;

fn item(id: &str, kind: HumanInboxKind, due: u64, cursor: u64) -> HumanInboxItem {
    HumanInboxItem {
        item_id: id.to_owned(),
        kind,
        title: "redacted notification".to_owned(),
        source_ref: format!("event:{id}"),
        run_id: None,
        detail: json!({"due_at_unix_ms": due, "source_cursor": cursor}),
        actions: vec![HumanAction {
            id: "review".to_owned(),
            label: "Review".to_owned(),
            command: "notification.review".to_owned(),
            arguments: json!({"opaque":"ref"}),
            required_fields: Vec::new(),
        }],
    }
}

#[test]
fn urgency_due_cursor_and_digest_group_sort_deterministically() {
    let critical =
        classify_notification(&item("critical", HumanInboxKind::Incident, 900, 2)).unwrap();
    let high = classify_notification(&item("high", HumanInboxKind::Approval, 100, 1)).unwrap();
    assert_eq!(critical.urgency, NotificationUrgency::Critical);
    assert_eq!(high.urgency, NotificationUrgency::High);
    assert_eq!(
        compare_notification_priority(&critical, &high),
        std::cmp::Ordering::Less
    );
    assert!(!critical.digest_group.is_empty());
}

#[test]
fn malformed_due_or_cursor_fails_closed() {
    let mut malformed = item("bad", HumanInboxKind::Question, 0, 1);
    malformed.detail = json!({"source_cursor":1});
    assert!(classify_notification(&malformed).is_err());
    malformed.detail = json!({"due_at_unix_ms":1,"source_cursor":0});
    assert!(classify_notification(&malformed).is_err());
}

#[test]
fn snooze_without_projection_is_unavailable_not_authority() {
    let mut store = NotificationStore::new();
    assert_eq!(
        store.snooze("actor-11", "notification:missing", 10, 20),
        Err(NotificationStoreError::ProjectionUnavailable)
    );
    let _ = NotificationListRequest {
        schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
        recipient_id: "actor-11".to_owned(),
        limit: 1,
        after_item_id: None,
        expected_source_cursor: None,
    };
}
