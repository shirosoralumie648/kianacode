use kiana_core::{
    NotificationListRequest, NotificationStore, NotificationStoreError, NOTIFICATION_STORE_SCHEMA,
};
use kiana_domain::{
    EventId, HumanAction, HumanInboxKind, HumanTaskBridge, MessageId, Notification,
    NotificationChannel, NotificationMaterialization, RequestId, RuntimeEvent,
};
use serde_json::json;

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

fn committed_fixture(recipient: &str) -> RuntimeEvent {
    let notification = Notification::new(
        MessageId::new(),
        recipient,
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        1,
        None,
    )
    .unwrap();
    let action = HumanAction {
        id: "approve".to_owned(),
        label: "Approve".to_owned(),
        command: "approval".to_owned(),
        arguments: json!({"approval_id":"opaque"}),
        required_fields: Vec::new(),
    };
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "approval.requested",
        json!({"source":"eventlog","owner_id":recipient}),
    )
    .unwrap();
    let bridge = HumanTaskBridge::new(
        "task-09",
        "approval",
        "approval-09",
        1,
        DIGEST,
        event.event_id,
        1,
        recipient,
        500,
        9_000,
        vec!["event:evidence-09".to_owned()],
        vec![action.clone()],
    )
    .unwrap();
    let materialization = NotificationMaterialization::new(
        notification,
        event.event_id,
        1,
        HumanInboxKind::Approval,
        "Approval required",
        "A redacted approval is waiting",
        500,
        9_000,
        vec!["event:evidence-09".to_owned()],
        vec![action],
        Some(bridge),
    )
    .unwrap();
    let mut event = event;
    event.data["source_cursor"] = json!(1);
    event.data["notification_materialization"] = json!(materialization);
    event
}

#[test]
fn empty_and_unavailable_are_distinct_and_query_is_bounded() {
    let mut store = NotificationStore::new();
    let unavailable = store.list(NotificationListRequest {
        schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
        recipient_id: "actor-09".to_owned(),
        limit: 10,
        after_item_id: None,
        expected_source_cursor: None,
    });
    assert_eq!(
        unavailable,
        Err(NotificationStoreError::ProjectionUnavailable)
    );

    store
        .apply_committed(&[committed_fixture("actor-09")], 1)
        .unwrap();
    let page = store
        .list(NotificationListRequest {
            schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
            recipient_id: "actor-09".to_owned(),
            limit: 1,
            after_item_id: None,
            expected_source_cursor: Some(1),
        })
        .unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(!page.items[0].read);
    assert!(!page.items[0].acknowledged);

    let empty = store
        .list(NotificationListRequest {
            schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
            recipient_id: "actor-other".to_owned(),
            limit: 10,
            after_item_id: None,
            expected_source_cursor: Some(1),
        })
        .unwrap();
    assert!(empty.items.is_empty());
    assert!(store
        .list(NotificationListRequest {
            schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
            recipient_id: "actor-09".to_owned(),
            limit: 10,
            after_item_id: Some("notification:missing".to_owned()),
            expected_source_cursor: Some(1),
        })
        .is_err());
}

#[test]
fn mark_read_and_ack_are_projection_only_idempotent_mutations() {
    let mut store = NotificationStore::new();
    store
        .apply_committed(&[committed_fixture("actor-09")], 1)
        .unwrap();
    let page = store
        .list(NotificationListRequest {
            schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
            recipient_id: "actor-09".to_owned(),
            limit: 10,
            after_item_id: None,
            expected_source_cursor: Some(1),
        })
        .unwrap();
    let item_id = page.items[0].item.item_id.clone();
    let read = store.mark_read("actor-09", &item_id, 10).unwrap();
    assert!(read.changed);
    let replay = store.mark_read("actor-09", &item_id, 10).unwrap();
    assert!(!replay.changed);
    let ack = store.acknowledge("actor-09", &item_id, 11).unwrap();
    assert!(ack.acknowledged);
    assert!(store.mark_read("foreign", &item_id, 12).is_err());
    assert!(store.mark_read("actor-09", &item_id, 9).is_err());
}
