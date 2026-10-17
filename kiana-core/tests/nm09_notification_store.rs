//! NM-09 fixtures: the in-process/in-app notification store answers only a server-bound
//! recipient, keeps pages bounded and rebuildable, and never turns a read/ACK into authority.
//!
//! Deny-first order: unbounded/foreign/unscoped access, then empty-vs-unavailable, then
//! bounded pagination, then read/ACK separation, then append-only history.

use kiana_core::{
    NotificationListRequest, NotificationPage, NotificationProjectionMutation, NotificationStore,
    NotificationStoreError, NOTIFICATION_MUTATION_SCHEMA, NOTIFICATION_PAGE_SCHEMA,
    NOTIFICATION_STORE_MAX_PAGE_SIZE, NOTIFICATION_STORE_SCHEMA,
};
use kiana_domain::{
    HumanAction, HumanInboxKind, HumanTaskBridge, MessageId, Notification, NotificationChannel,
    NotificationMaterialization, RequestId, RuntimeEvent,
};
use serde_json::json;
use std::collections::BTreeSet;

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

fn action(id: &str) -> HumanAction {
    HumanAction {
        id: id.to_owned(),
        label: "Act".to_owned(),
        command: format!("notification.{id}"),
        arguments: json!({"ref": "opaque"}),
        required_fields: Vec::new(),
    }
}

fn list_request(recipient: &str, limit: u16) -> NotificationListRequest {
    NotificationListRequest {
        schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
        recipient_id: recipient.to_owned(),
        limit,
        after_item_id: None,
        expected_source_cursor: None,
    }
}

fn list(
    store: &NotificationStore,
    recipient: &str,
    limit: u16,
) -> Result<NotificationPage, NotificationStoreError> {
    store.list(list_request(recipient, limit))
}

/// A committed `run.completed` fact materialized as a non-actionable Question row.
fn question_event(recipient: &str, cursor: u64) -> RuntimeEvent {
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
    let event = RuntimeEvent::new(
        RequestId::new(),
        cursor,
        "run.completed",
        json!({"source": "eventlog", "owner_id": recipient}),
    )
    .unwrap();
    let materialization = NotificationMaterialization::new(
        notification,
        event.event_id,
        cursor,
        HumanInboxKind::Question,
        "Run completed",
        "A redacted run terminal is waiting",
        500,
        9_000,
        vec![format!("event:evidence-{cursor}")],
        vec![action("open")],
        None,
    )
    .unwrap();
    let mut event = event;
    event.data["source_cursor"] = json!(cursor);
    event.data["notification_materialization"] = json!(materialization);
    event
}

/// A committed `approval.requested` fact materialized as an actionable Approval row with the
/// original HumanTask bridge (identity, decider, actions; never a copied task status).
fn approval_event(recipient: &str, cursor: u64) -> RuntimeEvent {
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
    let event = RuntimeEvent::new(
        RequestId::new(),
        cursor,
        "approval.requested",
        json!({"source": "eventlog", "owner_id": recipient}),
    )
    .unwrap();
    let bridge = HumanTaskBridge::new(
        "task-09",
        "approval",
        "approval-09",
        1,
        DIGEST,
        event.event_id,
        cursor,
        recipient,
        500,
        9_000,
        vec![format!("event:evidence-{cursor}")],
        vec![action("approve")],
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
        vec![format!("event:evidence-{cursor}")],
        vec![action("approve")],
        Some(bridge),
    )
    .unwrap();
    let mut event = event;
    event.data["source_cursor"] = json!(cursor);
    event.data["notification_materialization"] = json!(materialization);
    event
}

