//! NM-06 fixtures: a committed notification materialization may only become visible together with
//! the original HumanTask binding that owns the task status.
//!
//! Deny-first: approval/review/acceptance/incident/reconciliation facts that miss their source
//! event, evidence, due window or decider are rejected and stay invisible.  Success path: every
//! actionable fact projects a redacted summary, action refs, due/expiry and source cursor while
//! the HumanTask authority record stays the only owner of the task status.

use kiana_core::{materialize_notification_inbox, NotificationMaterializer};
use kiana_domain::{
    EventId, HumanAction, HumanInboxItem, HumanInboxKind, HumanTaskBridge, MessageId, Notification,
    NotificationChannel, NotificationMaterialization, RequestId, RuntimeEvent,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const REDACTED_SUMMARY: &str = "A redacted action is waiting";
const ACTION_ID: &str = "approve";
const EVIDENCE_REF: &str = "event:evidence-1";
const DUE_AT_UNIX_MS: u64 = 500;
const EXPIRES_AT_UNIX_MS: u64 = 9_000;
const ACTIONABLE_KINDS: [HumanInboxKind; 5] = [
    HumanInboxKind::Approval,
    HumanInboxKind::Review,
    HumanInboxKind::Acceptance,
    HumanInboxKind::Incident,
    HumanInboxKind::Reconciliation,
];

fn event_kind_for(kind: &HumanInboxKind) -> &'static str {
    match kind {
        HumanInboxKind::Approval => "approval.requested",
        HumanInboxKind::Review => "communication.evidence",
        HumanInboxKind::Acceptance => "run.completed",
        HumanInboxKind::Incident => "communication.incident",
        HumanInboxKind::Reconciliation => "connector.reconciliation",
        HumanInboxKind::Question => "notification.reminder",
        HumanInboxKind::Feedback => "communication.status_report",
    }
}

fn human_action() -> HumanAction {
    HumanAction {
        id: ACTION_ID.to_owned(),
        label: "Approve".to_owned(),
        command: "approval".to_owned(),
        arguments: json!({"approval_id": "opaque"}),
        required_fields: Vec::new(),
    }
}

