//! NM-11 fixtures: server-time urgency/due/source ordering, unread/read/ACK/snooze/digest
//! projection, and the replayable read-state facts behind it.
//!
//! Deny-first order: client clocks / pre-selected answers cannot reprioritize or approve, silence
//! cannot hide critical or Unknown items, ordering never crosses the bound recipient's scope, and
//! a read-state rebuild refuses unknown, ahead, conflicting and regressive facts. Success path:
//! the same committed facts produce the same order, the same counts and the same read state after
//! a restart, and a critical item stays queryable at every page size.

use kiana_core::{
    classify_notification, compare_notification_digest_folds, compare_notification_priority,
    notification_due_state, notification_priority_digest, NotificationDigestFold,
    NotificationDigestMember, NotificationDueState, NotificationInboxCounts,
    NotificationInboxSummary, NotificationListRequest, NotificationPage, NotificationPriority,
    NotificationProjector, NotificationReadStateFact, NotificationStore, NotificationStoreError,
    NotificationUrgency, NOTIFICATION_DIGEST_FOLD_SCHEMA, NOTIFICATION_INBOX_SUMMARY_SCHEMA,
    NOTIFICATION_STORE_MAX_PAGE_SIZE, NOTIFICATION_STORE_MAX_SNOOZE_MS, NOTIFICATION_STORE_SCHEMA,
};
use kiana_domain::{
    EventId, HumanAction, HumanInboxItem, HumanInboxKind, HumanTaskBridge, MessageId, Notification,
    NotificationChannel, NotificationMaterialization, NotificationStatus, RequestId, RuntimeEvent,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const RECIPIENT: &str = "actor-11";
const OTHER: &str = "actor-12";
const EXPIRES_AT_UNIX_MS: u64 = 9_999;

struct Inbox {
    event: RuntimeEvent,
    item_id: String,
    cursor: u64,
}

fn action(id: &str) -> HumanAction {
    HumanAction {
        id: id.to_owned(),
        label: "Act".to_owned(),
        command: format!("notification.{id}"),
        arguments: json!({"ref": "opaque"}),
        required_fields: Vec::new(),
    }
}

/// A committed notification fact materialized as one display-only inbox row.
fn inbox_event(
    event_kind: &str,
    recipient: &str,
    cursor: u64,
    kind: HumanInboxKind,
    due_at_unix_ms: u64,
    status: NotificationStatus,
) -> Inbox {
    let mut notification = Notification::new(
        MessageId::new(),
        recipient,
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        10,
        10_000,
        1,
        None,
    )
    .unwrap();
    if status != NotificationStatus::Pending {
        notification
            .transition_status(NotificationStatus::Queued)
            .unwrap();
        if status != NotificationStatus::Queued {
            notification.transition_status(status).unwrap();
        }
    }
    let item_id = format!("notification:{}", notification.notification_id);
    let event = RuntimeEvent::new(
        RequestId::new(),
        cursor,
        event_kind,
        json!({"source": "eventlog", "owner_id": recipient}),
    )
    .unwrap();
    let bridge = kind.human_task_target_kind().map(|target_kind| {
        HumanTaskBridge::new(
            format!("task-{cursor}"),
            target_kind,
            format!("target-{cursor}"),
            1,
            DIGEST,
            event.event_id,
            cursor,
            recipient,
            due_at_unix_ms,
            EXPIRES_AT_UNIX_MS,
            vec![format!("event:evidence-{cursor}")],
            vec![action("open")],
        )
        .unwrap()
    });
    let materialization = NotificationMaterialization::new(
        notification,
        event.event_id,
        cursor,
        kind,
        format!("Notification {cursor}"),
        "A redacted notification is waiting",
        due_at_unix_ms,
        EXPIRES_AT_UNIX_MS,
        vec![format!("event:evidence-{cursor}")],
        vec![action("open")],
        bridge,
    )
    .unwrap();
    let mut event = event;
    event.data["source_cursor"] = json!(cursor);
    event.data["notification_materialization"] = json!(materialization);
    Inbox {
        event,
        item_id,
        cursor,
    }
}

fn events(indexes: &[&Inbox]) -> Vec<RuntimeEvent> {
    indexes.iter().map(|inbox| inbox.event.clone()).collect()
}

fn seeded_store(indexes: &[&Inbox], clock: Option<u64>) -> NotificationStore {
    let mut store = NotificationStore::new();
    store.bind_server_recipient(RECIPIENT).unwrap();
    if let Some(clock) = clock {
        store.bind_server_clock(clock).unwrap();
    }
    store
        .apply_committed(&events(indexes), indexes.len() as u64)
        .unwrap();
    store
}

fn list(store: &NotificationStore, limit: u16) -> Result<NotificationPage, NotificationStoreError> {
    store.list(NotificationListRequest {
        schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
        recipient_id: RECIPIENT.to_owned(),
        limit,
        after_item_id: None,
        expected_source_cursor: None,
    })
}

fn order(page: &NotificationPage) -> Vec<String> {
    page.items
        .iter()
        .map(|row| row.item.item_id.clone())
        .collect()
}

/// A minimal display item with a hand-written detail, used to prove that a committed fact cannot
/// smuggle a client priority hint into the server-side classification.
fn hinted_item(item_id: &str, kind: HumanInboxKind, extra: (&str, Value)) -> HumanInboxItem {
    let mut detail = json!({
        "schema": "kiana.notification-materialization.v1",
        "notification": {"status": "pending"},
        "source_cursor": 1,
        "due_at_unix_ms": 100,
    });
    detail[extra.0] = extra.1;
    HumanInboxItem {
        item_id: item_id.to_owned(),
        kind,
        title: "Server-owned attention".to_owned(),
        source_ref: "event:event-11".to_owned(),
        run_id: None,
        detail,
        actions: vec![action("open")],
    }
}

#[test]
fn client_clocks_and_preselected_answers_cannot_reprioritize_or_approve() {
    // The list request has no client clock, urgency, action or decision field: a forged request
    // fails to decode instead of being reinterpreted as a server input.
    let base = json!({
        "schema": NOTIFICATION_STORE_SCHEMA,
        "recipient_id": RECIPIENT,
        "limit": 10,
    });
    for (field, value) in [
        ("urgency", json!("critical")),
        ("priority", json!(1)),
        ("priority_rank", json!(1)),
        ("client_time_unix_ms", json!(1)),
        ("server_time_unix_ms", json!(1)),
        ("expected_server_time_unix_ms", json!(1)),
        ("preselected_action", json!("approve")),
        ("approved", json!(true)),
        ("decision", json!("approved")),
        ("actions", json!([])),
        ("read", json!(true)),
        ("acknowledged", json!(true)),
        ("snoozed_until_unix_ms", json!(30)),
        ("recipient_scope", json!(["project/a"])),
    ] {
        let mut forged = base.clone();
        forged[field] = value;
        assert!(
            serde_json::from_value::<NotificationListRequest>(forged).is_err(),
            "{field} must not decode into a list request"
        );
    }

    // A committed materialization cannot smuggle the same keys into the item detail either: the
    // classification refuses before a priority forms.
    for (field, value) in [
        ("urgency", json!("critical")),
        ("priority", json!(1)),
        ("client_time_unix_ms", json!(1)),
        ("server_time_unix_ms", json!(99_999)),
        ("read", json!(true)),
        ("acknowledged", json!(true)),
        ("snoozed_until_unix_ms", json!(30)),
        ("preselected_action", json!("approve")),
        ("approved", json!(true)),
        ("decision", json!("approved")),
    ] {
        let item = hinted_item(
            "notification:hinted",
            HumanInboxKind::Question,
            (field, value),
        );
        assert_eq!(
            classify_notification(&item),
            Err(kiana_core::NotificationPriorityError::ClientHint),
            "{field} must not reprioritize a committed item"
        );
    }

    // Urgency comes from the server-owned kind, not from the prose: the same alarming title cannot
    // promote a medium question into a critical incident.
    let mut question = hinted_item(
        "notification:q",
        HumanInboxKind::Question,
        ("note", json!("x")),
    );
    question.title = "CRITICAL incident requires approval".to_owned();
    let mut incident = question.clone();
    incident.item_id = "notification:i".to_owned();
    incident.kind = HumanInboxKind::Incident;
    assert_eq!(
        classify_notification(&question).unwrap().urgency,
        NotificationUrgency::Medium
    );
    assert_eq!(
        classify_notification(&incident).unwrap().urgency,
        NotificationUrgency::Critical
    );

    // A malformed due time or a zero source sequence never classifies, so no client can make an
    // item "due now" by omitting the server values.
    let mut missing_due = question.clone();
    missing_due.detail = json!({"source_cursor": 1});
    assert_eq!(
        classify_notification(&missing_due),
        Err(kiana_core::NotificationPriorityError::DueInvalid)
    );
    let mut zero_due = question.clone();
    zero_due.detail = json!({"source_cursor": 1, "due_at_unix_ms": 0});
    assert_eq!(
        classify_notification(&zero_due),
        Err(kiana_core::NotificationPriorityError::DueInvalid)
    );
    let mut zero_cursor = question.clone();
    zero_cursor.detail = json!({"source_cursor": 0, "due_at_unix_ms": 100});
    assert_eq!(
        classify_notification(&zero_cursor),
        Err(kiana_core::NotificationPriorityError::CursorInvalid)
    );

    // The expired horizon is bounded and zero is not a clock reading.
    assert_eq!(
        notification_due_state(100, 0),
        Err(kiana_core::NotificationPriorityError::ServerTimeInvalid)
    );
    assert_eq!(
        notification_due_state(0, 100),
        Err(kiana_core::NotificationPriorityError::ServerTimeInvalid)
    );
    assert_eq!(
        notification_due_state(100, 99),
        Ok(NotificationDueState::Scheduled)
    );
    assert_eq!(
        notification_due_state(100, 100),
        Ok(NotificationDueState::Due)
    );
    assert_eq!(
        notification_due_state(100, 101),
        Ok(NotificationDueState::Overdue)
    );

    // The ordering key is server-derived: a priority with a client-proposed due time cannot
    // reproduce the committed digest.
    let priority = classify_notification(&incident).unwrap();
    priority.validate().unwrap();
    let expected = priority.priority_digest();
    assert_eq!(
        expected,
        notification_priority_digest(
            &priority.item_id,
            priority.urgency,
            priority.due_at_unix_ms,
            priority.source_cursor,
            &priority.digest_group,
        )
    );
    assert_ne!(
        expected,
        notification_priority_digest(
            &priority.item_id,
            priority.urgency,
            99_999,
            priority.source_cursor,
            &priority.digest_group,
        )
    );
    assert_ne!(
        expected,
        notification_priority_digest(
            &priority.item_id,
            NotificationUrgency::Low,
            priority.due_at_unix_ms,
            priority.source_cursor,
            &priority.digest_group,
        )
    );
    let mut forged = priority.clone();
    forged.due_at_unix_ms = 0;
    assert_eq!(
        forged.validate(),
        Err(kiana_core::NotificationPriorityError::DueInvalid)
    );
    let mut forged_group = priority.clone();
    forged_group.digest_group = "  ".to_owned();
    assert_eq!(
        forged_group.validate(),
        Err(kiana_core::NotificationPriorityError::DigestGroupInvalid)
    );
}

#[test]
fn silence_cannot_hide_critical_or_unknown_items_or_approve_anything() {
    let question = inbox_event(
        "run.completed",
        RECIPIENT,
        1,
        HumanInboxKind::Question,
        100,
        NotificationStatus::Pending,
    );
    let incident = inbox_event(
        "communication.incident",
        RECIPIENT,
        2,
        HumanInboxKind::Incident,
        5_000,
        NotificationStatus::Pending,
    );
    let unknown = inbox_event(
        "run.result_unknown",
        RECIPIENT,
        3,
        HumanInboxKind::Question,
        3_000,
        NotificationStatus::Unknown,
    );
    let fixtures = [&question, &incident, &unknown];
    let mut store = seeded_store(&fixtures, Some(100));
    let before = list(&store, 10).unwrap();
    before.validate().unwrap();
    assert_eq!(
        order(&before),
        vec![
            incident.item_id.clone(),
            question.item_id.clone(),
            unknown.item_id.clone()
        ]
    );
    let unknown_row = before
        .items
        .iter()
        .find(|row| row.item.item_id == unknown.item_id)
        .unwrap();
    assert!(unknown_row.unknown);
    assert_eq!(unknown_row.urgency, NotificationUrgency::Medium);
    assert_eq!(unknown_row.due_state, Some(NotificationDueState::Scheduled));
    assert_eq!(
        before.items[0].due_state,
        Some(NotificationDueState::Scheduled)
    );

    // A critical item and an unresolved outcome may be read, but never silenced: snooze is refused
    // before any projection state changes.
    assert_eq!(
        store.snooze(RECIPIENT, &incident.item_id, 100, 200),
        Err(NotificationStoreError::SnoozeNotHideable)
    );
    assert_eq!(
        store.snooze(RECIPIENT, &unknown.item_id, 100, 200),
        Err(NotificationStoreError::SnoozeNotHideable)
    );
    let read = store.mark_read(RECIPIENT, &incident.item_id, 100).unwrap();
    read.validate().unwrap();
    assert!(read.changed && read.read && !read.acknowledged);
    let ack = store
        .acknowledge(RECIPIENT, &incident.item_id, 101)
        .unwrap();
    assert!(ack.changed && ack.read && ack.acknowledged);
    let ack_replay = store
        .acknowledge(RECIPIENT, &incident.item_id, 101)
        .unwrap();
    assert!(!ack_replay.changed);
    assert_eq!(ack_replay.projection_revision, ack.projection_revision);

    // A client clock behind the bound server clock cannot retro-date the mutation, and a read/
    // ACK on a critical item is not an approval: the item stays first and stays queryable.
    assert_eq!(
        store.mark_read(RECIPIENT, &incident.item_id, 99),
        Err(NotificationStoreError::ClockRegression)
    );
    assert_eq!(
        store.bind_server_clock(99),
        Err(NotificationStoreError::ClockRegression)
    );
    assert_eq!(store.bind_server_clock(100), Ok(()));

    // A snoozable item only ever moves within its bounded window; the window cannot end at or
    // before the server clock and cannot leave it.
    let snoozed = store
        .snooze(RECIPIENT, &question.item_id, 100, 200)
        .unwrap();
    snoozed.validate().unwrap();
    assert!(snoozed.changed && !snoozed.read);
    assert_eq!(
        store.snooze(RECIPIENT, &question.item_id, 100, 100),
        Err(NotificationStoreError::Invalid("snooze_window"))
    );
    assert_eq!(
        store.snooze(
            RECIPIENT,
            &question.item_id,
            100,
            100 + NOTIFICATION_STORE_MAX_SNOOZE_MS + 1
        ),
        Err(NotificationStoreError::Invalid("snooze_horizon"))
    );
    assert_eq!(
        store.snooze(RECIPIENT, &question.item_id, 100, 150),
        Err(NotificationStoreError::ClockRegression)
    );

    let snoozed_page = list(&store, 10).unwrap();
    snoozed_page.validate().unwrap();
    assert_eq!(
        order(&snoozed_page),
        order(&before),
        "read/ACK/snooze state must never reorder the inbox"
    );
    let question_row = snoozed_page
        .items
        .iter()
        .find(|row| row.item.item_id == question.item_id)
        .unwrap();
    assert!(question_row.snooze_active);
    assert_eq!(question_row.snoozed_until_unix_ms, Some(200));
    assert_eq!(
        question_row.due_state,
        Some(NotificationDueState::Scheduled)
    );
    assert!(!question_row.read && !question_row.acknowledged);

    // Once the server clock passes the window the item is due again, still listed, and its stored
    // read/ACK state is untouched.
    store.bind_server_clock(200).unwrap();
    let expired = list(&store, 10).unwrap();
    expired.validate().unwrap();
    let question_row = expired
        .items
        .iter()
        .find(|row| row.item.item_id == question.item_id)
        .unwrap();
    assert!(!question_row.snooze_active);
    assert_eq!(question_row.snoozed_until_unix_ms, Some(200));
    assert_eq!(question_row.due_state, Some(NotificationDueState::Overdue));
    assert_eq!(
        store.snooze(RECIPIENT, &question.item_id, 200, 150),
        Err(NotificationStoreError::Invalid("snooze_window"))
    );
    assert_eq!(
        store.snooze(RECIPIENT, &question.item_id, 150, 300),
        Err(NotificationStoreError::ClockRegression)
    );

    // Reading and re-snoozing never approve: the committed item (bridge, actions, source status)
    // and its server ordering key are byte-identical, and a critical item is always the first row
    // at every page size.
    let after = list(&store, 10).unwrap();
    let before_incident = before
        .items
        .iter()
        .find(|row| row.item.item_id == incident.item_id)
        .unwrap();
    let after_incident = after
        .items
        .iter()
        .find(|row| row.item.item_id == incident.item_id)
        .unwrap();
    assert_eq!(before_incident.item, after_incident.item);
    assert_eq!(
        after_incident.item.detail["notification"]["status"].as_str(),
        Some("pending")
    );
    assert_eq!(
        after_incident.item.detail["human_task"]["target_revision"],
        json!(1)
    );
    assert_eq!(
        after_incident.item.detail["human_task"]["target_digest"],
        json!(DIGEST)
    );
    assert_eq!(
        before_incident.priority_digest,
        after_incident.priority_digest
    );
    for limit in [1, 2, NOTIFICATION_STORE_MAX_PAGE_SIZE] {
        let page = list(&store, limit).unwrap();
        page.validate().unwrap();
        assert_eq!(
            page.items[0].item.item_id, incident.item_id,
            "a critical item is always queryable at limit {limit}"
        );
    }
    assert_eq!(store.retained_source_event_count(), 3);
}

#[test]
fn ordering_is_urgency_due_source_sequence_and_never_crosses_the_recipient_scope() {
    let first = inbox_event(
        "run.completed",
        RECIPIENT,
        1,
        HumanInboxKind::Question,
        900,
        NotificationStatus::Pending,
    );
    let other = inbox_event(
        "run.completed",
        OTHER,
        2,
        HumanInboxKind::Question,
        1,
        NotificationStatus::Pending,
    );
    let unknown = inbox_event(
        "run.result_unknown",
        RECIPIENT,
        3,
        HumanInboxKind::Question,
        900,
        NotificationStatus::Unknown,
    );
    let approval = inbox_event(
        "approval.requested",
        RECIPIENT,
        4,
        HumanInboxKind::Approval,
        100,
        NotificationStatus::Pending,
    );
    let foreign_incident = inbox_event(
        "communication.incident",
        OTHER,
        5,
        HumanInboxKind::Incident,
        5_000,
        NotificationStatus::Pending,
    );
    let incident = inbox_event(
        "communication.incident",
        RECIPIENT,
        6,
        HumanInboxKind::Incident,
        5_000,
        NotificationStatus::Pending,
    );
    let due_now = inbox_event(
        "run.completed",
        RECIPIENT,
        7,
        HumanInboxKind::Question,
        100,
        NotificationStatus::Pending,
    );
    let fixtures = [
        &first,
        &other,
        &unknown,
        &approval,
        &foreign_incident,
        &incident,
        &due_now,
    ];
    let store = seeded_store(&fixtures, Some(100));
    let page = list(&store, NOTIFICATION_STORE_MAX_PAGE_SIZE).unwrap();
    page.validate().unwrap();

    // urgency (critical first) -> due -> committed source sequence -> identity.
    assert_eq!(
        order(&page),
        vec![
            incident.item_id.clone(),
            approval.item_id.clone(),
            due_now.item_id.clone(),
            first.item_id.clone(),
            unknown.item_id.clone(),
        ]
    );
    assert_eq!(
        page.items.iter().map(|row| row.urgency).collect::<Vec<_>>(),
        vec![
            NotificationUrgency::Critical,
            NotificationUrgency::High,
            NotificationUrgency::Medium,
            NotificationUrgency::Medium,
            NotificationUrgency::Medium,
        ]
    );

    // The other recipient's critical item is not in this page and cannot be probed as a cursor.
    for row in &page.items {
        assert!(row.item.item_id != foreign_incident.item_id);
        assert!(row.item.item_id != other.item_id);
    }
    let mut probe = NotificationListRequest {
        schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
        recipient_id: RECIPIENT.to_owned(),
        limit: 5,
        after_item_id: Some(foreign_incident.item_id.clone()),
        expected_source_cursor: None,
    };
    assert_eq!(
        store.list(probe.clone()),
        Err(NotificationStoreError::CursorInvalid)
    );
    probe.after_item_id = Some(other.item_id.clone());
    assert_eq!(
        store.list(probe.clone()),
        Err(NotificationStoreError::CursorInvalid)
    );
    probe.recipient_id = OTHER.to_owned();
    assert_eq!(
        store.list(probe),
        Err(NotificationStoreError::RecipientMismatch)
    );
    let mut foreign = NotificationListRequest {
        schema: NOTIFICATION_STORE_SCHEMA.to_owned(),
        recipient_id: OTHER.to_owned(),
        limit: 5,
        after_item_id: None,
        expected_source_cursor: None,
    };
    assert_eq!(
        store.list(foreign.clone()),
        Err(NotificationStoreError::RecipientMismatch)
    );
    foreign.recipient_id = "   ".to_owned();
    assert_eq!(
        store.list(foreign),
        Err(NotificationStoreError::Invalid("recipient_id"))
    );

    // The order is a total order: it does not depend on the array order the caller presents, and
    // it does not depend on the server clock (only the due state does).
    let priorities = page
        .items
        .iter()
        .map(|row| classify_notification(&row.item).unwrap())
        .collect::<Vec<NotificationPriority>>();
    let mut sorted = priorities.clone();
    sorted.sort_by(compare_notification_priority);
    assert_eq!(
        sorted
            .iter()
            .map(|priority| priority.item_id.clone())
            .collect::<Vec<_>>(),
        order(&page)
    );
    let mut reversed = priorities;
    reversed.reverse();
    reversed.sort_by(compare_notification_priority);
    assert_eq!(
        reversed
            .iter()
            .map(|priority| priority.item_id.clone())
            .collect::<Vec<_>>(),
        order(&page)
    );
    let other_clock = seeded_store(&fixtures, Some(1_000_000));
    let other_page = list(&other_clock, NOTIFICATION_STORE_MAX_PAGE_SIZE).unwrap();
    assert_eq!(order(&other_page), order(&page));
    assert_eq!(
        other_page.server_time_unix_ms,
        Some(1_000_000),
        "the page stamps the bound server clock"
    );
    assert_eq!(page.server_time_unix_ms, Some(100));
    assert_eq!(
        other_page.items[0].due_state,
        Some(NotificationDueState::Overdue)
    );

    // Ties on urgency, due and source cursor fall back to the item identity, so a page never
    // depends on map iteration order.
    let mut tied = priorities_for_ties();
    tied.sort_by(compare_notification_priority);
    let tied_order = tied
        .iter()
        .map(|priority| priority.item_id.clone())
        .collect::<Vec<_>>();
    let mut reversed_ties = tied.clone();
    reversed_ties.reverse();
    reversed_ties.sort_by(compare_notification_priority);
    assert_eq!(
        reversed_ties
            .iter()
            .map(|priority| priority.item_id.clone())
            .collect::<Vec<_>>(),
        tied_order
    );
}

fn priorities_for_ties() -> Vec<NotificationPriority> {
    ["notification:c", "notification:a", "notification:b"]
        .into_iter()
        .map(|item_id| NotificationPriority {
            schema: "kiana.notification-priority.v1".to_owned(),
            item_id: item_id.to_owned(),
            urgency: NotificationUrgency::Medium,
            due_at_unix_ms: 100,
            source_cursor: 1,
            digest_group: "digest:tie".to_owned(),
        })
        .collect()
}

#[test]
fn read_state_facts_rebuild_the_same_unread_state_and_never_become_authority() {
    let read_item = inbox_event(
        "run.completed",
        RECIPIENT,
        1,
        HumanInboxKind::Question,
        100,
        NotificationStatus::Pending,
    );
    let snoozed_item = inbox_event(
        "run.completed",
        RECIPIENT,
        2,
        HumanInboxKind::Question,
        700,
        NotificationStatus::Pending,
    );
    let acknowledged_item = inbox_event(
        "run.completed",
        RECIPIENT,
        3,
        HumanInboxKind::Question,
        900,
        NotificationStatus::Pending,
    );
    let fixtures = [&read_item, &snoozed_item, &acknowledged_item];
    let mut store = seeded_store(&fixtures, Some(100));
    store.mark_read(RECIPIENT, &read_item.item_id, 100).unwrap();
    store
        .snooze(RECIPIENT, &snoozed_item.item_id, 100, 2_000)
        .unwrap();
    store
        .acknowledge(RECIPIENT, &acknowledged_item.item_id, 101)
        .unwrap();
    let before = list(&store, 10).unwrap();
    before.validate().unwrap();
    assert_eq!(store.retained_source_event_count(), 3);

    // The exported facts bind the committed source event and cursor of each item; the receipt can
    // never name them itself.
    let facts = store.read_state_facts().unwrap();
    assert_eq!(facts.len(), 3);
    facts.iter().for_each(|fact| fact.validate().unwrap());
    let mut cursors = facts
        .iter()
        .map(|fact| fact.source_cursor)
        .collect::<Vec<_>>();
    cursors.sort_unstable();
    assert_eq!(cursors, vec![1, 2, 3]);
    let mut declared = fixtures
        .iter()
        .map(|inbox| inbox.cursor)
        .collect::<Vec<u64>>();
    declared.sort_unstable();
    assert_eq!(cursors, declared, "each fact binds its committed cursor");
    assert_eq!(
        facts.iter().filter(|fact| fact.read).count(),
        2,
        "read + acknowledged"
    );
    assert_eq!(facts.iter().filter(|fact| fact.acknowledged).count(), 1);
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.snoozed_until_unix_ms == Some(2_000))
            .count(),
        1
    );

    // A fresh process folds the committed page plus the read-state facts back into the same rows.
    let mut restarted = seeded_store(&fixtures, Some(100));
    restarted.apply_read_state_facts(&facts).unwrap();
    let after = list(&restarted, 10).unwrap();
    after.validate().unwrap();
    assert_eq!(
        serde_json::to_string(&after).unwrap(),
        serde_json::to_string(&before).unwrap(),
        "read/ACK/snooze state must survive the restart byte for byte"
    );
    assert_eq!(restarted.retained_source_event_count(), 3);
    assert_eq!(restarted.read_state_facts().unwrap(), facts);

    // The same facts are what the durable inbox rebuild (NM-12) folds, so the two projections
    // agree on read/ACK/snooze without either of them owning a task status.
    let projector = NotificationProjector::rebuild(&events(&fixtures), 3, &[], &facts).unwrap();
    let snapshot = projector.inbox_snapshot(RECIPIENT).unwrap();
    snapshot.validate().unwrap();
    for row in &after.items {
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.item.item_id == row.item.item_id)
            .unwrap();
        assert_eq!(entry.read, row.read);
        assert_eq!(entry.acknowledged, row.acknowledged);
        assert_eq!(entry.snoozed_until_unix_ms, row.snoozed_until_unix_ms);
        assert_eq!(entry.item, row.item);
    }

    // Idempotent replay: the same facts fold twice without changing the projection.
    restarted.apply_read_state_facts(&facts).unwrap();
    assert_eq!(
        serde_json::to_string(&list(&restarted, 10).unwrap()).unwrap(),
        serde_json::to_string(&after).unwrap()
    );

    // A rebuild without the scope, without a committed projection, naming an unknown item, running
    // ahead of the cursor, un-reading, clearing a snooze, moving a snooze backwards or claiming an
    // already-used revision is refused.
    let mut unbound = NotificationStore::new();
    assert_eq!(
        unbound.apply_read_state_facts(&facts),
        Err(NotificationStoreError::ScopeRequired)
    );
    unbound.bind_server_recipient(RECIPIENT).unwrap();
    assert_eq!(
        unbound.apply_read_state_facts(&facts),
        Err(NotificationStoreError::ProjectionUnavailable)
    );
    let mut fresh = seeded_store(&fixtures, Some(100));
    let unknown_fact = NotificationReadStateFact::new(
        "notification:missing",
        EventId::new(),
        1,
        1,
        true,
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        fresh.apply_read_state_facts(&[unknown_fact]),
        Err(NotificationStoreError::ItemNotFound)
    );
    let ahead = NotificationReadStateFact::new(
        read_item.item_id.clone(),
        EventId::new(),
        9,
        2,
        true,
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        fresh.apply_read_state_facts(&[ahead]),
        Err(NotificationStoreError::ReadStateCursorAhead)
    );
    // The same committed facts fold into a fresh store; only then is a conflicting revision, an
    // un-read, a cleared snooze or a backwards snooze a refusal instead of a first write.
    let current = store.read_state_facts().unwrap();
    fresh.apply_read_state_facts(&current).unwrap();
    let read_fact = current
        .iter()
        .find(|fact| fact.item_id == read_item.item_id)
        .unwrap()
        .clone();
    let mut regressed = read_fact.clone();
    regressed.projection_revision += 1;
    regressed.read = false;
    regressed.fact_digest = regressed.digest();
    assert_eq!(
        fresh.apply_read_state_facts(&[regressed]),
        Err(NotificationStoreError::ReadStateRegression)
    );
    let mut conflict = read_fact.clone();
    conflict.read = false;
    conflict.fact_digest = conflict.digest();
    assert_eq!(
        fresh.apply_read_state_facts(&[conflict]),
        Err(NotificationStoreError::ReadStateConflict)
    );
    let snooze_fact = current
        .iter()
        .find(|fact| fact.item_id == snoozed_item.item_id)
        .unwrap()
        .clone();
    let mut backwards = snooze_fact.clone();
    backwards.projection_revision += 1;
    backwards.snoozed_until_unix_ms = Some(1_000);
    backwards.fact_digest = backwards.digest();
    assert_eq!(
        fresh.apply_read_state_facts(&[backwards]),
        Err(NotificationStoreError::ReadStateRegression)
    );
    let mut cleared = snooze_fact.clone();
    cleared.projection_revision += 1;
    cleared.snoozed_until_unix_ms = None;
    cleared.fact_digest = cleared.digest();
    assert_eq!(
        fresh.apply_read_state_facts(&[cleared]),
        Err(NotificationStoreError::ReadStateRegression)
    );

    // A presentation fact is not a decision: the receipt keeps its nine presentation keys and a
    // forged fact carrying an approval field fails to decode.
    let receipt = store.mark_read(RECIPIENT, &read_item.item_id, 102).unwrap();
    let keys = serde_json::to_value(&receipt)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<String>>();
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
        .collect::<std::collections::BTreeSet<String>>()
    );
    let mut forged_fact = serde_json::to_value(&read_fact).unwrap();
    forged_fact["approved"] = json!(true);
    assert!(serde_json::from_value::<NotificationReadStateFact>(forged_fact).is_err());
    let mut forged_decision = serde_json::to_value(&read_fact).unwrap();
    forged_decision["decision"] = json!("approved");
    assert!(serde_json::from_value::<NotificationReadStateFact>(forged_decision).is_err());
    // The restored page still presents the committed item, not a copy of any task state.
    let pristine = seeded_store(&fixtures, Some(100));
    let committed_item = list(&pristine, 10)
        .unwrap()
        .items
        .iter()
        .find(|row| row.item.item_id == read_item.item_id)
        .cloned()
        .unwrap()
        .item;
    assert_eq!(
        after
            .items
            .iter()
            .find(|row| row.item.item_id == read_item.item_id)
            .unwrap()
            .item,
        committed_item
    );
}

