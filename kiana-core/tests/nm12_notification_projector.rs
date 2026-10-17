//! NM-12 fixtures: durable inbox rebuild plus append-only retention/withdraw/supersede.
//!
//! Deny-first: a rebuild may not invent items, un-read a read row, un-acknowledge an
//! acknowledgement, move a snooze backwards, admit a retention decision for a foreign principal,
//! a stale policy, an active legal hold, an unadvanced epoch or a payload layer that still serves
//! the data; a withdrawal may not overwrite source history or an earlier terminal fact; audit
//! metadata may not smuggle a payload reference (or the reverse).
//!
//! Success path: the same committed events, lifecycle facts and read-state facts rebuild the same
//! inbox snapshot, an admitted retention decision appends exactly one lifecycle fact, and the
//! decision's data epoch becomes visible on every payload-bearing derived layer.

use kiana_core::{
    NotificationProjector, NotificationProjectorError, NotificationReadStateFact,
    NotificationRetentionDecision, NotificationStore, NotificationVisibility, RevocationLayer,
    RevocationLayerObservation, RevocationLayerState, NOTIFICATION_RETENTION_PAYLOAD_LAYERS,
};
use kiana_domain::{
    DataPayloadState, EventId, HumanAction, HumanInboxKind, MessageId, Notification,
    NotificationChannel, NotificationId, NotificationLifecycleFact, NotificationLifecycleKind,
    NotificationMaterialization, ReceiptAuditMetadata, ReceiptDataBinding, ReceiptPayloadRef,
    RequestId, RuntimeEvent,
};
use serde_json::json;

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const DIGEST_TWO: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const RECIPIENT: &str = "actor-12";
const REDACTED_SUMMARY: &str = "A redacted notification is waiting";
const DUE_AT_UNIX_MS: u64 = 500;
const EXPIRES_AT_UNIX_MS: u64 = 9_000;
const POLICY_REVISION: u64 = 3;
const OBSERVED_AT_MS: u64 = 1_000;

fn event() -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        1,
        // This is a committed, unrelated event. `run.*` is a closed notification family;
        // using an unregistered member would be rejected before the projector can exercise
        // cursor and lifecycle replay semantics.
        "workspace.restored",
        json!({"source":"runtime"}),
    )
    .unwrap()
}

#[test]
fn rebuild_and_lifecycle_replay_are_cursor_and_epoch_bound() {
    let notification = NotificationId::new();
    let first = NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        1,
        2,
        NotificationLifecycleKind::Withdraw,
        None,
        "user withdrew obsolete notification",
    )
    .unwrap();
    let committed = event();
    let mut projector =
        NotificationProjector::rebuild_from_committed(&[committed.clone()], 1, &[first.clone()])
            .unwrap();
    assert_eq!(
        projector.visibility(notification),
        NotificationVisibility::Withdrawn
    );
    assert_eq!(projector.source_cursor(), 1);
    assert_eq!(projector.data_epoch(), 2);

    projector
        .apply_committed(&[committed], 1, &[first])
        .expect("exact replay is idempotent");
    let conflicting = NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        2,
        2,
        NotificationLifecycleKind::Expire,
        None,
        "late rewrite",
    )
    .unwrap();
    assert_eq!(
        projector.apply_committed(&[event()], 2, &[conflicting]),
        Err(NotificationProjectorError::TerminalRewrite)
    );
    assert_eq!(
        projector.visibility(notification),
        NotificationVisibility::Withdrawn
    );
}

#[test]
fn supersede_and_epoch_regression_fail_closed() {
    let notification = NotificationId::new();
    let replacement = NotificationId::new();
    let supersede = NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        1,
        4,
        NotificationLifecycleKind::Supersede,
        Some(replacement),
        "replacement",
    )
    .unwrap();
    let mut projector = NotificationProjector::new();
    projector
        .apply_committed(&[event()], 1, &[supersede])
        .unwrap();
    assert_eq!(
        projector.visibility(notification),
        NotificationVisibility::Superseded
    );
    let lower_epoch = NotificationLifecycleFact::new(
        NotificationId::new(),
        EventId::new(),
        2,
        3,
        NotificationLifecycleKind::Withdraw,
        None,
        "regression",
    )
    .unwrap();
    assert_eq!(
        projector.apply_committed(&[event()], 2, &[lower_epoch]),
        Err(NotificationProjectorError::EpochRegressed)
    );
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

fn human_action() -> HumanAction {
    HumanAction {
        id: "review".to_owned(),
        label: "Review".to_owned(),
        command: "notification.review".to_owned(),
        arguments: json!({"opaque": "ref"}),
        required_fields: Vec::new(),
    }
}