fn notification(recipient: &str) -> Notification {
    Notification::new(
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
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn bridge_with_target_kind(
    target_kind: &str,
    recipient: &str,
    source_event_id: EventId,
    source_cursor: u64,
    due_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    evidence_refs: Vec<String>,
    actions: Vec<HumanAction>,
) -> Result<HumanTaskBridge, String> {
    HumanTaskBridge::new(
        format!("task-{target_kind}"),
        target_kind,
        format!("{target_kind}-1"),
        1,
        DIGEST,
        source_event_id,
        source_cursor,
        recipient,
        due_at_unix_ms,
        expires_at_unix_ms,
        evidence_refs,
        actions,
    )
}

fn bridge(
    kind: &HumanInboxKind,
    recipient: &str,
    source_event_id: EventId,
    source_cursor: u64,
) -> Result<HumanTaskBridge, String> {
    bridge_with_target_kind(
        kind.human_task_target_kind().unwrap_or("question"),
        recipient,
        source_event_id,
        source_cursor,
        DUE_AT_UNIX_MS,
        EXPIRES_AT_UNIX_MS,
        vec![EVIDENCE_REF.to_owned()],
        vec![human_action()],
    )
}

#[allow(clippy::too_many_arguments)]
fn materialization_with(
    recipient: &str,
    kind: HumanInboxKind,
    source_event_id: EventId,
    source_cursor: u64,
    due_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    evidence_refs: Vec<String>,
    actions: Vec<HumanAction>,
    human_task: Option<HumanTaskBridge>,
) -> Result<NotificationMaterialization, String> {
    NotificationMaterialization::new(
        notification(recipient),
        source_event_id,
        source_cursor,
        kind,
        "Action required",
        REDACTED_SUMMARY,
        due_at_unix_ms,
        expires_at_unix_ms,
        evidence_refs,
        actions,
        human_task,
    )
}

fn envelope(kind: &HumanInboxKind, recipient: &str, cursor: u64, payload: Value) -> RuntimeEvent {
    let mut event = RuntimeEvent::new(
        RequestId::new(),
        cursor,
        event_kind_for(kind),
        json!({"source": "eventlog", "owner_id": recipient}),
    )
    .unwrap();
    event.data["source_cursor"] = json!(cursor);
    event.data["notification_materialization"] = payload;
    event
}

fn actionable_fixture(
    kind: &HumanInboxKind,
    recipient: &str,
    cursor: u64,
) -> (RuntimeEvent, NotificationMaterialization) {
    let source_event_id = EventId::new();
    let materialization = materialization_with(
        recipient,
        kind.clone(),
        source_event_id,
        cursor,
        DUE_AT_UNIX_MS,
        EXPIRES_AT_UNIX_MS,
        vec![EVIDENCE_REF.to_owned()],
        vec![human_action()],
        Some(bridge(kind, recipient, source_event_id, cursor).unwrap()),
    )
    .unwrap();
    let mut event = envelope(kind, recipient, cursor, json!(materialization));
    event.event_id = source_event_id;
    (event, materialization)
}

#[test]
fn materializer_requires_committed_source_and_complete_materialization() {
    let (mut event, materialization) = actionable_fixture(&HumanInboxKind::Approval, "actor-1", 1);
    event.data["notification_materialization"] = json!({
        "schema": materialization.schema,
        "notification": materialization.notification,
        "source_event_id": EventId::new(),
        "source_cursor": 0,
        "kind": "approval",
        "title": "Action required",
        "redacted_summary": REDACTED_SUMMARY,
        "due_at_unix_ms": DUE_AT_UNIX_MS,
        "expires_at_unix_ms": EXPIRES_AT_UNIX_MS,
        "evidence_refs": [],
        "actions": [],
        "human_task": null,
        "materialization_digest": DIGEST,
    });
    let mut materializer = NotificationMaterializer::new();
    assert!(materializer.apply_committed(&[event], 1).is_err());
    assert_eq!(materializer.source_cursor(), 0);
    assert!(materializer
        .human_inbox_items("actor-1")
        .unwrap()
        .is_empty());
}

#[test]
fn materializer_rejects_cursor_gaps_and_replays_exactly() {
    let (first, _) = actionable_fixture(&HumanInboxKind::Approval, "actor-1", 1);
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

    let (mut conflict, _) = actionable_fixture(&HumanInboxKind::Approval, "actor-1", 1);
    conflict.event_id = first.event_id;
    conflict.data["source_cursor"] = json!(1);
    conflict.data["notification_materialization"]["redacted_summary"] =
        json!("different redacted summary");
    assert!(materializer.apply_committed(&[conflict], 1).is_err());
    assert_eq!(materializer, before);
}

#[test]
fn materializer_filters_recipient_and_wrong_decider_and_orders_items() {
    let (first, _) = actionable_fixture(&HumanInboxKind::Approval, "actor-1", 1);
    let (second, _) = actionable_fixture(&HumanInboxKind::Approval, "actor-2", 2);
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
        Vec::<HumanInboxItem>::new()
    );

    // A question is not an actionable fact: it needs no HumanTask bridge and stays visible to its
    // own recipient only.  This keeps the actionable-only rule from widening into other kinds.
    let question = materialization_with(
        "actor-3",
        HumanInboxKind::Question,
        EventId::new(),
        3,
        DUE_AT_UNIX_MS,
        EXPIRES_AT_UNIX_MS,
        vec![EVIDENCE_REF.to_owned()],
        vec![human_action()],
        None,
    )
    .unwrap();
    let question_event = envelope(&HumanInboxKind::Question, "actor-3", 3, json!(question));
    let mut materializer = materializer.clone();
    materializer.apply_committed(&[question_event], 3).unwrap();
    let questions = materializer.human_inbox_items("actor-3").unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].kind, HumanInboxKind::Question);
    assert!(questions[0].detail["human_task"].is_null());
    assert_eq!(
        materializer.human_inbox_items("actor-1").unwrap(),
        actor_one
    );
}