#[test]
fn unbound_and_foreign_recipient_queries_are_refused() {
    let mut store = NotificationStore::new();
    assert_eq!(
        list(&store, "actor-09", 10),
        Err(NotificationStoreError::ProjectionUnavailable)
    );
    store
        .apply_committed(&[approval_event("actor-09", 1)], 1)
        .unwrap();

    // A projection without a server-bound principal answers nobody, and no mutation runs.
    assert_eq!(store.server_recipient(), None);
    assert_eq!(
        list(&store, "actor-09", 10),
        Err(NotificationStoreError::ScopeRequired)
    );
    assert_eq!(
        store.mark_read("actor-09", "notification:probe", 10),
        Err(NotificationStoreError::ScopeRequired)
    );
    assert_eq!(
        store.acknowledge("actor-09", "notification:probe", 11),
        Err(NotificationStoreError::ScopeRequired)
    );
    assert_eq!(
        store.snooze("actor-09", "notification:probe", 12, 20),
        Err(NotificationStoreError::ScopeRequired)
    );

    // Binding is idempotent for the same principal and fails closed for another one.
    assert_eq!(store.bind_server_recipient("actor-09"), Ok(()));
    assert_eq!(store.bind_server_recipient("actor-09"), Ok(()));
    assert_eq!(
        store.bind_server_recipient("actor-10"),
        Err(NotificationStoreError::ScopeConflict)
    );
    assert_eq!(store.server_recipient(), Some("actor-09"));

    // A foreign recipient can neither page nor mutate through this store.
    assert_eq!(
        list(&store, "actor-10", 10),
        Err(NotificationStoreError::RecipientMismatch)
    );
    assert_eq!(
        store.mark_read("actor-10", "notification:probe", 10),
        Err(NotificationStoreError::RecipientMismatch)
    );
    assert_eq!(
        store.acknowledge("actor-10", "notification:probe", 11),
        Err(NotificationStoreError::RecipientMismatch)
    );
    assert_eq!(
        store.snooze("actor-10", "notification:probe", 12, 20),
        Err(NotificationStoreError::RecipientMismatch)
    );
    assert_eq!(
        store.mark_read("actor-09", "notification:missing", 13),
        Err(NotificationStoreError::ItemNotFound)
    );

    let page = list(&store, "actor-09", 10).unwrap();
    page.validate().unwrap();
    assert_eq!(page.schema, NOTIFICATION_PAGE_SCHEMA);
    assert_eq!(page.items.len(), 1);

    // Expected-cursor outcomes stay distinct: unset/equal is served, behind is stale, ahead is
    // not yet available.
    let mut behind = list_request("actor-09", 10);
    behind.expected_source_cursor = Some(0);
    assert_eq!(
        store.list(behind),
        Err(NotificationStoreError::Invalid("expected_source_cursor"))
    );
    let mut ahead = list_request("actor-09", 10);
    ahead.expected_source_cursor = Some(2);
    assert_eq!(
        store.list(ahead),
        Err(NotificationStoreError::ProjectionUnavailable)
    );
    let mut current = list_request("actor-09", 10);
    current.expected_source_cursor = Some(1);
    assert!(store.list(current).is_ok());
}

#[test]
fn empty_projection_is_not_unavailable_and_page_cap_is_enforced() {
    let mut store = NotificationStore::new();
    store.bind_server_recipient("actor-09").unwrap();
    store
        .apply_committed(&[approval_event("actor-09", 1)], 1)
        .unwrap();

    // Unbounded, zero and wrong-schema requests are refused before any row is read.
    for limit in [0, NOTIFICATION_STORE_MAX_PAGE_SIZE + 1, u16::MAX] {
        assert_eq!(
            list(&store, "actor-09", limit),
            Err(NotificationStoreError::Invalid("limit"))
        );
    }
    let mut wrong_schema = list_request("actor-09", 10);
    wrong_schema.schema = "kiana.notification-store.v2".to_owned();
    assert_eq!(
        store.list(wrong_schema),
        Err(NotificationStoreError::Invalid("schema"))
    );
    assert_eq!(
        list(&store, "   ", 10),
        Err(NotificationStoreError::Invalid("recipient_id"))
    );

    // An empty page for a bound principal with no facts is Ok, not an error.
    let mut other = NotificationStore::new();
    other.bind_server_recipient("actor-10").unwrap();
    other
        .apply_committed(&[approval_event("actor-09", 1)], 1)
        .unwrap();
    let empty = list(&other, "actor-10", 10).unwrap();
    empty.validate().unwrap();
    assert!(empty.items.is_empty());
    assert_eq!(empty.next_after_item_id, None);
    assert!(!empty.page_digest.is_empty());
    assert_eq!(empty.source_cursor, 1);
    let page = list(&store, "actor-09", 10).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_ne!(empty, page);

    // A forged continuation cursor or an impossible ack-without-read row never validates.
    let mut forged_cursor = page.clone();
    forged_cursor.next_after_item_id = Some("notification:forged".to_owned());
    assert_eq!(
        forged_cursor.validate(),
        Err(NotificationStoreError::Invalid("page_next_cursor"))
    );
    let mut forged_row = page.items[0].clone();
    forged_row.acknowledged = true;
    assert_eq!(
        forged_row.validate(),
        Err(NotificationStoreError::Invalid("acknowledgement"))
    );
    let mut forged_page_digest = page.clone();
    forged_page_digest.page_digest = "sha256:forged".to_owned();
    assert_eq!(
        forged_page_digest.validate(),
        Err(NotificationStoreError::DigestMismatch)
    );
}