fn fixture(recipient: &str, cursor: u64) -> (RuntimeEvent, NotificationMaterialization) {
    let source_event_id = EventId::new();
    let materialization = NotificationMaterialization::new(
        notification(recipient),
        source_event_id,
        cursor,
        HumanInboxKind::Question,
        "Action required",
        REDACTED_SUMMARY,
        DUE_AT_UNIX_MS,
        EXPIRES_AT_UNIX_MS,
        vec!["event:evidence-1".to_owned()],
        vec![human_action()],
        None,
    )
    .unwrap();
    let mut event = RuntimeEvent::new(
        RequestId::new(),
        cursor,
        "notification.reminder",
        json!({"source": "eventlog", "owner_id": recipient}),
    )
    .unwrap();
    event.event_id = source_event_id;
    event.data["source_cursor"] = json!(cursor);
    event.data["notification_materialization"] = json!(materialization);
    (event, materialization)
}

fn audit_metadata(
    policy_revision: u64,
    data_epoch: u64,
    source_cursor: u64,
    source_event_id: EventId,
) -> ReceiptAuditMetadata {
    ReceiptAuditMetadata {
        project_ref: "project:nm12".to_owned(),
        policy_revision,
        data_epoch,
        source_cursor,
        source_event_ids: vec![source_event_id],
        redaction_profile: "standard".to_owned(),
        retained: true,
    }
}

fn payload_ref(object_ref: &str, data_epoch: u64) -> ReceiptPayloadRef {
    ReceiptPayloadRef {
        object_ref: object_ref.to_owned(),
        source_digest: DIGEST.to_owned(),
        data_epoch,
        payload_digest: DIGEST_TWO.to_owned(),
    }
}

fn observation(
    layer: RevocationLayer,
    state: RevocationLayerState,
    data_epoch: u64,
    source_cursor: u64,
) -> RevocationLayerObservation {
    let receipt = (state == RevocationLayerState::Tombstoned).then(|| DIGEST.to_owned());
    RevocationLayerObservation::new(
        layer,
        state,
        data_epoch,
        source_cursor,
        receipt,
        OBSERVED_AT_MS,
    )
    .unwrap()
}