#[test]
fn actionable_kinds_require_the_original_human_task_binding() {
    for kind in ACTIONABLE_KINDS {
        let source_event_id = EventId::new();
        let unbound = materialization_with(
            "actor-1",
            kind.clone(),
            source_event_id,
            1,
            DUE_AT_UNIX_MS,
            EXPIRES_AT_UNIX_MS,
            vec![EVIDENCE_REF.to_owned()],
            vec![human_action()],
            None,
        );
        assert_eq!(
            unbound.unwrap_err(),
            "notification_materialization_human_task_required"
        );

        let wrong_target = bridge_with_target_kind(
            if kind == HumanInboxKind::Approval {
                "review"
            } else {
                "approval"
            },
            "actor-1",
            source_event_id,
            1,
            DUE_AT_UNIX_MS,
            EXPIRES_AT_UNIX_MS,
            vec![EVIDENCE_REF.to_owned()],
            vec![human_action()],
        )
        .unwrap();
        let mismatched_target = materialization_with(
            "actor-1",
            kind.clone(),
            source_event_id,
            1,
            DUE_AT_UNIX_MS,
            EXPIRES_AT_UNIX_MS,
            vec![EVIDENCE_REF.to_owned()],
            vec![human_action()],
            Some(wrong_target),
        );
        assert_eq!(
            mismatched_target.unwrap_err(),
            "notification_materialization_human_task_target_kind_mismatch"
        );

        let wrong_decider = materialization_with(
            "actor-1",
            kind.clone(),
            source_event_id,
            1,
            DUE_AT_UNIX_MS,
            EXPIRES_AT_UNIX_MS,
            vec![EVIDENCE_REF.to_owned()],
            vec![human_action()],
            Some(bridge(&kind, "actor-other", source_event_id, 1).unwrap()),
        );
        assert_eq!(
            wrong_decider.unwrap_err(),
            "notification_materialization_human_task_mismatch"
        );

        let task = bridge(&kind, "actor-1", source_event_id, 1).unwrap();
        let no_evidence = materialization_with(
            "actor-1",
            kind.clone(),
            source_event_id,
            1,
            DUE_AT_UNIX_MS,
            EXPIRES_AT_UNIX_MS,
            Vec::new(),
            vec![human_action()],
            Some(task.clone()),
        );
        assert_eq!(
            no_evidence.unwrap_err(),
            "notification_materialization_evidence_required"
        );

        let no_due = materialization_with(
            "actor-1",
            kind.clone(),
            source_event_id,
            1,
            0,
            EXPIRES_AT_UNIX_MS,
            vec![EVIDENCE_REF.to_owned()],
            vec![human_action()],
            Some(task.clone()),
        );
        assert_eq!(
            no_due.unwrap_err(),
            "notification_materialization_header_invalid"
        );

        let no_expiry_window = materialization_with(
            "actor-1",
            kind.clone(),
            source_event_id,
            1,
            DUE_AT_UNIX_MS,
            DUE_AT_UNIX_MS,
            vec![EVIDENCE_REF.to_owned()],
            vec![human_action()],
            Some(task.clone()),
        );
        assert_eq!(
            no_expiry_window.unwrap_err(),
            "notification_materialization_header_invalid"
        );

        let no_source_cursor = materialization_with(
            "actor-1",
            kind.clone(),
            source_event_id,
            0,
            DUE_AT_UNIX_MS,
            EXPIRES_AT_UNIX_MS,
            vec![EVIDENCE_REF.to_owned()],
            vec![human_action()],
            Some(task.clone()),
        );
        assert_eq!(
            no_source_cursor.unwrap_err(),
            "notification_materialization_header_invalid"
        );

        let nil_source_event = EventId::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
        let no_source_event = materialization_with(
            "actor-1",
            kind.clone(),
            nil_source_event,
            1,
            DUE_AT_UNIX_MS,
            EXPIRES_AT_UNIX_MS,
            vec![EVIDENCE_REF.to_owned()],
            vec![human_action()],
            Some(task),
        );
        assert_eq!(
            no_source_event.unwrap_err(),
            "notification_materialization_header_invalid"
        );
    }
}