#[test]
fn bounded_pagination_terminates_without_duplicates_or_cross_recipient_leaks() {
    let mut events = Vec::new();
    for cursor in 1..=20u64 {
        events.push(question_event("actor-09", cursor));
    }
    for cursor in 21..=33u64 {
        events.push(question_event("actor-10", cursor));
    }
    let mut store = NotificationStore::new();
    store.bind_server_recipient("actor-09").unwrap();
    store.apply_committed(&events, 33).unwrap();
    assert_eq!(store.source_cursor(), 33);
    assert_eq!(store.retained_source_event_count(), 33);

    // The bound recipient sees exactly its 20 rows even though 33 facts are retained.
    let full = list(&store, "actor-09", NOTIFICATION_STORE_MAX_PAGE_SIZE).unwrap();
    full.validate().unwrap();
    assert_eq!(full.items.len(), 20);
    assert_eq!(full.next_after_item_id, None);

    // Same request, same cursor: the page and its digest recompute identically.
    let request = list_request("actor-09", 10);
    let first = store.list(request.clone()).unwrap();
    let second = store.list(request.clone()).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );

    // A fresh store rebuilt from the same committed page produces the same bytes.
    let mut rebuilt = NotificationStore::new();
    rebuilt.bind_server_recipient("actor-09").unwrap();
    rebuilt.apply_committed(&events, 33).unwrap();
    assert_eq!(rebuilt.list(request.clone()).unwrap(), first);
    assert_eq!(
        serde_json::to_string(&rebuilt.list(request).unwrap()).unwrap(),
        serde_json::to_string(&first).unwrap()
    );

    // Paginate with a small limit: bounded pages, forward progress, no duplicates.
    let mut seen: Vec<String> = Vec::new();
    let mut after: Option<String> = None;
    let mut pages = 0usize;
    loop {
        let mut step = list_request("actor-09", 5);
        step.after_item_id = after.clone();
        let page = store.list(step).unwrap();
        page.validate().unwrap();
        pages += 1;
        assert!(pages <= 8, "pagination must terminate within bounds");
        assert!(page.items.len() <= usize::from(NOTIFICATION_STORE_MAX_PAGE_SIZE));
        if let Some(next) = &page.next_after_item_id {
            assert_eq!(page.items.last().unwrap().item.item_id, *next);
        }
        if page.items.is_empty() {
            assert_eq!(page.next_after_item_id, None);
            break;
        }
        seen.extend(page.items.iter().map(|row| row.item.item_id.clone()));
        after = page.next_after_item_id;
        if after.is_none() {
            break;
        }
    }
    assert_eq!(seen.len(), 20);
    assert_eq!(seen.iter().collect::<BTreeSet<_>>().len(), 20);

    // A continuation cursor owned by another recipient is not a valid probe.
    let mut other = NotificationStore::new();
    other.bind_server_recipient("actor-10").unwrap();
    other.apply_committed(&events, 33).unwrap();
    let foreign = list(&other, "actor-10", NOTIFICATION_STORE_MAX_PAGE_SIZE).unwrap();
    assert_eq!(foreign.items.len(), 13);
    let foreign_id = foreign.items[0].item.item_id.clone();
    let mut probe = list_request("actor-09", 5);
    probe.after_item_id = Some(foreign_id);
    assert_eq!(
        store.list(probe),
        Err(NotificationStoreError::CursorInvalid)
    );
}

