use kiana_core::NotificationMaterializer;
use kiana_domain::{
    EventId, HumanAction, HumanInboxKind, HumanTaskBridge, MessageId, Notification,
    NotificationChannel, NotificationMaterialization, RequestId, RuntimeEvent,
};
use serde_json::json;

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

fn fixture(recipient: &str, cursor: u64) -> (RuntimeEvent, NotificationMaterialization) {
    let notification = Notification::new(
        MessageId::new(),
        recipient,
        None,
        vec!["global".to_owned()],
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
        arguments: json!({"approval_id": "opaque"}),
        required_fields: Vec::new(),
    };
    let event = RuntimeEvent::new(
        RequestId::new(),
        cursor,
        "approval.requested",
        json!({"source":"eventlog","owner_id":recipient}),
    )
    .unwrap();
    let bridge = HumanTaskBridge::new(
        "task-1",
        "approval",
        "approval-1",
        1,
        DIGEST,
        event.event_id,
        cursor,
        recipient,
        500,
        9_000,
        vec!["event:evidence-1".to_owned()],
        vec![action.clone()],
    )
    .unwrap();
    let materialization = NotificationMaterialization::new(
        notification,
        event.event_id,
        cursor,
        HumanInboxKind::Approval,
        "Approval required",
        "A redacted approval is waiting",
        500,
        9_000,
        vec!["event:evidence-1".to_owned()],
        vec![action],
        Some(bridge),
    )
    .unwrap();
    let mut event = event;
    event.data["source_cursor"] = json!(cursor);
    event.data["notification_materialization"] = json!(materialization);
    (event, materialization)
}

#[test]
fn materializer_requires_committed_source_and_complete_materialization() {
    let (mut event, materialization) = fixture("actor-1", 1);
    event.data["notification_materialization"] = json!({
        "schema": materialization.schema,
        "notification": materialization.notification,
        "source_event_id": EventId::new(),
        "source_cursor": 0,
        "kind": "approval",
        "title": "Approval required",
        "redacted_summary": "A redacted approval is waiting",
        "due_at_unix_ms": 500,
        "expires_at_unix_ms": 9000,
        "evidence_refs": [],
        "actions": [],
        "human_task": null,
        "materialization_digest": DIGEST,
    });
    let mut materializer = NotificationMaterializer::new();
    assert!(materializer.apply_committed(&[event], 1).is_err());
    assert_eq!(materializer.source_cursor(), 0);
}

#[test]
fn materializer_rejects_cursor_gaps_and_replays_exactly() {
    let (first, _) = fixture("actor-1", 1);
    let mut materializer = NotificationMaterializer::new();
    materializer
        .apply_committed(std::slice::from_ref(&first), 1)
        .unwrap();
    let before = materializer.clone();
    assert!(materializer.apply_committed(&[], 2).is_err());
    assert_eq!(materializer, before);
    materializer
        .apply_committed(std::slice::from_ref(&first), 1)
        .unwrap();
    assert_eq!(materializer, before);

    let (mut conflict, _) = fixture("actor-1", 1);
    conflict.event_id = first.event_id;
    conflict.data["source_cursor"] = json!(1);
    conflict.data["notification_materialization"]["redacted_summary"] =
        json!("different redacted summary");
    assert!(materializer.apply_committed(&[conflict], 1).is_err());
    assert_eq!(materializer, before);
}

#[test]
fn materializer_filters_recipient_and_wrong_decider_and_orders_items() {
    let (first, _) = fixture("actor-1", 1);
    let (second, _) = fixture("actor-2", 2);
    let mut materializer = NotificationMaterializer::new();
    materializer.apply_committed(&[second, first], 2).unwrap();
    let actor_one = materializer.human_inbox_items("actor-1").unwrap();
    assert_eq!(actor_one.len(), 1);
    assert_eq!(actor_one[0].kind, HumanInboxKind::Approval);
    assert!(actor_one[0].detail["redacted_summary"]
        .as_str()
        .unwrap()
        .contains("redacted"));
    assert_eq!(
        materializer.human_inbox_items("actor-unknown").unwrap(),
        Vec::<kiana_domain::HumanInboxItem>::new()
    );
}