#[test]
fn materializer_never_visible_when_source_evidence_due_or_decider_is_missing() {
    for kind in ACTIONABLE_KINDS {
        for (mutation, expected) in [
            (
                "missing_evidence",
                "notification_materialization_evidence_required",
            ),
            ("missing_due", "notification_materialization_header_invalid"),
            (
                "missing_human_task",
                "notification_materialization_human_task_required",
            ),
            ("wrong_decider", "human_task_bridge_digest_mismatch"),
            (
                "foreign_source_event",
                "notification_materializer_source_event_mismatch",
            ),
        ] {
            let (_, materialization) = actionable_fixture(&kind, "actor-1", 1);
            let event = if mutation == "foreign_source_event" {
                // The materialization itself stays internally consistent, but it belongs to a
                // different committed source event: the envelope must refuse it.
                envelope(&kind, "actor-1", 1, json!(materialization))
            } else {
                let mut payload = serde_json::to_value(&materialization).unwrap();
                match mutation {
                    "missing_evidence" => payload["evidence_refs"] = json!([]),
                    "missing_due" => {
                        payload["due_at_unix_ms"] = json!(0);
                        payload["human_task"]["due_at_unix_ms"] = json!(0);
                    }
                    "missing_human_task" => payload["human_task"] = Value::Null,
                    "wrong_decider" => {
                        payload["human_task"]["decider_principal_id"] = json!("actor-intruder");
                    }
                    other => panic!("unknown mutation: {other}"),
                }
                envelope(&kind, "actor-1", 1, payload)
            };
            let mut materializer = NotificationMaterializer::new();
            let before = materializer.clone();
            assert_eq!(
                materializer.apply_committed(&[event], 1),
                Err(expected.to_owned()),
                "{kind:?} accepted a committed fact mutated with {mutation}"
            );
            assert_eq!(materializer, before, "{kind:?}/{mutation} mutated state");
            assert!(materializer
                .human_inbox_items("actor-1")
                .unwrap()
                .is_empty());
        }
    }
}