#[test]
fn read_and_ack_never_become_an_approval_or_task_status_mutation() {
    let event = approval_event("actor-09", 1);
    let mut store = NotificationStore::new();
    store.bind_server_recipient("actor-09").unwrap();
    store.apply_committed(&[event.clone()], 1).unwrap();
    let before = list(&store, "actor-09", 10).unwrap();
    before.validate().unwrap();
    let item_id = before.items[0].item.item_id.clone();

    let read = store.mark_read("actor-09", &item_id, 10).unwrap();
    read.validate().unwrap();
    assert_eq!(read.schema, NOTIFICATION_MUTATION_SCHEMA);
    assert!(read.changed && read.read && !read.acknowledged);
    let read_replay = store.mark_read("actor-09", &item_id, 10).unwrap();
    assert!(!read_replay.changed);
    assert_eq!(read_replay.projection_revision, read.projection_revision);

    let ack = store.acknowledge("actor-09", &item_id, 11).unwrap();
    ack.validate().unwrap();
    assert!(ack.changed && ack.read && ack.acknowledged);
    let ack_replay = store.acknowledge("actor-09", &item_id, 11).unwrap();
    assert!(!ack_replay.changed);

    // Clock regression and the projection ack-without-read state fail closed.
    assert_eq!(
        store.mark_read("actor-09", &item_id, 9),
        Err(NotificationStoreError::ClockRegression)
    );
    assert_eq!(
        store.acknowledge("actor-09", &item_id, 9),
        Err(NotificationStoreError::ClockRegression)
    );

    // The receipt is exactly the projection surface: no decision, approval or task-status field.
    let receipt = serde_json::to_value(&ack).unwrap();
    let keys = receipt
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<String>>();
    assert_eq!(
        keys,
        [
            "acknowledged",
            "changed",
            "item_id",
            "mutation_digest",
            "projection_revision",
            "read",
            "schema",
            "snoozed_until_unix_ms",
            "source_cursor",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<String>>()
    );
    let mut forged_approval = receipt.clone();
    forged_approval["approved"] = json!(true);
    assert!(serde_json::from_value::<NotificationProjectionMutation>(forged_approval).is_err());
    let mut forged_decision = receipt.clone();
    forged_decision["decision"] = json!("approved");
    assert!(serde_json::from_value::<NotificationProjectionMutation>(forged_decision).is_err());
    let mut tampered: NotificationProjectionMutation = serde_json::from_value(receipt).unwrap();
    tampered.mutation_digest = "sha256:tampered".to_owned();
    assert_eq!(
        tampered.validate(),
        Err(NotificationStoreError::DigestMismatch)
    );
    let mut forged_state: NotificationProjectionMutation =
        serde_json::from_value(serde_json::to_value(&ack).unwrap()).unwrap();
    forged_state.read = false;
    forged_state.mutation_digest = forged_state.digest();
    assert_eq!(
        forged_state.validate(),
        Err(NotificationStoreError::Invalid("mutation_acknowledgement"))
    );

    // The page reflects read/ACK presentation state only; the HumanTask bridge, its actions and
    // the source notification row are untouched, and no approval happened.
    let after = list(&store, "actor-09", 10).unwrap();
    after.validate().unwrap();
    assert!(after.items[0].read && after.items[0].acknowledged);
    assert_eq!(after.items[0].item, before.items[0].item);
    assert_eq!(
        before.items[0].item.detail["human_task"],
        after.items[0].item.detail["human_task"]
    );
    assert_eq!(
        after.items[0].item.detail["human_task"]["target_revision"],
        1
    );
    assert_eq!(
        after.items[0].item.detail["human_task"]["target_digest"],
        DIGEST
    );
    assert_eq!(
        before.items[0].item.detail["notification"],
        after.items[0].item.detail["notification"]
    );
    assert_eq!(
        after.items[0].item.detail["notification"]["status"],
        "pending"
    );
    assert_eq!(store.retained_source_event_count(), 1);

    // A fresh rebuild from the same committed fact is unread and unacknowledged again: the
    // projection mutation is process-local presentation state, not an authority fact.
    let mut rebuilt = NotificationStore::new();
    rebuilt.bind_server_recipient("actor-09").unwrap();
    rebuilt.apply_committed(&[event], 1).unwrap();
    let rebuilt_page = list(&rebuilt, "actor-09", 10).unwrap();
    assert!(!rebuilt_page.items[0].read && !rebuilt_page.items[0].acknowledged);
    assert_eq!(rebuilt_page.items[0].item, before.items[0].item);
}

#[test]
fn projections_only_append_and_mutations_never_delete_source_history() {
    let first = question_event("actor-09", 1);
    let second = question_event("actor-09", 2);
    let mut store = NotificationStore::new();
    store.bind_server_recipient("actor-09").unwrap();
    store.apply_committed(&[first.clone()], 1).unwrap();
    let retained = store.retained_source_event_count();
    let item_id = list(&store, "actor-09", 10).unwrap().items[0]
        .item
        .item_id
        .clone();
    store.mark_read("actor-09", &item_id, 10).unwrap();
    store.acknowledge("actor-09", &item_id, 11).unwrap();
    assert_eq!(store.retained_source_event_count(), retained);

    // An exact replay of the same committed page is idempotent and keeps the cursor.
    store.apply_committed(&[first.clone()], 1).unwrap();
    assert_eq!(store.source_cursor(), 1);
    assert_eq!(store.retained_source_event_count(), retained);
    assert!(list(&store, "actor-09", 10).unwrap().items[0].acknowledged);

    // Advancing the committed cursor appends the new fact and keeps presentation state.
    store.apply_committed(&[second.clone()], 2).unwrap();
    assert_eq!(store.source_cursor(), 2);
    assert_eq!(store.retained_source_event_count(), 2);
    let page = list(&store, "actor-09", 10).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 2);
    let read_row = page
        .items
        .iter()
        .find(|row| row.item.item_id == item_id)
        .unwrap();
    assert!(read_row.read && read_row.acknowledged);

    // A regressed committed cursor is refused and cannot delete anything.
    assert!(store.apply_committed(&[first], 1).is_err());
    assert_eq!(store.source_cursor(), 2);
    assert_eq!(store.retained_source_event_count(), 2);
}