#[test]
fn summary_and_digest_folds_are_server_time_scoped_to_the_bound_recipient() {
    let question = inbox_event(
        "run.completed",
        RECIPIENT,
        1,
        HumanInboxKind::Question,
        100,
        NotificationStatus::Pending,
    );
    let incident = inbox_event(
        "communication.incident",
        RECIPIENT,
        2,
        HumanInboxKind::Incident,
        5_000,
        NotificationStatus::Pending,
    );
    let unknown = inbox_event(
        "run.result_unknown",
        RECIPIENT,
        3,
        HumanInboxKind::Question,
        3_000,
        NotificationStatus::Unknown,
    );
    let foreign = inbox_event(
        "communication.incident",
        OTHER,
        4,
        HumanInboxKind::Incident,
        100,
        NotificationStatus::Pending,
    );
    let fixtures = [&question, &incident, &unknown, &foreign];

    // The summary needs the server-bound recipient and the server clock, and it refuses a
    // projection that has no committed cursor.
    let mut unbound = NotificationStore::new();
    assert_eq!(
        unbound.inbox_summary(),
        Err(NotificationStoreError::ScopeRequired)
    );
    unbound.bind_server_recipient(RECIPIENT).unwrap();
    unbound.bind_server_clock(100).unwrap();
    assert_eq!(
        unbound.inbox_summary(),
        Err(NotificationStoreError::ProjectionUnavailable)
    );
    let mut clockless = NotificationStore::new();
    clockless.bind_server_recipient(RECIPIENT).unwrap();
    clockless.apply_committed(&events(&fixtures), 4).unwrap();
    assert_eq!(
        clockless.inbox_summary(),
        Err(NotificationStoreError::ServerTimeRequired)
    );
    assert_eq!(
        clockless.bind_server_clock(0),
        Err(NotificationStoreError::Invalid("server_time_unix_ms"))
    );
    let mut store = seeded_store(&fixtures, Some(100));
    store.mark_read(RECIPIENT, &incident.item_id, 100).unwrap();
    store
        .acknowledge(RECIPIENT, &incident.item_id, 101)
        .unwrap();
    store
        .snooze(RECIPIENT, &question.item_id, 100, 200)
        .unwrap();

    let summary = store.inbox_summary().unwrap();
    summary.validate().unwrap();
    assert_eq!(summary.schema, NOTIFICATION_INBOX_SUMMARY_SCHEMA);
    assert_eq!(summary.recipient_id, RECIPIENT);
    assert_eq!(summary.server_time_unix_ms, 100);
    assert_eq!(summary.source_cursor, 4);
    assert_eq!(summary.counts.total, 3);
    assert_eq!(summary.counts.read, 1);
    assert_eq!(summary.counts.unread, 2);
    assert_eq!(summary.counts.acknowledged, 1);
    assert_eq!(summary.counts.snoozed, 1);
    assert_eq!(summary.counts.critical_total, 1);
    assert_eq!(summary.counts.critical_unread, 0);
    assert_eq!(summary.counts.unknown_total, 1);
    assert_eq!(summary.counts.overdue, 0);
    assert_eq!(summary.counts.due_now, 1);
    assert_eq!(summary.counts.scheduled, 2);
    assert_eq!(summary.digest_group_count, 3);

    // Counts and groups are recomputable projections: a forged digest, incoherent counts, an
    // impossible group count or a zero server time never validates.
    let mut forged_total = summary.clone();
    forged_total.counts.total = 4;
    assert_eq!(
        forged_total.validate(),
        Err(NotificationStoreError::Invalid("summary_counts"))
    );
    let mut forged_ack = summary.clone();
    forged_ack.counts.acknowledged = 3;
    assert_eq!(
        forged_ack.validate(),
        Err(NotificationStoreError::Invalid("summary_counts"))
    );
    let mut forged_buckets = summary.clone();
    forged_buckets.counts.scheduled = 0;
    forged_buckets.counts.overdue = 1;
    assert_eq!(
        forged_buckets.validate(),
        Err(NotificationStoreError::Invalid("summary_counts"))
    );
    let mut forged_groups = summary.clone();
    forged_groups.digest_group_count = 0;
    assert_eq!(
        forged_groups.validate(),
        Err(NotificationStoreError::Invalid("summary_digest_groups"))
    );
    let mut forged_clock = summary.clone();
    forged_clock.server_time_unix_ms = 0;
    assert_eq!(
        forged_clock.validate(),
        Err(NotificationStoreError::Invalid("summary_header"))
    );
    let mut forged_digest = summary.clone();
    forged_digest.summary_digest = "sha256:forged".to_owned();
    assert_eq!(
        forged_digest.validate(),
        Err(NotificationStoreError::DigestMismatch)
    );
    let mut incoherent = NotificationInboxCounts::default();
    incoherent.total = 1;
    incoherent.unread = 2;
    assert_eq!(
        NotificationInboxSummary::new(RECIPIENT, 4, 100, incoherent, 1),
        Err(NotificationStoreError::Invalid("summary_counts"))
    );
    assert_eq!(
        unbound.digest_folds(),
        Err(NotificationStoreError::ProjectionUnavailable)
    );

    // Digest folds are assembled only from the bound recipient's items, in the same deterministic
    // order as the page, and they never lower a member's urgency.
    let folds = store.digest_folds().unwrap();
    folds.iter().for_each(|fold| fold.validate().unwrap());
    assert_eq!(folds.len(), 3);
    assert_eq!(folds[0].urgency, NotificationUrgency::Critical);
    assert_eq!(folds[0].critical_count, 1);
    assert_eq!(folds[0].members.len(), 1);
    assert_eq!(folds[0].members[0].item_id, incident.item_id);
    let unknown_fold = folds
        .iter()
        .find(|fold| fold.members[0].item_id == unknown.item_id)
        .unwrap();
    assert_eq!(unknown_fold.unknown_count, 1);
    assert_eq!(unknown_fold.members[0].unknown, true);
    assert_eq!(unknown_fold.due_at_unix_ms, 3_000);
    let question_fold = folds
        .iter()
        .find(|fold| fold.members[0].item_id == question.item_id)
        .unwrap();
    assert_eq!(question_fold.due_at_unix_ms, 100);
    assert_eq!(question_fold.source_cursor, 1);
    let page = list(&store, 10).unwrap();
    let page_order = order(&page);
    let mut fold_order = folds
        .iter()
        .map(|fold| fold.members[0].item_id.clone())
        .collect::<Vec<_>>();
    fold_order.sort_by_key(|item_id| {
        page_order
            .iter()
            .position(|candidate| candidate == item_id)
            .unwrap()
    });
    let store_fold_order = folds
        .iter()
        .map(|fold| fold.members[0].item_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(fold_order, store_fold_order);
    let mut sorted_folds = folds.clone();
    sorted_folds.sort_by(compare_notification_digest_folds);
    assert_eq!(sorted_folds, folds);
    for fold in &folds {
        for member in &fold.members {
            assert!(page_order.contains(&member.item_id));
            assert!(member.item_id != foreign.item_id);
            assert_eq!(fold.recipient_id, RECIPIENT);
        }
    }

    // A fold is self-verifying: lowering the urgency, dropping a count, reordering or duplicating
    // members, or re-signing the digest is refused.
    let mut lowered = folds[0].clone();
    lowered.urgency = NotificationUrgency::Low;
    assert_eq!(
        lowered.validate(),
        Err(NotificationStoreError::Invalid("digest_fold_urgency"))
    );
    let mut lowered_due = folds[0].clone();
    lowered_due.due_at_unix_ms = 1;
    assert_eq!(
        lowered_due.validate(),
        Err(NotificationStoreError::Invalid("digest_fold_due"))
    );
    let mut dropped_count = folds[0].clone();
    dropped_count.critical_count = 0;
    assert_eq!(
        dropped_count.validate(),
        Err(NotificationStoreError::Invalid(
            "digest_fold_critical_count"
        ))
    );
    let mut dropped_unknown = unknown_fold.clone();
    dropped_unknown.unknown_count = 0;
    assert_eq!(
        dropped_unknown.validate(),
        Err(NotificationStoreError::Invalid("digest_fold_unknown_count"))
    );
    let mut dropped_unread = question_fold.clone();
    dropped_unread.unread_count = 0;
    assert_eq!(
        dropped_unread.validate(),
        Err(NotificationStoreError::Invalid("digest_fold_unread_count"))
    );
    let mut dropped_ack = question_fold.clone();
    dropped_ack.acknowledged_count = 1;
    assert_eq!(
        dropped_ack.validate(),
        Err(NotificationStoreError::Invalid(
            "digest_fold_acknowledged_count"
        ))
    );
    let mut forged_fold_digest = folds[0].clone();
    forged_fold_digest.fold_digest = "sha256:forged".to_owned();
    assert_eq!(
        forged_fold_digest.validate(),
        Err(NotificationStoreError::DigestMismatch)
    );
    let member = folds[0].members[0].clone();
    assert_eq!(
        NotificationDigestFold::new(RECIPIENT, "digest:empty", Vec::new()),
        Err(NotificationStoreError::Invalid("digest_fold_members"))
    );
    assert_eq!(
        NotificationDigestFold::new(
            RECIPIENT,
            "digest:duplicate",
            vec![member.clone(), member.clone()]
        ),
        Err(NotificationStoreError::Invalid(
            "digest_fold_member_duplicate"
        ))
    );
    let second_member = NotificationDigestMember {
        item_id: format!("{}z", member.item_id),
        ..member.clone()
    };
    let mut reordered = NotificationDigestFold::new(
        RECIPIENT,
        "digest:ordered",
        vec![member.clone(), second_member.clone()],
    )
    .unwrap();
    reordered.members = vec![second_member, member];
    assert_eq!(
        reordered.validate(),
        Err(NotificationStoreError::Invalid("digest_fold_member_order"))
    );
    assert_eq!(reordered.schema, NOTIFICATION_DIGEST_FOLD_SCHEMA);

    // Expiring the snooze at the server clock moves the item out of the snoozed count without
    // hiding it or lowering the critical bucket.
    store.bind_server_clock(200).unwrap();
    let expired = store.inbox_summary().unwrap();
    expired.validate().unwrap();
    assert_eq!(expired.counts.snoozed, 0);
    assert_eq!(expired.counts.overdue, 1);
    assert_eq!(expired.counts.due_now, 0);
    assert_eq!(expired.counts.scheduled, 2);
    assert_eq!(expired.counts.critical_total, 1);
}

#[test]
fn page_rows_recompute_priority_due_state_and_snooze_activation() {
    let question = inbox_event(
        "run.completed",
        RECIPIENT,
        1,
        HumanInboxKind::Question,
        100,
        NotificationStatus::Pending,
    );
    let incident = inbox_event(
        "communication.incident",
        RECIPIENT,
        2,
        HumanInboxKind::Incident,
        5_000,
        NotificationStatus::Pending,
    );
    let fixtures = [&question, &incident];
    let mut store = seeded_store(&fixtures, Some(100));
    store
        .snooze(RECIPIENT, &question.item_id, 100, 200)
        .unwrap();
    let page = list(&store, 10).unwrap();
    page.validate().unwrap();
    let question_row = page
        .items
        .iter()
        .find(|row| row.item.item_id == question.item_id)
        .unwrap()
        .clone();
    assert!(question_row.snooze_active);
    assert!(question_row.digest_group.starts_with("sha256:"));
    assert_eq!(question_row.digest_group.len(), 71);

    // A row may not display a due time or source sequence the committed materialization does not
    // have, and the server-derived ordering key may not be re-labelled.
    let mut rebound = question_row.clone();
    rebound.due_at_unix_ms = 99;
    assert_eq!(
        rebound.validate(),
        Err(NotificationStoreError::Invalid("page_row_binding"))
    );
    let mut rebound_cursor = question_row.clone();
    rebound_cursor.source_cursor = 9;
    assert_eq!(
        rebound_cursor.validate(),
        Err(NotificationStoreError::Invalid("page_row_binding"))
    );
    let mut reprioritized = question_row.clone();
    reprioritized.urgency = NotificationUrgency::Critical;
    assert_eq!(
        reprioritized.validate(),
        Err(NotificationStoreError::DigestMismatch)
    );
    let mut relabelled = question_row.clone();
    relabelled.digest_group = "digest:forged".to_owned();
    assert_eq!(
        relabelled.validate(),
        Err(NotificationStoreError::DigestMismatch)
    );
    let mut forged_unknown = question_row.clone();
    forged_unknown.unknown = true;
    assert_eq!(
        forged_unknown.validate(),
        Err(NotificationStoreError::Invalid("page_row_unknown"))
    );
    let mut impossible_snooze = question_row.clone();
    impossible_snooze.snoozed_until_unix_ms = None;
    assert_eq!(
        impossible_snooze.validate(),
        Err(NotificationStoreError::Invalid("snooze_active"))
    );
    let mut ack_without_read = question_row.clone();
    ack_without_read.acknowledged = true;
    assert_eq!(
        ack_without_read.validate(),
        Err(NotificationStoreError::Invalid("acknowledgement"))
    );
    let mut zero_due = question_row.clone();
    zero_due.due_at_unix_ms = 0;
    assert_eq!(
        zero_due.validate(),
        Err(NotificationStoreError::Invalid("due_at_unix_ms"))
    );
    let mut unbounded_actions = question_row.clone();
    unbounded_actions.item.actions = vec![action("a"); 17];
    assert_eq!(
        unbounded_actions.validate(),
        Err(NotificationStoreError::Invalid("actions"))
    );

    // Page-level: due state and snooze activation are recomputed against the page's server time,
    // the page cannot claim a zero clock, and the page digest binds every row.
    let mut wrong_due_state = page.clone();
    wrong_due_state.items[0].due_state = Some(NotificationDueState::Overdue);
    wrong_due_state.page_digest = wrong_due_state.digest();
    assert_eq!(
        wrong_due_state.validate(),
        Err(NotificationStoreError::Invalid("page_row_server_time"))
    );
    let mut expired_snooze = page.clone();
    let expired_index = expired_snooze
        .items
        .iter()
        .position(|row| row.item.item_id == question.item_id)
        .unwrap();
    expired_snooze.items[expired_index].snooze_active = false;
    expired_snooze.page_digest = expired_snooze.digest();
    assert_eq!(
        expired_snooze.validate(),
        Err(NotificationStoreError::Invalid("page_row_server_time"))
    );
    let mut zero_clock = page.clone();
    zero_clock.server_time_unix_ms = Some(0);
    zero_clock.page_digest = zero_clock.digest();
    assert_eq!(
        zero_clock.validate(),
        Err(NotificationStoreError::Invalid("server_time_unix_ms"))
    );
    let mut clockless_rows = page.clone();
    clockless_rows.server_time_unix_ms = None;
    clockless_rows.page_digest = clockless_rows.digest();
    assert_eq!(
        clockless_rows.validate(),
        Err(NotificationStoreError::Invalid("page_row_server_time"))
    );
    let mut forged_page_digest = page.clone();
    forged_page_digest.page_digest = "sha256:forged".to_owned();
    assert_eq!(
        forged_page_digest.validate(),
        Err(NotificationStoreError::DigestMismatch)
    );
    let mut forged_cursor = page.clone();
    forged_cursor.next_after_item_id = Some("notification:forged".to_owned());
    assert_eq!(
        forged_cursor.validate(),
        Err(NotificationStoreError::Invalid("page_next_cursor"))
    );

    // Without a server clock the store still answers pages, but stamps no due state and reports a
    // stored snooze as pending rather than inventing a classification from client time.
    let mut clockless = seeded_store(&fixtures, None);
    clockless
        .snooze(RECIPIENT, &question.item_id, 100, 200)
        .unwrap();
    let clockless_page = list(&clockless, 10).unwrap();
    clockless_page.validate().unwrap();
    assert_eq!(clockless_page.server_time_unix_ms, None);
    assert!(clockless_page
        .items
        .iter()
        .all(|row| row.due_state.is_none()));
    let clockless_question = clockless_page
        .items
        .iter()
        .find(|row| row.item.item_id == question.item_id)
        .unwrap();
    assert!(clockless_question.snooze_active);
    assert_eq!(clockless_question.snoozed_until_unix_ms, Some(200));
    assert_eq!(
        clockless.inbox_summary(),
        Err(NotificationStoreError::ServerTimeRequired)
    );
}