#[test]
fn materializer_materializes_every_kind_with_summary_actions_due_and_source_authority() {
    let mut events = Vec::new();
    let mut fixtures = Vec::new();
    for (index, kind) in ACTIONABLE_KINDS.iter().enumerate() {
        let cursor = index as u64 + 1;
        let (event, materialization) = actionable_fixture(kind, "actor-1", cursor);
        fixtures.push((kind.clone(), cursor, event.event_id, materialization));
        events.push(event);
    }
    let question_cursor = ACTIONABLE_KINDS.len() as u64 + 1;
    let question = materialization_with(
        "actor-1",
        HumanInboxKind::Question,
        EventId::new(),
        question_cursor,
        DUE_AT_UNIX_MS,
        EXPIRES_AT_UNIX_MS,
        vec![EVIDENCE_REF.to_owned()],
        vec![human_action()],
        None,
    )
    .unwrap();
    let question_event = envelope(
        &HumanInboxKind::Question,
        "actor-1",
        question_cursor,
        json!(question),
    );
    events.push(question_event);

    let mut materializer = NotificationMaterializer::new();
    materializer
        .apply_committed(&events, question_cursor)
        .unwrap();
    assert_eq!(materializer.source_cursor(), question_cursor);
    assert_eq!(materializer.materializations().count(), 6);
    assert_eq!(materializer.source_event_ids().count(), 6);

    let items = materializer.human_inbox_items("actor-1").unwrap();
    assert_eq!(items.len(), 6);
    for (kind, cursor, event_id, materialization) in &fixtures {
        let item = items
            .iter()
            .find(|item| item.kind == *kind)
            .unwrap_or_else(|| panic!("{kind:?} item is missing"));
        assert_eq!(
            item.item_id,
            format!(
                "notification:{}",
                materialization.notification.notification_id
            )
        );
        assert_eq!(item.source_ref, format!("event:{event_id}"));
        assert_eq!(item.title, "Action required");
        assert_eq!(item.actions, materialization.actions);
        assert_eq!(item.detail["schema"], json!(materialization.schema));
        assert_eq!(item.detail["redacted_summary"], json!(REDACTED_SUMMARY));
        assert_eq!(item.detail["due_at_unix_ms"], json!(DUE_AT_UNIX_MS));
        assert_eq!(item.detail["expires_at_unix_ms"], json!(EXPIRES_AT_UNIX_MS));
        assert_eq!(item.detail["source_cursor"], json!(cursor));
        assert_eq!(item.detail["source_event_id"], json!(event_id));
        assert_eq!(item.detail["evidence_refs"], json!([EVIDENCE_REF]));

        let task = item.detail["human_task"]
            .as_object()
            .unwrap_or_else(|| panic!("{kind:?} lost its HumanTask bridge"));
        let target_kind = kind.human_task_target_kind().unwrap();
        assert_eq!(task["task_id"], json!(format!("task-{target_kind}")));
        assert_eq!(task["target_kind"], json!(target_kind));
        assert_eq!(task["target_revision"], json!(1));
        assert_eq!(task["target_digest"], json!(DIGEST));
        assert_eq!(task["decider_principal_id"], json!("actor-1"));
        assert_eq!(task["source_event_id"], json!(event_id));
        assert_eq!(task["source_cursor"], json!(cursor));
        assert_eq!(task["due_at_unix_ms"], json!(DUE_AT_UNIX_MS));
        assert_eq!(task["expires_at_unix_ms"], json!(EXPIRES_AT_UNIX_MS));
        // The bridge carries identity and bindings only: no copied HumanTask status may exist in
        // the materialized view, so the original object stays the authority.
        assert_eq!(task.len(), 14);
        assert!(
            task.keys().all(|key| !key.contains("status")),
            "{kind:?} copied a task status into the notification"
        );
        assert!(item.detail.get("task_status").is_none());
    }
    let question_item = items
        .iter()
        .find(|item| item.kind == HumanInboxKind::Question)
        .unwrap();
    assert!(question_item.detail["human_task"].is_null());
    assert_eq!(
        question_item.item_id,
        format!("notification:{}", question.notification.notification_id)
    );

    // An exact replay of the same committed page stays byte-identical for the consumer.
    materializer
        .apply_committed(&events, question_cursor)
        .unwrap();
    assert_eq!(materializer.human_inbox_items("actor-1").unwrap(), items);
    let mut rebuilt = NotificationMaterializer::new();
    rebuilt.apply_committed(&events, question_cursor).unwrap();
    assert_eq!(materializer, rebuilt);
}