fn acknowledged_layers(data_epoch: u64, source_cursor: u64) -> Vec<RevocationLayerObservation> {
    NOTIFICATION_RETENTION_PAYLOAD_LAYERS
        .iter()
        .map(|layer| {
            observation(
                *layer,
                RevocationLayerState::Tombstoned,
                data_epoch,
                source_cursor,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn retention_decision(
    kind: NotificationLifecycleKind,
    replacement_notification_id: Option<NotificationId>,
    notification_id: NotificationId,
    authorized_by: &str,
    policy_revision: u64,
    previous_epoch: u64,
    data_epoch: u64,
    source_cursor: u64,
    payload_refs: Vec<ReceiptPayloadRef>,
    layer_observations: Vec<RevocationLayerObservation>,
    legal_hold_ids: Vec<String>,
) -> Result<NotificationRetentionDecision, String> {
    let source_event_id = EventId::new();
    NotificationRetentionDecision::new(
        format!("retention-{data_epoch}-{previous_epoch}"),
        kind,
        notification_id,
        replacement_notification_id,
        authorized_by,
        policy_revision,
        previous_epoch,
        data_epoch,
        source_event_id,
        source_cursor,
        audit_metadata(policy_revision, data_epoch, source_cursor, source_event_id),
        payload_refs,
        layer_observations,
        legal_hold_ids,
        "retention window closed",
    )
}

fn valid_decision(
    notification_id: NotificationId,
    previous_epoch: u64,
    data_epoch: u64,
    source_cursor: u64,
) -> NotificationRetentionDecision {
    retention_decision(
        NotificationLifecycleKind::Expire,
        None,
        notification_id,
        RECIPIENT,
        POLICY_REVISION,
        previous_epoch,
        data_epoch,
        source_cursor,
        vec![payload_ref("notification:payload", data_epoch)],
        acknowledged_layers(data_epoch, source_cursor),
        Vec::new(),
    )
    .unwrap()
}

fn reseal(decision: &mut NotificationRetentionDecision) {
    decision.decision_digest = decision.digest();
}

fn item_id_of(materialization: &NotificationMaterialization) -> String {
    format!(
        "notification:{}",
        materialization.notification.notification_id
    )
}

#[test]
fn fresh_rebuild_reproduces_the_same_inbox_snapshot_as_incremental_apply() {
    let (first, first_materialization) = fixture(RECIPIENT, 1);
    let (second, _) = fixture("actor-other", 2);
    let events = vec![first.clone(), second.clone()];

    let mut incremental = NotificationProjector::new();
    incremental.apply_committed(&events, 2, &[]).unwrap();
    let before = incremental.inbox_snapshot(RECIPIENT).unwrap();
    before.validate().unwrap();
    assert_eq!(before.source_cursor, 2);
    assert_eq!(before.entries.len(), 1);
    assert_eq!(before.entries[0].visibility, NotificationVisibility::Active);
    assert!(!before.entries[0].read);
    assert_eq!(
        before.entries[0].item.item_id,
        item_id_of(&first_materialization)
    );

    let withdraw = NotificationLifecycleFact::new(
        first_materialization.notification.notification_id,
        EventId::new(),
        2,
        1,
        NotificationLifecycleKind::Withdraw,
        None,
        "user withdrew the stale notification",
    )
    .unwrap();
    incremental
        .apply_committed(&[], 2, std::slice::from_ref(&withdraw))
        .unwrap();
    let withdrawn = incremental.inbox_snapshot(RECIPIENT).unwrap();
    assert_eq!(
        withdrawn.entries[0].visibility,
        NotificationVisibility::Withdrawn
    );
    assert_eq!(withdrawn.entries[0].item, before.entries[0].item);
    assert!(incremental
        .visible_human_inbox_items(RECIPIENT)
        .unwrap()
        .is_empty());
    assert_eq!(
        incremental
            .visible_human_inbox_items("actor-other")
            .unwrap()
            .len(),
        1
    );

    let rebuilt =
        NotificationProjector::rebuild(&events, 2, std::slice::from_ref(&withdraw), &[]).unwrap();
    let rebuilt_snapshot = rebuilt.inbox_snapshot(RECIPIENT).unwrap();
    assert_eq!(rebuilt_snapshot, withdrawn);
    assert_eq!(
        rebuilt_snapshot.snapshot_digest,
        withdrawn.snapshot_digest.clone()
    );
    assert_eq!(rebuilt.retained_source_event_count(), 2);
    assert_eq!(rebuilt.materialization_count(), 2);
    assert_eq!(rebuilt.source_cursor(), 2);

    let mut forged = withdrawn.clone();
    forged.snapshot_digest = DIGEST.to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "notification_inbox_snapshot_digest_mismatch"
    );
    let empty = NotificationProjector::new();
    assert_eq!(
        empty.inbox_snapshot(RECIPIENT),
        Err(NotificationProjectorError::ProjectionUnavailable)
    );
    assert_eq!(
        incremental.inbox_snapshot("  "),
        Err(NotificationProjectorError::Invalid(
            "recipient_id".to_owned()
        ))
    );
}

#[test]
fn withdraw_expire_and_supersede_only_append_facts_and_never_delete_source_history() {
    let (first, materialization) = fixture(RECIPIENT, 1);
    let (second, _) = fixture("actor-other", 2);
    let notification_id = materialization.notification.notification_id;
    let mut projector = NotificationProjector::new();
    projector
        .apply_committed(std::slice::from_ref(&first), 1, &[])
        .unwrap();
    let before = projector.inbox_snapshot(RECIPIENT).unwrap();
    assert_eq!(projector.materialization_count(), 1);
    assert_eq!(projector.retained_source_event_count(), 1);
    assert_eq!(before.entries[0].visibility, NotificationVisibility::Active);

    let withdraw = NotificationLifecycleFact::new(
        notification_id,
        EventId::new(),
        1,
        1,
        NotificationLifecycleKind::Withdraw,
        None,
        "withdrawn after delivery",
    )
    .unwrap();
    projector
        .apply_committed(&[], 1, std::slice::from_ref(&withdraw))
        .unwrap();
    let after = projector.inbox_snapshot(RECIPIENT).unwrap();
    assert_eq!(projector.materialization_count(), 1);
    assert_eq!(projector.retained_source_event_count(), 1);
    assert_eq!(projector.lifecycle_facts().count(), 1);
    assert_eq!(after.entries.len(), 1);
    assert_eq!(
        after.entries[0].visibility,
        NotificationVisibility::Withdrawn
    );
    assert_eq!(after.entries[0].item, before.entries[0].item);
    assert!(projector
        .visible_human_inbox_items(RECIPIENT)
        .unwrap()
        .is_empty());
    let withdrawn_digest = after.snapshot_digest.clone();

    // Exact replay is idempotent and does not append a second fact.
    projector
        .apply_committed(&[], 1, std::slice::from_ref(&withdraw))
        .unwrap();
    assert_eq!(projector.lifecycle_facts().count(), 1);
    assert_eq!(
        projector.inbox_snapshot(RECIPIENT).unwrap().snapshot_digest,
        withdrawn_digest
    );

    // The same cursor with a different fact cannot overwrite the committed one.
    let conflicting = NotificationLifecycleFact::new(
        notification_id,
        EventId::new(),
        1,
        1,
        NotificationLifecycleKind::Expire,
        None,
        "late overwrite attempt",
    )
    .unwrap();
    assert_eq!(
        projector.apply_committed(&[], 1, std::slice::from_ref(&conflicting)),
        Err(NotificationProjectorError::FactConflict)
    );
    assert_eq!(
        projector.inbox_snapshot(RECIPIENT).unwrap().snapshot_digest,
        withdrawn_digest
    );

    // A lower-cursor fact cannot rewrite an already committed higher-cursor fact.
    projector
        .apply_committed(std::slice::from_ref(&second), 2, &[])
        .unwrap();
    let earlier = NotificationLifecycleFact::new(
        notification_id,
        EventId::new(),
        1,
        2,
        NotificationLifecycleKind::Expire,
        None,
        "cursor regression",
    )
    .unwrap();
    assert_eq!(
        projector.apply_committed(&[], 2, std::slice::from_ref(&earlier)),
        Err(NotificationProjectorError::FactConflict)
    );

    // A terminal lifecycle fact cannot be rewritten by a later fact either.
    let terminal = NotificationLifecycleFact::new(
        notification_id,
        EventId::new(),
        2,
        2,
        NotificationLifecycleKind::Expire,
        None,
        "retention expired the notification",
    )
    .unwrap();
    assert_eq!(
        projector.apply_committed(&[], 2, std::slice::from_ref(&terminal)),
        Err(NotificationProjectorError::TerminalRewrite)
    );
    assert_eq!(
        projector.visibility(notification_id),
        NotificationVisibility::Withdrawn
    );
    assert_eq!(projector.retained_source_event_count(), 2);
    assert_eq!(projector.materialization_count(), 2);
}

#[test]
fn retention_admission_refuses_unauthorized_hold_and_stale_policy() {
    let (event, materialization) = fixture(RECIPIENT, 1);
    let notification_id = materialization.notification.notification_id;
    let mut projector = NotificationProjector::new();
    projector
        .apply_committed(std::slice::from_ref(&event), 1, &[])
        .unwrap();
    let valid = valid_decision(notification_id, 0, 1, 1);

    let assert_untouched = |projector: &NotificationProjector| {
        assert_eq!(
            projector.visibility(notification_id),
            NotificationVisibility::Active
        );
        assert!(projector.layer_epochs().is_empty());
        assert_eq!(projector.retention_decision_count(), 0);
        assert_eq!(projector.lifecycle_facts().count(), 0);
        assert_eq!(projector.retained_source_event_count(), 1);
    };

    assert_eq!(
        projector.admit_retention(&valid, "actor-foreign", POLICY_REVISION),
        Err(NotificationProjectorError::RetentionUnauthorized)
    );
    assert_untouched(&projector);

    assert_eq!(
        projector.admit_retention(&valid, RECIPIENT, POLICY_REVISION + 1),
        Err(NotificationProjectorError::RetentionPolicyStale)
    );
    assert_untouched(&projector);

    let held = retention_decision(
        NotificationLifecycleKind::Expire,
        None,
        notification_id,
        RECIPIENT,
        POLICY_REVISION,
        0,
        1,
        1,
        vec![payload_ref("notification:payload", 1)],
        acknowledged_layers(1, 1),
        vec!["hold:legal-1".to_owned()],
    )
    .unwrap();
    assert_eq!(
        projector.admit_retention(&held, RECIPIENT, POLICY_REVISION),
        Err(NotificationProjectorError::RetentionLegalHold)
    );
    assert_untouched(&projector);

    // The decision and its audit metadata must agree on the policy revision they were derived
    // from; drift between them is a malformed decision, not a policy choice.
    let mut drifted = valid.clone();
    drifted.audit_metadata.policy_revision = POLICY_REVISION + 1;
    reseal(&mut drifted);
    assert_eq!(
        drifted.validate().unwrap_err(),
        "notification_retention_policy_conflict"
    );
    assert_eq!(
        projector.admit_retention(&drifted, RECIPIENT, POLICY_REVISION),
        Err(NotificationProjectorError::Invalid(
            "notification_retention_policy_conflict".to_owned()
        ))
    );
    assert_untouched(&projector);

    // A blank server principal is not a principal.
    assert_eq!(
        projector.admit_retention(&valid, "  ", POLICY_REVISION),
        Err(NotificationProjectorError::Invalid(
            "server_principal_id".to_owned()
        ))
    );
    assert_untouched(&projector);
}

#[test]
fn retention_refuses_incomplete_payload_layers_and_facts_deletion() {
    let (_, materialization) = fixture(RECIPIENT, 1);
    let notification_id = materialization.notification.notification_id;

    let mut missing_index = valid_decision(notification_id, 0, 1, 1);
    missing_index.layer_observations = acknowledged_layers(1, 1)
        .into_iter()
        .filter(|observation| observation.layer != RevocationLayer::Index)
        .collect();
    reseal(&mut missing_index);
    assert_eq!(
        missing_index.validate().unwrap_err(),
        "notification_retention_layer_incomplete"
    );

    let mut facts_target = valid_decision(notification_id, 0, 1, 1);
    facts_target.layer_observations = vec![observation(
        RevocationLayer::Facts,
        RevocationLayerState::Tombstoned,
        1,
        1,
    )];
    reseal(&mut facts_target);
    assert_eq!(
        facts_target.validate().unwrap_err(),
        "notification_retention_facts_immutable"
    );

    let mut out_of_scope = valid_decision(notification_id, 0, 1, 1);
    out_of_scope.layer_observations = vec![observation(
        RevocationLayer::Checkpoint,
        RevocationLayerState::Tombstoned,
        1,
        1,
    )];
    reseal(&mut out_of_scope);
    assert_eq!(
        out_of_scope.validate().unwrap_err(),
        "notification_retention_layer_out_of_scope"
    );

    let mut still_serving = valid_decision(notification_id, 0, 1, 1);
    still_serving.layer_observations[3] = observation(
        RevocationLayer::Cache,
        RevocationLayerState::StillServing,
        1,
        1,
    );
    reseal(&mut still_serving);
    assert_eq!(
        still_serving.validate().unwrap_err(),
        "notification_retention_layer_still_serving:cache"
    );

    let mut pending = valid_decision(notification_id, 0, 1, 1);
    pending.layer_observations[2] =
        observation(RevocationLayer::Index, RevocationLayerState::Pending, 1, 1);
    reseal(&mut pending);
    assert_eq!(
        pending.validate().unwrap_err(),
        "notification_retention_layer_pending:index"
    );

    let mut unreachable = valid_decision(notification_id, 0, 1, 1);
    unreachable.layer_observations[1] = observation(
        RevocationLayer::Memory,
        RevocationLayerState::Unreachable,
        1,
        1,
    );
    reseal(&mut unreachable);
    assert_eq!(
        unreachable.validate().unwrap_err(),
        "notification_retention_layer_unreachable:memory"
    );

    let mut stale_layer = valid_decision(notification_id, 0, 2, 1);
    stale_layer.layer_observations[0] = observation(
        RevocationLayer::Artifact,
        RevocationLayerState::Tombstoned,
        1,
        1,
    );
    reseal(&mut stale_layer);
    assert_eq!(
        stale_layer.validate().unwrap_err(),
        "notification_retention_layer_epoch_stale:artifact"
    );

    let mut cursor_ahead = valid_decision(notification_id, 0, 1, 1);
    cursor_ahead.layer_observations[0] = observation(
        RevocationLayer::Artifact,
        RevocationLayerState::Tombstoned,
        1,
        2,
    );
    reseal(&mut cursor_ahead);
    assert_eq!(
        cursor_ahead.validate().unwrap_err(),
        "notification_retention_layer_cursor_ahead"
    );

    let mut out_of_order = valid_decision(notification_id, 0, 1, 1);
    out_of_order.layer_observations.swap(0, 1);
    reseal(&mut out_of_order);
    assert_eq!(
        out_of_order.validate().unwrap_err(),
        "notification_retention_layer_out_of_order"
    );

    let mut no_payload = valid_decision(notification_id, 0, 1, 1);
    no_payload.payload_refs.clear();
    reseal(&mut no_payload);
    assert_eq!(
        no_payload.validate().unwrap_err(),
        "notification_retention_payload_required"
    );

    let mut stale_payload = valid_decision(notification_id, 0, 1, 1);
    stale_payload
        .payload_refs
        .push(payload_ref("notification:other-payload", 2));
    reseal(&mut stale_payload);
    assert_eq!(
        stale_payload.validate().unwrap_err(),
        "notification_retention_payload_epoch_stale"
    );

    let mut duplicate_payload = valid_decision(notification_id, 0, 1, 1);
    duplicate_payload
        .payload_refs
        .push(payload_ref("notification:payload", 1));
    reseal(&mut duplicate_payload);
    assert_eq!(
        duplicate_payload.validate().unwrap_err(),
        "notification_retention_payload_duplicate"
    );

    let mut audit_epoch = valid_decision(notification_id, 0, 1, 1);
    audit_epoch.audit_metadata.data_epoch = 2;
    reseal(&mut audit_epoch);
    assert_eq!(
        audit_epoch.validate().unwrap_err(),
        "notification_retention_audit_epoch_mismatch"
    );

    let mut audit_not_retained = valid_decision(notification_id, 0, 1, 1);
    audit_not_retained.audit_metadata.retained = false;
    reseal(&mut audit_not_retained);
    assert_eq!(
        audit_not_retained.validate().unwrap_err(),
        "notification_retention_audit_not_retained"
    );

    let mut unbound = valid_decision(notification_id, 0, 1, 1);
    unbound.audit_metadata.source_event_ids = vec![EventId::new()];
    reseal(&mut unbound);
    assert_eq!(
        unbound.validate().unwrap_err(),
        "notification_retention_source_event_unbound"
    );

    let mut confused = valid_decision(notification_id, 0, 1, 1);
    confused.payload_refs = vec![payload_ref(&confused.source_event_id.to_string(), 1)];
    reseal(&mut confused);
    assert_eq!(
        confused.validate().unwrap_err(),
        "notification_retention_audit_ref_confusion"
    );

    // A malformed decision is refused before it can touch projection state.
    let mut projector = NotificationProjector::new();
    let (event, _) = fixture(RECIPIENT, 1);
    projector
        .apply_committed(std::slice::from_ref(&event), 1, &[])
        .unwrap();
    assert_eq!(
        projector.admit_retention(&missing_index, RECIPIENT, POLICY_REVISION),
        Err(NotificationProjectorError::Invalid(
            "notification_retention_layer_incomplete".to_owned()
        ))
    );
    assert_eq!(projector.retention_decision_count(), 0);
    assert!(projector.layer_epochs().is_empty());
}

#[test]
fn retention_admission_appends_one_lifecycle_fact_and_propagates_the_data_epoch() {
    let (first, first_materialization) = fixture(RECIPIENT, 1);
    let (second, second_materialization) = fixture(RECIPIENT, 2);
    let events = vec![first.clone(), second.clone()];
    let first_id = first_materialization.notification.notification_id;
    let second_id = second_materialization.notification.notification_id;

    let mut projector = NotificationProjector::new();
    projector.apply_committed(&events, 2, &[]).unwrap();

    let supersede = retention_decision(
        NotificationLifecycleKind::Supersede,
        Some(second_id),
        first_id,
        RECIPIENT,
        POLICY_REVISION,
        0,
        1,
        2,
        vec![payload_ref("notification:payload-first", 1)],
        acknowledged_layers(1, 2),
        Vec::new(),
    )
    .unwrap();
    let admission = projector
        .admit_retention(&supersede, RECIPIENT, POLICY_REVISION)
        .unwrap();
    admission.validate().unwrap();
    assert_eq!(admission.visibility, NotificationVisibility::Superseded);
    assert_eq!(admission.data_epoch, 1);
    assert_eq!(admission.retained_source_event_count, 2);
    assert_eq!(
        projector.visibility(first_id),
        NotificationVisibility::Superseded
    );
    assert_eq!(
        projector.visibility(second_id),
        NotificationVisibility::Active
    );
    assert_eq!(projector.data_epoch(), 1);
    for layer in NOTIFICATION_RETENTION_PAYLOAD_LAYERS {
        assert_eq!(projector.layer_epochs().get(&layer), Some(&1));
    }
    assert_eq!(projector.retention_decision_count(), 1);
    assert_eq!(projector.lifecycle_facts().count(), 1);
    assert_eq!(projector.retained_source_event_count(), 2);
    assert_eq!(projector.materialization_count(), 2);
    assert_eq!(
        projector
            .visible_human_inbox_items(RECIPIENT)
            .unwrap()
            .len(),
        1
    );
    let after_supersede = projector.inbox_snapshot(RECIPIENT).unwrap();
    assert_eq!(after_supersede.entries.len(), 2);
    assert_eq!(
        after_supersede
            .entries
            .iter()
            .filter(|entry| entry.visibility == NotificationVisibility::Active)
            .count(),
        1
    );

    // An exact replay returns the same admission and appends nothing.
    let replay = projector
        .admit_retention(&supersede, RECIPIENT, POLICY_REVISION)
        .unwrap();
    assert_eq!(replay, admission);
    assert_eq!(projector.lifecycle_facts().count(), 1);
    assert_eq!(projector.retention_decision_count(), 1);

    // The replacement notification is an independent source object: superseding one notification
    // does not expire or withdraw the replacement.
    let expire = valid_decision(second_id, 1, 2, 2);
    let expire_admission = projector
        .admit_retention(&expire, RECIPIENT, POLICY_REVISION)
        .unwrap();
    assert_eq!(expire_admission.visibility, NotificationVisibility::Expired);
    assert_eq!(projector.data_epoch(), 2);
    for layer in NOTIFICATION_RETENTION_PAYLOAD_LAYERS {
        assert_eq!(projector.layer_epochs().get(&layer), Some(&2));
    }
    assert_eq!(
        projector.visibility(first_id),
        NotificationVisibility::Superseded
    );
    assert_eq!(
        projector.visibility(second_id),
        NotificationVisibility::Expired
    );

    // A decision that does not advance from the current epoch is refused.
    let drift = valid_decision(second_id, 0, 1, 2);
    assert_eq!(
        projector.admit_retention(&drift, RECIPIENT, POLICY_REVISION),
        Err(NotificationProjectorError::RetentionEpochConflict)
    );
    // A decision for a notification the committed projection never saw is refused.
    let unknown = valid_decision(NotificationId::new(), 2, 3, 2);
    assert_eq!(
        projector.admit_retention(&unknown, RECIPIENT, POLICY_REVISION),
        Err(NotificationProjectorError::UnknownNotification)
    );
    // A decision that runs ahead of the committed cursor is refused.
    let ahead = valid_decision(first_id, 2, 3, 3);
    assert_eq!(
        projector.admit_retention(&ahead, RECIPIENT, POLICY_REVISION),
        Err(NotificationProjectorError::CursorConflict)
    );

    // The retained decisions replay into the same per-layer epochs and the same snapshot.
    let decisions = vec![supersede.clone(), expire.clone()];
    let rebuilt = NotificationProjector::rebuild_with_retention(
        &events,
        2,
        &[],
        &[],
        &decisions,
        RECIPIENT,
        POLICY_REVISION,
    )
    .unwrap();
    assert_eq!(rebuilt.layer_epochs(), projector.layer_epochs());
    assert_eq!(rebuilt.data_epoch(), projector.data_epoch());
    assert_eq!(
        rebuilt.inbox_snapshot(RECIPIENT).unwrap().snapshot_digest,
        projector.inbox_snapshot(RECIPIENT).unwrap().snapshot_digest
    );
    assert_eq!(rebuilt.retained_source_event_count(), 2);

    // Replaying the appended facts together with the decisions takes the idempotent branch and
    // still converges to the same state.
    let mut facts = projector.lifecycle_facts().cloned().collect::<Vec<_>>();
    facts.sort_by_key(|fact| (fact.source_cursor, fact.data_epoch));
    let replayed = NotificationProjector::rebuild_with_retention(
        &events,
        2,
        &facts,
        &[],
        &decisions,
        RECIPIENT,
        POLICY_REVISION,
    )
    .unwrap();
    assert_eq!(replayed.layer_epochs(), projector.layer_epochs());
    assert_eq!(
        replayed.inbox_snapshot(RECIPIENT).unwrap().snapshot_digest,
        projector.inbox_snapshot(RECIPIENT).unwrap().snapshot_digest
    );

    // The admission receipt carries no approval/decision field; a forged one fails to decode.
    let mut forged = serde_json::to_value(&admission).unwrap();
    forged["approved"] = json!(true);
    assert!(serde_json::from_value::<kiana_core::NotificationRetentionAdmission>(forged).is_err());
}

#[test]
fn read_state_facts_rebuild_the_same_unread_state_from_the_source_cursor() {
    let (event, materialization) = fixture(RECIPIENT, 1);
    let item_id = item_id_of(&materialization);
    let mut projector = NotificationProjector::new();
    projector
        .apply_committed(std::slice::from_ref(&event), 1, &[])
        .unwrap();
    let unread = projector.inbox_snapshot(RECIPIENT).unwrap();
    assert!(!unread.entries[0].read);
    assert!(!unread.entries[0].acknowledged);

    let read_fact =
        NotificationReadStateFact::new(item_id.clone(), EventId::new(), 1, 1, true, false, None)
            .unwrap();
    projector
        .apply_read_state_facts(std::slice::from_ref(&read_fact))
        .unwrap();
    let read = projector.inbox_snapshot(RECIPIENT).unwrap();
    assert!(read.entries[0].read);
    assert!(!read.entries[0].acknowledged);
    assert_eq!(projector.read_state_count(), 1);
    assert_eq!(projector.read_state(&item_id), Some(&read_fact));

    let rebuilt = NotificationProjector::rebuild(
        std::slice::from_ref(&event),
        1,
        &[],
        std::slice::from_ref(&read_fact),
    )
    .unwrap();
    assert_eq!(rebuilt.inbox_snapshot(RECIPIENT).unwrap(), read);
    assert_eq!(rebuilt.retained_source_event_count(), 1);

    let unknown = NotificationReadStateFact::new(
        "notification:00000000-0000-0000-0000-000000000000",
        EventId::new(),
        1,
        1,
        true,
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        projector.apply_read_state_facts(&[unknown]),
        Err(NotificationProjectorError::UnknownNotification)
    );

    let ahead =
        NotificationReadStateFact::new(item_id.clone(), EventId::new(), 2, 2, true, false, None)
            .unwrap();
    assert_eq!(
        projector.apply_read_state_facts(&[ahead]),
        Err(NotificationProjectorError::CursorConflict)
    );

    let same_revision =
        NotificationReadStateFact::new(item_id.clone(), EventId::new(), 1, 1, true, false, None)
            .unwrap();
    assert_eq!(
        projector.apply_read_state_facts(&[same_revision]),
        Err(NotificationProjectorError::ReadStateConflict)
    );

    let unread_again =
        NotificationReadStateFact::new(item_id.clone(), EventId::new(), 1, 2, false, false, None)
            .unwrap();
    assert_eq!(
        projector.apply_read_state_facts(&[unread_again]),
        Err(NotificationProjectorError::ReadStateRegression)
    );

    let acknowledged =
        NotificationReadStateFact::new(item_id.clone(), EventId::new(), 1, 2, true, true, None)
            .unwrap();
    projector
        .apply_read_state_facts(std::slice::from_ref(&acknowledged))
        .unwrap();
    let snoozed = NotificationReadStateFact::new(
        item_id.clone(),
        EventId::new(),
        1,
        3,
        true,
        true,
        Some(10_000),
    )
    .unwrap();
    projector
        .apply_read_state_facts(std::slice::from_ref(&snoozed))
        .unwrap();
    let snoozed_snapshot = projector.inbox_snapshot(RECIPIENT).unwrap();
    assert!(snoozed_snapshot.entries[0].read);
    assert!(snoozed_snapshot.entries[0].acknowledged);
    assert_eq!(
        snoozed_snapshot.entries[0].snoozed_until_unix_ms,
        Some(10_000)
    );

    let backwards = NotificationReadStateFact::new(
        item_id.clone(),
        EventId::new(),
        1,
        4,
        true,
        true,
        Some(9_000),
    )
    .unwrap();
    assert_eq!(
        projector.apply_read_state_facts(&[backwards]),
        Err(NotificationProjectorError::ReadStateRegression)
    );
    let cleared =
        NotificationReadStateFact::new(item_id.clone(), EventId::new(), 1, 4, true, true, None)
            .unwrap();
    assert_eq!(
        projector.apply_read_state_facts(&[cleared]),
        Err(NotificationProjectorError::ReadStateRegression)
    );

    // An acknowledgement without a read never becomes a fact.
    assert_eq!(
        NotificationReadStateFact::new(item_id.clone(), EventId::new(), 1, 5, false, true, None)
            .unwrap_err(),
        "notification_read_state_acknowledgement"
    );

    // Exact replay of the latest fact is idempotent.
    projector
        .apply_read_state_facts(std::slice::from_ref(&snoozed))
        .unwrap();
    assert_eq!(projector.read_state_count(), 1);
    assert_eq!(
        projector.inbox_snapshot(RECIPIENT).unwrap().snapshot_digest,
        snoozed_snapshot.snapshot_digest
    );
}

#[test]
fn presentation_mutations_become_replayable_read_state_facts() {
    let (event, materialization) = fixture(RECIPIENT, 1);
    let item_id = item_id_of(&materialization);
    let mut store = NotificationStore::new();
    store.bind_server_recipient(RECIPIENT).unwrap();
    store
        .apply_committed(std::slice::from_ref(&event), 1)
        .unwrap();
    let mutation = store.mark_read(RECIPIENT, &item_id, 1_000).unwrap();
    let fact = NotificationReadStateFact::from_projection_mutation(&mutation, EventId::new())
        .expect("a valid receipt converts into a replayable fact");
    assert!(fact.read);
    assert!(!fact.acknowledged);

    let mut projector = NotificationProjector::new();
    projector
        .apply_committed(std::slice::from_ref(&event), 1, &[])
        .unwrap();
    projector
        .apply_read_state_facts(std::slice::from_ref(&fact))
        .unwrap();
    assert!(projector.inbox_snapshot(RECIPIENT).unwrap().entries[0].read);

    // The same receipt committed under the same event id replays idempotently.
    let replay =
        NotificationReadStateFact::from_projection_mutation(&mutation, fact.source_event_id)
            .unwrap();
    assert_eq!(replay, fact);
    projector.apply_read_state_facts(&[replay]).unwrap();
    assert_eq!(projector.read_state_count(), 1);

    // The same receipt claiming a different committed event is a conflict, not an overwrite.
    let other_event =
        NotificationReadStateFact::from_projection_mutation(&mutation, EventId::new()).unwrap();
    assert_ne!(other_event.fact_digest, fact.fact_digest);
    assert_eq!(
        projector.apply_read_state_facts(&[other_event]),
        Err(NotificationProjectorError::ReadStateConflict)
    );
    assert_eq!(projector.read_state_count(), 1);
}

#[test]
fn audit_metadata_and_payload_refs_never_cross_the_boundary() {
    let (_, materialization) = fixture(RECIPIENT, 1);
    let notification_id = materialization.notification.notification_id;
    let decision = valid_decision(notification_id, 0, 1, 1);
    let encoded = serde_json::to_value(&decision).unwrap();
    let decoded: NotificationRetentionDecision = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, decision);

    // A payload reference smuggled into audit metadata fails to decode.
    let mut smuggled = encoded.clone();
    smuggled["audit_metadata"]["payload_refs"] = json!([payload_ref("notification:payload", 1)]);
    assert!(serde_json::from_value::<NotificationRetentionDecision>(smuggled).is_err());

    // An audit field smuggled into a payload reference fails to decode.
    let mut smuggled = encoded.clone();
    smuggled["payload_refs"][0]["retained"] = json!(true);
    assert!(serde_json::from_value::<NotificationRetentionDecision>(smuggled).is_err());

    // A lifecycle fact cannot carry a payload reference either.
    let fact = NotificationLifecycleFact::new(
        notification_id,
        EventId::new(),
        1,
        1,
        NotificationLifecycleKind::Withdraw,
        None,
        "withdrawn",
    )
    .unwrap();
    let mut fact_value = serde_json::to_value(&fact).unwrap();
    fact_value["payload_refs"] = json!(["notification:payload"]);
    assert!(serde_json::from_value::<NotificationLifecycleFact>(fact_value).is_err());

    // Redaction removes payload references without authorizing a read at the old epoch.
    let binding = ReceiptDataBinding::new(
        DIGEST,
        audit_metadata(POLICY_REVISION, 1, 1, EventId::new()),
        vec![payload_ref("notification:payload", 1)],
        DataPayloadState::Available,
    )
    .unwrap();
    assert!(binding.authorize_payload(1).is_ok());
    let redacted = binding.redact_payload_refs().unwrap();
    assert!(redacted.payload_refs.is_empty());
    assert_eq!(
        redacted.authorize_payload(1).unwrap_err(),
        "receipt_payload_authorization_denied"
    );
    assert_eq!(
        binding.authorize_payload(2).unwrap_err(),
        "receipt_payload_authorization_denied"
    );

    // Payload references bound to a different epoch than the audit metadata are refused.
    let mut mismatched = binding.clone();
    mismatched.payload_refs[0].data_epoch = 2;
    mismatched.binding_digest = mismatched.digest();
    assert_eq!(
        mismatched.validate().unwrap_err(),
        "receipt_payload_ref_boundary_invalid"
    );
}