#[test]
fn materializer_refuses_a_class_that_cannot_own_the_materialized_kind() {
    // A committed approval fact cannot materialize a reconciliation task.
    let (mut approval_event, materialization) =
        actionable_fixture(&HumanInboxKind::Approval, "actor-1", 1);
    assert_eq!(materialization.kind, HumanInboxKind::Approval);
    let reconciliation = materialization_with(
        "actor-1",
        HumanInboxKind::Reconciliation,
        approval_event.event_id,
        1,
        DUE_AT_UNIX_MS,
        EXPIRES_AT_UNIX_MS,
        vec![EVIDENCE_REF.to_owned()],
        vec![human_action()],
        Some(
            bridge(
                &HumanInboxKind::Reconciliation,
                "actor-1",
                approval_event.event_id,
                1,
            )
            .unwrap(),
        ),
    )
    .unwrap();
    approval_event.data["notification_materialization"] = json!(reconciliation);
    let mut materializer = NotificationMaterializer::new();
    let before = materializer.clone();
    assert_eq!(
        materializer
            .apply_committed(&[approval_event], 1)
            .unwrap_err(),
        "notification_materializer_event_class_mismatch"
    );
    assert_eq!(materializer, before);
    assert!(materializer
        .human_inbox_items("actor-1")
        .unwrap()
        .is_empty());

    // An incident fact cannot materialize an approval task either.
    let (mut incident_event, _) = actionable_fixture(&HumanInboxKind::Incident, "actor-1", 1);
    let approval = materialization_with(
        "actor-1",
        HumanInboxKind::Approval,
        incident_event.event_id,
        1,
        DUE_AT_UNIX_MS,
        EXPIRES_AT_UNIX_MS,
        vec![EVIDENCE_REF.to_owned()],
        vec![human_action()],
        Some(
            bridge(
                &HumanInboxKind::Approval,
                "actor-1",
                incident_event.event_id,
                1,
            )
            .unwrap(),
        ),
    )
    .unwrap();
    incident_event.data["notification_materialization"] = json!(approval);
    assert_eq!(
        NotificationMaterializer::new()
            .apply_committed(&[incident_event], 1)
            .unwrap_err(),
        "notification_materializer_event_class_mismatch"
    );

    // Unknown members of a closed notification family stay unregistered.
    let (mut unregistered, _) = actionable_fixture(&HumanInboxKind::Approval, "actor-1", 1);
    unregistered.kind = "approval.bogus".to_owned();
    assert_eq!(
        NotificationMaterializer::new()
            .apply_committed(&[unregistered], 1)
            .unwrap_err(),
        "notification_event_kind_unregistered"
    );
}

#[test]
fn bridge_identity_tampering_fails_closed() {
    let (_, materialization) = actionable_fixture(&HumanInboxKind::Approval, "actor-1", 1);
    let original = materialization.human_task.clone().unwrap();
    for mutation in ["target_revision", "decider_principal_id", "source_cursor"] {
        let mut payload = serde_json::to_value(&original).unwrap();
        match mutation {
            "target_revision" => payload["target_revision"] = json!(2),
            "decider_principal_id" => payload["decider_principal_id"] = json!("actor-intruder"),
            "source_cursor" => payload["source_cursor"] = json!(2),
            other => panic!("unknown mutation: {other}"),
        }
        let tampered: HumanTaskBridge = serde_json::from_value(payload).unwrap();
        assert_eq!(
            tampered.validate().unwrap_err(),
            "human_task_bridge_digest_mismatch",
            "{mutation} must stay bound to the original object digest"
        );
    }
    assert_eq!(
        original.validate(),
        Ok(()),
        "the untouched bridge stays valid"
    );
}

#[test]
fn platform_bridge_projects_server_scoped_items_and_fails_closed() {
    let (event, _) = actionable_fixture(&HumanInboxKind::Approval, "actor-1", 1);
    let mut materializer = NotificationMaterializer::new();
    materializer.apply_committed(&[event], 1).unwrap();

    let items = materialize_notification_inbox(&materializer, "actor-1").unwrap();
    assert_eq!(items, materializer.human_inbox_items("actor-1").unwrap());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, HumanInboxKind::Approval);

    let blank = materialize_notification_inbox(&materializer, "   ").unwrap_err();
    assert!(
        blank
            .to_string()
            .contains("notification_materializer_recipient_required"),
        "unexpected platform error: {blank}"
    );
    assert!(
        materialize_notification_inbox(&NotificationMaterializer::new(), "actor-1")
            .unwrap()
            .is_empty()
    );
}
