use kiana_client::{
    UiEntityKey, UiEntityKind, UiEntityLifecycle, UiEntityStore, UiEntityStoreSnapshot,
    UiOptimisticUpdate, UiStoreChange, UiStoreError, UiStoreEvent, UiStoreGapReason, UiStoreScope,
    UI_ENTITY_STORE_MAX_ENTITIES, UI_ENTITY_STORE_MAX_EVENTS, UI_ENTITY_STORE_MAX_UNKNOWN,
    UI_ENTITY_STORE_SNAPSHOT_SCHEMA,
};
use kiana_protocol::{
    AuthenticatedPrincipalRef, ExecutionStatus, ProjectId, RequestId, RunId, RunSummary,
    UiActionDisposition, UiActionResult, UiCursor, UiCursorV1, UiError, UiErrorCode,
    UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason, UiFeedGapV1,
    UiRetryDisposition, UiSnapshotV1, UI_ACTION_RESULT_SCHEMA, UI_ERROR_SCHEMA,
    UI_FEED_FRAME_SCHEMA, UI_FEED_GAP_SCHEMA, UI_SNAPSHOT_SCHEMA,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

fn scope(tab: &str) -> UiStoreScope {
    UiStoreScope::new("/workspace", "session-1", tab).unwrap()
}

fn frame(sequence: u64, event_id: &str, revision: u64) -> UiFeedFrameV1 {
    let cursor = UiFeedCursorV1::new(
        "instance-1",
        "epoch-1",
        sequence,
        UiCursor {
            epoch: "epoch-1".to_owned(),
            sequence: 1,
        },
    )
    .unwrap();
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: UiFeedFrameKind::Delta,
        cursor,
        event_id: event_id.to_owned(),
        replay: false,
        terminal: false,
        event: Some(json!({
            "entity_kind": "run",
            "entity_id": "run-1",
            "session_id": "session-1",
            "revision": revision,
            "status": "running"
        })),
        gap: None,
    }
}

fn optimistic(key: &str, expires_at_unix_ms: u64) -> UiOptimisticUpdate {
    UiOptimisticUpdate {
        idempotency_key: key.to_owned(),
        command_id: RequestId::new(),
        key: UiEntityKey::new(UiEntityKind::Submission, key).unwrap(),
        revision: 1,
        value: json!({"status": "sending"}),
        expires_at_unix_ms,
        previous: None,
    }
}

fn action_result(command_id: RequestId, disposition: UiActionDisposition) -> UiActionResult {
    let error = matches!(
        disposition,
        UiActionDisposition::Rejected | UiActionDisposition::Unknown
    )
    .then(|| UiError {
        schema: UI_ERROR_SCHEMA.to_owned(),
        code: if disposition == UiActionDisposition::Unknown {
            UiErrorCode::Unknown
        } else {
            UiErrorCode::Conflict
        },
        message: "fixture".to_owned(),
        retry: if disposition == UiActionDisposition::Unknown {
            UiRetryDisposition::QueryOriginal
        } else {
            UiRetryDisposition::DoNotRetry
        },
    });
    UiActionResult {
        schema: UI_ACTION_RESULT_SCHEMA.to_owned(),
        command_id,
        disposition,
        resulting_cursor: None,
        resulting_revision: Some(1),
        receipt: None,
        error,
        retry: if disposition == UiActionDisposition::Unknown {
            UiRetryDisposition::QueryOriginal
        } else {
            UiRetryDisposition::DoNotRetry
        },
    }
}

#[test]
fn reducer_rejects_old_revision_and_keeps_one_entity() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let store_scope = store.scope().clone();
    let mut boundary = frame(1, "boundary-1", 1);
    boundary.kind = UiFeedFrameKind::SnapshotBoundary;
    boundary.event = None;
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(store_scope, boundary))
            .unwrap(),
        UiStoreChange::SnapshotHydrated
    );
    let store_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(store_scope, frame(2, "event-1", 2)))
            .unwrap(),
        UiStoreChange::EntityInserted
    );
    let stale_scope = store.scope().clone();
    let stale = store.apply(UiStoreEvent::feed(stale_scope, frame(3, "event-2", 1)));
    assert!(matches!(stale, Err(UiStoreError::StaleRevision(_))));
    let key = UiEntityKey::new(UiEntityKind::Run, "run-1").unwrap();
    assert_eq!(store.entity(&key).unwrap().revision, 2);
    assert_eq!(store.len(), 1);
}

#[test]
fn reducer_deduplicates_events_and_requires_snapshot_after_gap() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let scope = store.scope().clone();
    let mut boundary = frame(1, "boundary-1", 1);
    boundary.kind = UiFeedFrameKind::SnapshotBoundary;
    boundary.event = None;
    store
        .apply(UiStoreEvent::feed(scope.clone(), boundary))
        .unwrap();
    store
        .apply(UiStoreEvent::feed(scope.clone(), frame(2, "event-1", 1)))
        .unwrap();
    assert!(matches!(
        store.apply(UiStoreEvent::feed(scope.clone(), frame(2, "event-1", 1))),
        Err(UiStoreError::DuplicateEvent(_))
    ));
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(scope.clone(), frame(4, "event-3", 3)))
            .unwrap(),
        UiStoreChange::SnapshotRequired
    );
    assert!(store.needs_snapshot());
    assert!(matches!(
        store.apply(UiStoreEvent::feed(scope, frame(5, "event-4", 4))),
        Err(UiStoreError::GapRequired)
    ));
}

#[test]
fn optimistic_state_rolls_back_and_protected_entries_block_eviction() {
    let mut store = UiEntityStore::new(scope("tab-1"), 1).unwrap();
    let update = optimistic("command-1", u64::MAX);
    let command_id = update.command_id;
    store.apply(UiStoreEvent::Optimistic(update)).unwrap();
    assert_eq!(store.pending_count(), 1);
    let result = store.apply(UiStoreEvent::Optimistic(optimistic("command-2", u64::MAX)));
    assert_eq!(result, Err(UiStoreError::CacheFullProtected));
    let result_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::action_result(
            result_scope,
            "command-1",
            action_result(command_id, UiActionDisposition::Rejected),
        ))
        .unwrap();
    assert!(store.is_empty());
}

#[test]
fn accepted_keeps_optimistic_record_until_settled_or_expired() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let update = optimistic("command-1", 10);
    let command_id = update.command_id;
    store.apply(UiStoreEvent::Optimistic(update)).unwrap();
    let result_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::action_result(
            result_scope,
            "command-1",
            action_result(command_id, UiActionDisposition::Accepted),
        ))
        .unwrap();
    assert_eq!(store.pending_count(), 1);
    let expiry_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::ExpireOptimistic {
            scope: expiry_scope,
            now_unix_ms: 10,
        })
        .unwrap();
    assert!(store.is_empty());
}

#[test]
fn unknown_and_pending_survive_hydrate_and_dehydrate() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let mut boundary = frame(1, "boundary-1", 1);
    boundary.kind = UiFeedFrameKind::SnapshotBoundary;
    boundary.event = None;
    let boundary_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(boundary_scope, boundary))
        .unwrap();
    let update = optimistic("command-1", u64::MAX);
    let command_id = update.command_id;
    store.apply(UiStoreEvent::Optimistic(update)).unwrap();
    let result_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::action_result(
            result_scope,
            "command-1",
            action_result(command_id, UiActionDisposition::Unknown),
        ))
        .unwrap();
    assert_eq!(store.unknown_count(), 1);
    let snapshot = store.dehydrate().unwrap();
    let restored = UiEntityStore::hydrate(snapshot).unwrap();
    assert_eq!(restored.unknown_count(), 1);
    assert_eq!(
        restored
            .entity(&UiEntityKey::new(UiEntityKind::Submission, "command-1").unwrap())
            .unwrap()
            .lifecycle,
        UiEntityLifecycle::Unknown
    );
}

#[test]
fn hydrate_rejects_optimistic_previous_from_a_foreign_scope() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let mut boundary = frame(1, "boundary-foreign-previous", 1);
    boundary.kind = UiFeedFrameKind::SnapshotBoundary;
    boundary.event = None;
    let boundary_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(boundary_scope, boundary))
        .unwrap();
    let update = optimistic("command-1", u64::MAX);
    store.apply(UiStoreEvent::Optimistic(update)).unwrap();
    let snapshot = store.dehydrate().unwrap();
    let mut encoded = serde_json::to_value(snapshot).unwrap();
    encoded["optimistic"][0]["previous"] = json!({
        "key": {"kind": "submission", "id": "command-1"},
        "workspace": "/foreign",
        "session_id": "foreign-session",
        "tab_id": "foreign-tab",
        "epoch": "foreign-epoch",
        "revision": 1,
        "lifecycle": "authoritative",
        "value": {},
        "source_event_id": null
    });
    let forged = serde_json::from_value(encoded).unwrap();
    assert_eq!(
        UiEntityStore::hydrate(forged).unwrap_err(),
        UiStoreError::ScopeMismatch
    );
}

#[test]
fn tab_scope_isolation_and_reducer_purity_are_explicit() {
    let store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let update = optimistic("command-1", u64::MAX);
    let transition = store.reduce(UiStoreEvent::Optimistic(update)).unwrap();
    assert!(store.is_empty());
    assert_eq!(transition.store.pending_count(), 1);
    let wrong_scope = UiStoreEvent::feed(scope("tab-2"), frame(1, "event-1", 1));
    assert_eq!(
        store.reduce(wrong_scope).unwrap_err(),
        UiStoreError::ScopeMismatch
    );
}

// ---------------------------------------------------------------------------------------------
// UI-08 completion slice: real feed positions, namespace fences and the declarative trace
// ---------------------------------------------------------------------------------------------

/// Build one frame for a chosen epoch/kind; the caller supplies the payload (or `None` for a
/// marker frame).  Every frame is validated before it reaches the reducer.
fn feed_frame(
    epoch: &str,
    sequence: u64,
    event_id: &str,
    kind: UiFeedFrameKind,
    event: Option<Value>,
) -> UiFeedFrameV1 {
    let cursor = UiFeedCursorV1::new(
        "instance-1",
        epoch,
        sequence,
        UiCursor {
            epoch: epoch.to_owned(),
            sequence: 1,
        },
    )
    .unwrap();
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        terminal: matches!(kind, UiFeedFrameKind::Terminal),
        kind,
        cursor,
        event_id: event_id.to_owned(),
        replay: false,
        event,
        gap: None,
    };
    frame.validate().expect("fixture feed frame is valid");
    frame
}

/// A boundary marker at the daemon-owned feed position.
fn boundary_frame(epoch: &str, sequence: u64, event_id: &str) -> UiFeedFrameV1 {
    feed_frame(
        epoch,
        sequence,
        event_id,
        UiFeedFrameKind::SnapshotBoundary,
        None,
    )
}

/// An entity-bearing delta: the payload names its namespace explicitly.
fn entity_frame(
    epoch: &str,
    sequence: u64,
    event_id: &str,
    entity_kind: &str,
    entity_id: &str,
    revision: u64,
) -> UiFeedFrameV1 {
    feed_frame(
        epoch,
        sequence,
        event_id,
        UiFeedFrameKind::Delta,
        Some(json!({
            "entity_kind": entity_kind,
            "entity_id": entity_id,
            "session_id": "session-1",
            "revision": revision,
        })),
    )
}

/// A terminal frame; `replay` marks the daemon's terminal retention replay.
fn terminal_frame(epoch: &str, sequence: u64, event_id: &str, replay: bool) -> UiFeedFrameV1 {
    let mut frame = feed_frame(
        epoch,
        sequence,
        event_id,
        UiFeedFrameKind::Terminal,
        Some(json!({
            "Terminal": {"run_id": "run-1"},
            "entity_kind": "run",
            "entity_id": "run-1",
            "session_id": "session-1",
            "revision": sequence,
        })),
    );
    frame.replay = replay;
    frame
}

/// A gap frame carrying the server's current position.
fn gap_frame(epoch: &str, sequence: u64, event_id: &str) -> UiFeedFrameV1 {
    let cursor = UiFeedCursorV1::new(
        "instance-1",
        epoch,
        sequence,
        UiCursor {
            epoch: epoch.to_owned(),
            sequence: 1,
        },
    )
    .unwrap();
    let gap = UiFeedGapV1 {
        schema: UI_FEED_GAP_SCHEMA.to_owned(),
        reason: UiFeedGapReason::ReplayExpired,
        from: None,
        to: cursor.clone(),
        snapshot_required: true,
    };
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: UiFeedFrameKind::Gap,
        cursor,
        event_id: event_id.to_owned(),
        replay: false,
        terminal: false,
        event: None,
        gap: Some(gap),
    };
    frame.validate().expect("fixture gap frame is valid");
    frame
}

/// A server snapshot projection of the same shape the authority owns.
fn server_snapshot(epoch: &str, cursor_sequence: u64, runs: Vec<RunSummary>) -> UiSnapshotV1 {
    UiSnapshotV1 {
        schema: UI_SNAPSHOT_SCHEMA.to_owned(),
        instance_id: "instance-1".to_owned(),
        authority_epoch: 1,
        snapshot_cursor: UiCursorV1 {
            epoch: epoch.to_owned(),
            sequence: cursor_sequence,
        },
        generated_at_unix_ms: 1,
        principal: AuthenticatedPrincipalRef::local(),
        workspace_id: ProjectId::new(),
        capabilities: Vec::new(),
        sessions: Vec::new(),
        active_session_id: None,
        runs,
        pending_actions: Vec::new(),
        artifacts: Vec::new(),
        notices: Vec::new(),
        next_page: None,
        limitations: Vec::new(),
    }
}

#[test]
fn live_feed_starts_at_the_initial_boundary_and_markers_never_regress() {
    // The daemon's live subscription starts with a boundary at feed_sequence 0; a presenter must
    // be able to adopt it instead of seeing its own starting position as a duplicate.
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let start_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                start_scope,
                boundary_frame("epoch-1", 0, "boundary-0"),
            ))
            .unwrap(),
        UiStoreChange::SnapshotHydrated
    );
    assert!(!store.needs_snapshot());
    assert_eq!(store.feed_sequence(), 0);
    assert_eq!(store.epoch(), "epoch-1");

    // A dense delta follows the boundary.
    let delta_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                delta_scope,
                entity_frame("epoch-1", 1, "event-1", "run", "run-1", 1),
            ))
            .unwrap(),
        UiStoreChange::EntityInserted
    );
    assert_eq!(store.feed_sequence(), 1);

    // A heartbeat at the same position is a marker: it never becomes a second item and never
    // errors, so a surface can keep the stream alive.
    let heartbeat_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                heartbeat_scope,
                feed_frame(
                    "epoch-1",
                    1,
                    "heartbeat-1",
                    UiFeedFrameKind::Heartbeat,
                    None
                ),
            ))
            .unwrap(),
        UiStoreChange::EntityUpdated
    );
    assert_eq!(store.feed_sequence(), 1);
    assert_eq!(store.len(), 1);

    // A stale position behind the projection is refused: a late frame can never be applied.
    let stale_scope = store.scope().clone();
    assert!(matches!(
        store.apply(UiStoreEvent::feed(
            stale_scope,
            entity_frame("epoch-1", 1, "late-frame", "run", "run-1", 2),
        )),
        Err(UiStoreError::DuplicateFrame(_))
    ));
    // An already applied identifier is refused as a duplicate event.
    let replay_scope = store.scope().clone();
    assert!(matches!(
        store.apply(UiStoreEvent::feed(
            replay_scope,
            entity_frame("epoch-1", 1, "event-1", "run", "run-1", 2),
        )),
        Err(UiStoreError::DuplicateEvent(_))
    ));
    assert_eq!(store.len(), 1);
    assert_eq!(
        store
            .entity(&UiEntityKey::new(UiEntityKind::Run, "run-1").unwrap())
            .unwrap()
            .revision,
        1
    );
}

#[test]
fn replayed_terminal_after_the_boundary_is_not_a_second_item() {
    // (a) Live: the terminal is applied once and a replay of the same identifier is refused.
    let mut live = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let live_scope = live.scope().clone();
    live.apply(UiStoreEvent::feed(
        live_scope,
        boundary_frame("epoch-1", 0, "boundary-0"),
    ))
    .unwrap();
    let delta_scope = live.scope().clone();
    live.apply(UiStoreEvent::feed(
        delta_scope,
        entity_frame("epoch-1", 1, "event-1", "run", "run-1", 1),
    ))
    .unwrap();
    let terminal_scope = live.scope().clone();
    assert_eq!(
        live.apply(UiStoreEvent::feed(
            terminal_scope,
            terminal_frame("epoch-1", 2, "terminal-2", false),
        ))
        .unwrap(),
        UiStoreChange::EntityUpdated
    );
    let run_key = UiEntityKey::new(UiEntityKind::Run, "run-1").unwrap();
    assert_eq!(live.entity(&run_key).unwrap().revision, 2);
    let replay_scope = live.scope().clone();
    assert!(matches!(
        live.apply(UiStoreEvent::feed(
            replay_scope,
            terminal_frame("epoch-1", 2, "terminal-2", true),
        )),
        Err(UiStoreError::DuplicateEvent(_))
    ));
    assert_eq!(live.len(), 1);

    // (b) Terminal retention: a reconnected subscription first reports the server position and
    // then replays the retained terminal at that position.  It is accepted exactly once.
    let mut retained = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let retained_scope = retained.scope().clone();
    assert_eq!(
        retained
            .apply(UiStoreEvent::feed(
                retained_scope,
                boundary_frame("epoch-1", 2, "boundary-2"),
            ))
            .unwrap(),
        UiStoreChange::SnapshotHydrated
    );
    assert_eq!(retained.feed_sequence(), 2);
    let retained_scope = retained.scope().clone();
    assert_eq!(
        retained
            .apply(UiStoreEvent::feed(
                retained_scope,
                terminal_frame("epoch-1", 2, "terminal-2", true),
            ))
            .unwrap(),
        UiStoreChange::EntityInserted
    );
    assert_eq!(retained.len(), 1);
    assert_eq!(retained.entity(&run_key).unwrap().revision, 2);
    let retained_scope = retained.scope().clone();
    assert!(matches!(
        retained.apply(UiStoreEvent::feed(
            retained_scope,
            terminal_frame("epoch-1", 2, "terminal-2", true),
        )),
        Err(UiStoreError::DuplicateEvent(_))
    ));
    assert_eq!(retained.len(), 1);
}

#[test]
fn epoch_reset_drops_authoritative_state_and_keeps_local_pending() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let start_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            start_scope,
            boundary_frame("epoch-1", 0, "boundary-0"),
        ))
        .unwrap();
    let delta_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            delta_scope,
            entity_frame("epoch-1", 1, "event-1", "run", "run-1", 1),
        ))
        .unwrap();
    let update = optimistic("command-1", u64::MAX);
    store.apply(UiStoreEvent::Optimistic(update)).unwrap();
    assert_eq!(store.pending_count(), 1);
    assert_eq!(store.len(), 2);

    // The authority instance changed: a boundary from the new epoch replaces the authoritative
    // view instead of merging two epochs into one.
    let adopt_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                adopt_scope,
                boundary_frame("epoch-2", 5, "boundary-new-epoch"),
            ))
            .unwrap(),
        UiStoreChange::SnapshotRequired
    );
    assert_eq!(store.epoch(), "epoch-2");
    assert_eq!(store.feed_sequence(), 5);
    assert!(store.needs_snapshot());
    assert_eq!(
        store.gap().map(|gap| &gap.reason),
        Some(&UiStoreGapReason::EpochReset)
    );
    assert!(store
        .entity(&UiEntityKey::new(UiEntityKind::Run, "run-1").unwrap())
        .is_none());
    assert_eq!(store.pending_count(), 1);
    assert_eq!(store.len(), 1);

    // Later deltas stay fenced until an explicit snapshot hydrate.
    let fenced_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                fenced_scope,
                entity_frame("epoch-2", 6, "event-6", "run", "run-2", 1),
            ))
            .unwrap_err(),
        UiStoreError::GapRequired
    );
    // A frame from the retired epoch is refused instead of being projected into the new one.
    let retired_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                retired_scope,
                entity_frame("epoch-1", 6, "event-6-retired", "run", "run-2", 1),
            ))
            .unwrap_err(),
        UiStoreError::EpochMismatch
    );

    // The explicit hydrate clears the requirement while the local pending value survives.
    let hydrate_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::snapshot(
                hydrate_scope,
                server_snapshot("epoch-2", 1, Vec::new()),
            ))
            .unwrap(),
        UiStoreChange::SnapshotHydrated
    );
    assert!(!store.needs_snapshot());
    assert!(store.gap().is_none());
    assert_eq!(store.pending_count(), 1);
    assert_eq!(store.epoch(), "epoch-2");
}

#[test]
fn generic_feed_requires_an_explicit_kind_and_refuses_the_draft_namespace() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let start_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            start_scope,
            boundary_frame("epoch-1", 0, "boundary-0"),
        ))
        .unwrap();

    // A payload without an entity namespace is refused instead of being guessed into a run.
    let no_kind_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                no_kind_scope,
                feed_frame(
                    "epoch-1",
                    1,
                    "event-no-kind",
                    UiFeedFrameKind::Delta,
                    Some(json!({"entity_id": "run-1", "session_id": "session-1"})),
                ),
            ))
            .unwrap_err(),
        UiStoreError::Invalid("ui_feed_entity_kind_missing".to_owned())
    );
    // The draft namespace belongs to the local input layer, never to server input.
    let draft_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                draft_scope,
                entity_frame("epoch-1", 1, "event-draft", "draft", "draft-1", 1),
            ))
            .unwrap_err(),
        UiStoreError::Invalid("ui_feed_draft_kind_forbidden".to_owned())
    );
    // An unknown namespace, a missing identity and a zero revision all fail closed.
    let unknown_kind_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                unknown_kind_scope,
                entity_frame("epoch-1", 1, "event-widget", "widget", "widget-1", 1),
            ))
            .unwrap_err(),
        UiStoreError::Invalid("ui_entity_kind_unknown:widget".to_owned())
    );
    let no_id_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                no_id_scope,
                feed_frame(
                    "epoch-1",
                    1,
                    "event-no-id",
                    UiFeedFrameKind::Delta,
                    Some(json!({"entity_kind": "run", "session_id": "session-1"})),
                ),
            ))
            .unwrap_err(),
        UiStoreError::Invalid("ui_feed_entity_id_missing".to_owned())
    );
    let zero_revision_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                zero_revision_scope,
                entity_frame("epoch-1", 1, "event-zero", "run", "run-1", 0),
            ))
            .unwrap_err(),
        UiStoreError::Invalid("ui_feed_entity_revision_invalid".to_owned())
    );
    // An entity for another session is never merged into this tab.
    let foreign_session_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                foreign_session_scope,
                feed_frame(
                    "epoch-1",
                    1,
                    "event-foreign",
                    UiFeedFrameKind::Delta,
                    Some(json!({
                        "entity_kind": "run",
                        "entity_id": "run-1",
                        "session_id": "session-2",
                        "revision": 1,
                    })),
                ),
            ))
            .unwrap_err(),
        UiStoreError::ScopeMismatch
    );

    // Every refusal left the projection untouched: no item, no position, still fenced.
    assert!(store.is_empty());
    assert_eq!(store.feed_sequence(), 0);
}

#[test]
fn pending_conflict_never_lets_an_optimistic_value_hide_a_newer_revision() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let start_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            start_scope,
            boundary_frame("epoch-1", 0, "boundary-0"),
        ))
        .unwrap();
    let update = optimistic("command-1", u64::MAX);
    let command_id = update.command_id;
    store.apply(UiStoreEvent::Optimistic(update)).unwrap();
    let command_key = UiEntityKey::new(UiEntityKind::Submission, "command-1").unwrap();
    assert_eq!(store.pending_count(), 1);

    // A newer authoritative revision for the same key arrives while the local value is pending.
    let conflict_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                conflict_scope,
                entity_frame("epoch-1", 1, "event-1", "submission", "command-1", 9),
            ))
            .unwrap(),
        UiStoreChange::SnapshotRequired
    );
    assert!(store.needs_snapshot());
    assert_eq!(
        store.gap().map(|gap| &gap.reason),
        Some(&UiStoreGapReason::PendingConflict)
    );
    assert_eq!(store.gap().map(|gap| gap.observed_sequence), Some(1));
    // The local value is kept, the conflicting frame is not recorded as applied, and later
    // frames stay fenced until an explicit hydrate.
    assert_eq!(
        store.entity(&command_key).unwrap().lifecycle,
        UiEntityLifecycle::OptimisticPending
    );
    assert_eq!(store.feed_sequence(), 0);
    let fenced_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(
                fenced_scope,
                entity_frame("epoch-1", 1, "event-1", "submission", "command-1", 9),
            ))
            .unwrap_err(),
        UiStoreError::GapRequired
    );

    // After the hydrate the pending command is still displayed and is settled by the authority's
    // own result, never by the optimistic value being promoted.
    let hydrate_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::snapshot(
            hydrate_scope,
            server_snapshot("epoch-1", 1, Vec::new()),
        ))
        .unwrap();
    assert!(store.entity(&command_key).is_some());
    assert_eq!(store.pending_count(), 1);
    let reject_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::action_result(
                reject_scope,
                "command-1",
                action_result(command_id, UiActionDisposition::Rejected),
            ))
            .unwrap(),
        UiStoreChange::ActionRejected
    );
    assert!(store.entity(&command_key).is_none());
    assert_eq!(store.pending_count(), 0);
}

#[test]
fn snapshot_merge_refreshes_the_rollback_baseline_instead_of_a_stale_value() {
    let run_id = RunId::new();
    let run_id_text = run_id.to_string();
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let run_key = UiEntityKey::new(UiEntityKind::Run, run_id_text.clone()).unwrap();
    let start_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            start_scope,
            boundary_frame("epoch-1", 0, "boundary-0"),
        ))
        .unwrap();
    let delta_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            delta_scope,
            entity_frame("epoch-1", 1, "event-1", "run", &run_id_text, 1),
        ))
        .unwrap();

    // A local optimistic value for the same run, with the revision-1 entity as its baseline.
    let command_id = RequestId::new();
    let update = UiOptimisticUpdate {
        idempotency_key: "command-1".to_owned(),
        command_id,
        key: run_key.clone(),
        revision: 2,
        value: json!({"status": "optimistic"}),
        expires_at_unix_ms: u64::MAX,
        previous: None,
    };
    store.apply(UiStoreEvent::Optimistic(update)).unwrap();
    assert_eq!(
        store.entity(&run_key).unwrap().lifecycle,
        UiEntityLifecycle::OptimisticPending
    );

    // A server snapshot at a newer revision arrives for the same key: the pending value stays,
    // but the rollback baseline is refreshed to the server's latest revision.
    let snapshot_scope = store.scope().clone();
    let snapshot = server_snapshot(
        "epoch-1",
        2,
        vec![RunSummary {
            run_id,
            status: ExecutionStatus::Completed,
            revision: 5,
            final_assistant_text: None,
            receipt: None,
        }],
    );
    assert_eq!(
        store
            .apply(UiStoreEvent::snapshot(snapshot_scope, snapshot))
            .unwrap(),
        UiStoreChange::SnapshotHydrated
    );
    assert_eq!(
        store.entity(&run_key).unwrap().lifecycle,
        UiEntityLifecycle::OptimisticPending
    );

    // The authority rejected the command: the rollback restores the server's latest value, never
    // the stale pre-optimistic revision.
    let reject_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::action_result(
                reject_scope,
                "command-1",
                action_result(command_id, UiActionDisposition::Rejected),
            ))
            .unwrap(),
        UiStoreChange::ActionRejected
    );
    let rolled_back = store
        .entity(&run_key)
        .expect("rollback restores the baseline");
    assert_eq!(rolled_back.lifecycle, UiEntityLifecycle::Authoritative);
    assert_eq!(rolled_back.revision, 5);
    assert_eq!(
        rolled_back.value.get("revision").and_then(Value::as_u64),
        Some(5)
    );
    assert_eq!(store.pending_count(), 0);
}

#[test]
fn hydrate_refuses_a_forged_draft_entity() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let start_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            start_scope,
            boundary_frame("epoch-1", 0, "boundary-0"),
        ))
        .unwrap();
    let delta_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            delta_scope,
            entity_frame("epoch-1", 1, "event-1", "run", "run-1", 1),
        ))
        .unwrap();
    let snapshot = store.dehydrate().unwrap();
    assert_eq!(snapshot.schema, UI_ENTITY_STORE_SNAPSHOT_SCHEMA);

    // A stored draft entity is forged/tampered input: the store keeps drafts in the input layer,
    // so the snapshot is refused before its digest is even considered.
    let mut encoded = serde_json::to_value(snapshot).unwrap();
    encoded["entities"][0]["key"]["kind"] = json!("draft");
    let forged: UiEntityStoreSnapshot = serde_json::from_value(encoded).unwrap();
    assert_eq!(
        UiEntityStore::hydrate(forged).unwrap_err(),
        UiStoreError::Invalid("ui_store_snapshot_draft_entity".to_owned())
    );
}

#[test]
fn a_stale_rejection_never_deletes_an_authoritative_entity() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let start_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            start_scope,
            boundary_frame("epoch-1", 0, "boundary-0"),
        ))
        .unwrap();
    let delta_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            delta_scope,
            entity_frame("epoch-1", 1, "event-1", "submission", "command-9", 1),
        ))
        .unwrap();
    let authoritative_key = UiEntityKey::new(UiEntityKind::Submission, "command-9").unwrap();

    // A stale rejection for the same idempotency key must not delete an authoritative entity the
    // store never staged as pending.
    let stale_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::action_result(
                stale_scope,
                "command-9",
                action_result(RequestId::new(), UiActionDisposition::Rejected),
            ))
            .unwrap(),
        UiStoreChange::ActionRejected
    );
    assert!(store.entity(&authoritative_key).is_some());

    // An accepted command without a staged record still rolls back its own pending placeholder.
    let accepted_scope = store.scope().clone();
    let command_id = RequestId::new();
    store
        .apply(UiStoreEvent::action_result(
            accepted_scope,
            "command-10",
            action_result(command_id, UiActionDisposition::Accepted),
        ))
        .unwrap();
    let placeholder_key = UiEntityKey::new(UiEntityKind::Submission, "command-10").unwrap();
    assert_eq!(
        store.entity(&placeholder_key).unwrap().lifecycle,
        UiEntityLifecycle::OptimisticPending
    );
    let rejected_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::action_result(
            rejected_scope,
            "command-10",
            action_result(command_id, UiActionDisposition::Rejected),
        ))
        .unwrap();
    assert!(store.entity(&placeholder_key).is_none());
    assert!(store.entity(&authoritative_key).is_some());
}

#[test]
fn kind_scoped_views_keep_the_six_namespaces_separate() {
    let mut store = UiEntityStore::new(scope("tab-1"), 8).unwrap();
    let start_scope = store.scope().clone();
    store
        .apply(UiStoreEvent::feed(
            start_scope,
            boundary_frame("epoch-1", 0, "boundary-0"),
        ))
        .unwrap();
    for (sequence, (kind, id)) in [
        ("run", "run-1"),
        ("connection", "session-1"),
        ("inbox", "action-1"),
        ("artifact", "artifact-1"),
    ]
    .into_iter()
    .enumerate()
    {
        let position = sequence as u64 + 1;
        let step_scope = store.scope().clone();
        assert_eq!(
            store
                .apply(UiStoreEvent::feed(
                    step_scope,
                    entity_frame(
                        "epoch-1",
                        position,
                        &format!("event-{position}"),
                        kind,
                        id,
                        position,
                    ),
                ))
                .unwrap(),
            UiStoreChange::EntityInserted
        );
    }
    let update = optimistic("command-1", u64::MAX);
    store.apply(UiStoreEvent::Optimistic(update)).unwrap();

    assert_eq!(store.count_of_kind(UiEntityKind::Run), 1);
    assert_eq!(store.count_of_kind(UiEntityKind::Connection), 1);
    assert_eq!(store.count_of_kind(UiEntityKind::Submission), 1);
    assert_eq!(store.count_of_kind(UiEntityKind::Inbox), 1);
    assert_eq!(store.count_of_kind(UiEntityKind::Artifact), 1);
    // Drafts are read from the input layer: the store never gains one from server input.
    assert_eq!(store.count_of_kind(UiEntityKind::Draft), 0);
    assert!(store.entities_of_kind(UiEntityKind::Draft).next().is_none());
    let names: Vec<&str> = store
        .entities_of_kind(UiEntityKind::Run)
        .map(|entity| entity.key.id.as_str())
        .collect();
    assert_eq!(names, vec!["run-1"]);
    assert_eq!(store.len(), 5);
}

// ---------------------------------------------------------------------------------------------
// Declarative plan and reducer trace
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorePlan {
    schema: String,
    step: String,
    entity_kinds: Vec<String>,
    lifecycles: Vec<String>,
    gap_reasons: Vec<String>,
    budgets: PlanBudgets,
    fixtures: Vec<String>,
    required: Vec<String>,
    denied: Vec<String>,
    limitations: Vec<String>,
    trace: TracePlan,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanBudgets {
    max_entities: usize,
    max_events: usize,
    max_unknown: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TracePlan {
    scope: TraceScope,
    epoch: String,
    max_entities: usize,
    steps: Vec<TraceStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceScope {
    workspace: String,
    session_id: String,
    tab_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceStep {
    op: String,
    #[serde(default)]
    feed_sequence: u64,
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    entity_kind: Option<String>,
    #[serde(default)]
    entity_id: Option<String>,
    #[serde(default)]
    revision: Option<u64>,
    #[serde(default)]
    idempotency_key: String,
    #[serde(default)]
    disposition: Option<String>,
    #[serde(default)]
    expires_at_unix_ms: u64,
    #[serde(default)]
    now_unix_ms: u64,
    expect: Expectation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expectation {
    #[serde(default)]
    change: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    needs_snapshot: Option<bool>,
    #[serde(default)]
    feed_sequence: Option<u64>,
    #[serde(default)]
    entity_count: Option<usize>,
    #[serde(default)]
    pending_count: Option<usize>,
    #[serde(default)]
    unknown_count: Option<usize>,
    #[serde(default)]
    epoch: Option<String>,
    #[serde(default)]
    gap_reason: Option<String>,
    #[serde(default)]
    entities: Vec<EntityProbe>,
    #[serde(default)]
    absent: Vec<EntityProbe>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityProbe {
    kind: String,
    id: String,
    #[serde(default)]
    revision: Option<u64>,
    #[serde(default)]
    lifecycle: Option<String>,
}

fn store_plan() -> StorePlan {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui08-reducer-store.json");
    let raw = std::fs::read_to_string(&path).expect("ui08 plan fixture is readable");
    serde_json::from_str(&raw).expect("ui08 plan fixture matches its declared schema")
}

fn traced_kind(name: &str) -> UiEntityKind {
    match name {
        "run" => UiEntityKind::Run,
        "connection" => UiEntityKind::Connection,
        "submission" => UiEntityKind::Submission,
        "draft" => UiEntityKind::Draft,
        "inbox" => UiEntityKind::Inbox,
        "artifact" => UiEntityKind::Artifact,
        other => panic!("traced entity kind is not part of the contract: {other}"),
    }
}

fn change_name(change: UiStoreChange) -> &'static str {
    match change {
        UiStoreChange::SnapshotHydrated => "snapshot_hydrated",
        UiStoreChange::EntityInserted => "entity_inserted",
        UiStoreChange::EntityUpdated => "entity_updated",
        UiStoreChange::DuplicateEvent => "duplicate_event",
        UiStoreChange::DuplicateFrame => "duplicate_frame",
        UiStoreChange::OptimisticPending => "optimistic_pending",
        UiStoreChange::OptimisticRolledBack => "optimistic_rolled_back",
        UiStoreChange::ActionAccepted => "action_accepted",
        UiStoreChange::ActionApplied => "action_applied",
        UiStoreChange::ActionRejected => "action_rejected",
        UiStoreChange::ActionUnknown => "action_unknown",
        UiStoreChange::ExpiredOptimistic => "expired_optimistic",
        UiStoreChange::SnapshotRequired => "snapshot_required",
    }
}

fn lifecycle_name(lifecycle: UiEntityLifecycle) -> &'static str {
    match lifecycle {
        UiEntityLifecycle::Authoritative => "authoritative",
        UiEntityLifecycle::OptimisticPending => "optimistic_pending",
        UiEntityLifecycle::Unknown => "unknown",
    }
}

fn kind_name(kind: UiEntityKind) -> &'static str {
    match kind {
        UiEntityKind::Run => "run",
        UiEntityKind::Connection => "connection",
        UiEntityKind::Submission => "submission",
        UiEntityKind::Draft => "draft",
        UiEntityKind::Inbox => "inbox",
        UiEntityKind::Artifact => "artifact",
    }
}

fn gap_reason_name(reason: &UiStoreGapReason) -> &'static str {
    match reason {
        UiStoreGapReason::SequenceGap => "sequence_gap",
        UiStoreGapReason::FeedGap => "feed_gap",
        UiStoreGapReason::EpochReset => "epoch_reset",
        UiStoreGapReason::SnapshotRequired => "snapshot_required",
        UiStoreGapReason::PendingConflict => "pending_conflict",
    }
}

/// Apply one traced event, asserting that the pure reducer preview and the committed transition
/// agree and that the preview never mutated the store.
fn apply_traced_event(
    store: &mut UiEntityStore,
    event: UiStoreEvent,
) -> Result<UiStoreChange, UiStoreError> {
    let entities_before = store.len();
    let position_before = store.feed_sequence();
    let pending_before = store.pending_count();
    let preview = store.reduce(event.clone());
    assert_eq!(
        store.len(),
        entities_before,
        "reduce must not mutate entities"
    );
    assert_eq!(
        store.feed_sequence(),
        position_before,
        "reduce must not move the position"
    );
    assert_eq!(
        store.pending_count(),
        pending_before,
        "reduce must not mutate pending state"
    );
    match store.apply(event) {
        Ok(change) => {
            match preview {
                Ok(transition) => assert_eq!(transition.change, change),
                Err(error) => panic!("reduce rejected what apply accepted: {error}"),
            }
            Ok(change)
        }
        Err(error) => {
            match preview {
                Err(preview_error) => assert_eq!(preview_error, error),
                Ok(_) => panic!("reduce accepted what apply rejected: {error}"),
            }
            Err(error)
        }
    }
}

fn trace_event(trace: &TracePlan, step: &TraceStep, index: usize) -> Option<UiStoreEvent> {
    let scope = UiStoreScope::new(
        trace.scope.workspace.clone(),
        trace.scope.session_id.clone(),
        trace.scope.tab_id.clone(),
    )
    .expect("traced scope");
    match step.op.as_str() {
        "snapshot_boundary" => Some(UiStoreEvent::feed(
            scope,
            boundary_frame(&trace.epoch, step.feed_sequence, &step.event_id),
        )),
        "heartbeat" => Some(UiStoreEvent::feed(
            scope,
            feed_frame(
                &trace.epoch,
                step.feed_sequence,
                &step.event_id,
                UiFeedFrameKind::Heartbeat,
                None,
            ),
        )),
        "feed_gap" => Some(UiStoreEvent::feed(
            scope,
            gap_frame(&trace.epoch, step.feed_sequence, &step.event_id),
        )),
        "feed_delta" => {
            let mut payload = serde_json::Map::new();
            payload.insert(
                "session_id".to_owned(),
                json!(trace.scope.session_id.clone()),
            );
            if let Some(entity_kind) = &step.entity_kind {
                payload.insert("entity_kind".to_owned(), json!(entity_kind.clone()));
            }
            if let Some(entity_id) = &step.entity_id {
                payload.insert("entity_id".to_owned(), json!(entity_id.clone()));
            }
            if let Some(revision) = step.revision {
                payload.insert("revision".to_owned(), json!(revision));
            }
            Some(UiStoreEvent::feed(
                scope,
                feed_frame(
                    &trace.epoch,
                    step.feed_sequence,
                    &step.event_id,
                    UiFeedFrameKind::Delta,
                    Some(Value::Object(payload)),
                ),
            ))
        }
        "snapshot" => Some(UiStoreEvent::snapshot(
            scope,
            server_snapshot(&trace.epoch, 1, Vec::new()),
        )),
        "optimistic" => Some(UiStoreEvent::Optimistic(UiOptimisticUpdate {
            idempotency_key: step.idempotency_key.clone(),
            command_id: RequestId::new(),
            key: UiEntityKey::new(UiEntityKind::Submission, step.idempotency_key.clone())
                .expect("traced optimistic key"),
            revision: 1,
            value: json!({"status": "sending"}),
            expires_at_unix_ms: step.expires_at_unix_ms,
            previous: None,
        })),
        "action_result" => {
            let disposition = match step.disposition.as_deref() {
                Some("accepted") => UiActionDisposition::Accepted,
                Some("applied") => UiActionDisposition::Applied,
                Some("rejected") => UiActionDisposition::Rejected,
                Some("unknown") => UiActionDisposition::Unknown,
                other => panic!("trace step {index} has an unhandled disposition: {other:?}"),
            };
            Some(UiStoreEvent::action_result(
                scope,
                step.idempotency_key.clone(),
                action_result(RequestId::new(), disposition),
            ))
        }
        "expire" => Some(UiStoreEvent::ExpireOptimistic {
            scope,
            now_unix_ms: step.now_unix_ms,
        }),
        "observe" => None,
        other => panic!("trace step {index} has an unhandled op: {other}"),
    }
}

fn check_trace_expectations(
    index: usize,
    step: &TraceStep,
    store: &UiEntityStore,
    outcome: &Result<UiStoreChange, UiStoreError>,
) {
    let expect = &step.expect;
    if let Some(error) = &expect.error {
        match outcome {
            Err(actual) => assert!(
                format!("{actual}").starts_with(error.as_str()),
                "trace step {index}: expected error {error}, got {actual}"
            ),
            Ok(change) => panic!("trace step {index}: expected error {error}, got {change:?}"),
        }
    } else if let Some(change) = &expect.change {
        match outcome {
            Ok(actual) => assert_eq!(
                change_name(*actual),
                change.as_str(),
                "trace step {index}: change"
            ),
            Err(error) => panic!("trace step {index}: expected change {change}, got {error}"),
        }
    } else {
        assert!(
            outcome.is_ok(),
            "trace step {index}: observation step failed: {:?}",
            outcome
        );
    }
    if let Some(needs_snapshot) = expect.needs_snapshot {
        assert_eq!(
            store.needs_snapshot(),
            needs_snapshot,
            "trace step {index}: needs_snapshot"
        );
    }
    if let Some(feed_sequence) = expect.feed_sequence {
        assert_eq!(
            store.feed_sequence(),
            feed_sequence,
            "trace step {index}: feed_sequence"
        );
    }
    if let Some(entity_count) = expect.entity_count {
        assert_eq!(
            store.len(),
            entity_count,
            "trace step {index}: entity_count"
        );
    }
    if let Some(pending_count) = expect.pending_count {
        assert_eq!(
            store.pending_count(),
            pending_count,
            "trace step {index}: pending_count"
        );
    }
    if let Some(unknown_count) = expect.unknown_count {
        assert_eq!(
            store.unknown_count(),
            unknown_count,
            "trace step {index}: unknown_count"
        );
    }
    if let Some(epoch) = &expect.epoch {
        assert_eq!(store.epoch(), epoch.as_str(), "trace step {index}: epoch");
    }
    if let Some(reason) = &expect.gap_reason {
        let gap = store
            .gap()
            .unwrap_or_else(|| panic!("trace step {index}: expected a recorded gap"));
        assert_eq!(
            gap_reason_name(&gap.reason),
            reason.as_str(),
            "trace step {index}: gap_reason"
        );
    }
    for probe in &expect.entities {
        let key = UiEntityKey::new(traced_kind(&probe.kind), probe.id.clone())
            .expect("traced entity probe key");
        let entity = store.entity(&key).unwrap_or_else(|| {
            panic!(
                "trace step {index}: entity is missing: {} {}",
                probe.kind, probe.id
            )
        });
        if let Some(revision) = probe.revision {
            assert_eq!(
                entity.revision, revision,
                "trace step {index}: {} revision",
                probe.id
            );
        }
        if let Some(lifecycle) = &probe.lifecycle {
            assert_eq!(
                lifecycle_name(entity.lifecycle),
                lifecycle.as_str(),
                "trace step {index}: {} lifecycle",
                probe.id
            );
        }
    }
    for probe in &expect.absent {
        let key = UiEntityKey::new(traced_kind(&probe.kind), probe.id.clone())
            .expect("traced absent probe key");
        assert!(
            store.entity(&key).is_none(),
            "trace step {index}: entity must be absent: {} {}",
            probe.kind,
            probe.id
        );
    }
}

fn run_store_trace(trace: &TracePlan) -> UiEntityStore {
    let scope = UiStoreScope::new(
        trace.scope.workspace.clone(),
        trace.scope.session_id.clone(),
        trace.scope.tab_id.clone(),
    )
    .expect("traced scope");
    let mut store = UiEntityStore::new(scope, trace.max_entities).expect("traced store");
    for (index, step) in trace.steps.iter().enumerate() {
        let outcome = match trace_event(trace, step, index) {
            Some(event) => apply_traced_event(&mut store, event),
            None => Ok(UiStoreChange::EntityUpdated),
        };
        check_trace_expectations(index, step, &store, &outcome);
    }
    store
}

#[test]
fn ui08_reducer_plan_and_trace_replay_deterministically() {
    let plan = store_plan();
    assert_eq!(plan.schema, "kiana.ui08-reducer-store-fixture.v1");
    assert_eq!(plan.step, "UI-08");
    assert_eq!(
        plan.entity_kinds,
        vec![
            "run",
            "connection",
            "submission",
            "draft",
            "inbox",
            "artifact"
        ]
    );
    for kind in &plan.entity_kinds {
        let traced = traced_kind(kind);
        assert_eq!(kind_name(traced.clone()), kind.as_str());
        assert_eq!(serde_json::to_value(traced).unwrap(), json!(kind));
    }

    assert_eq!(
        plan.lifecycles,
        vec!["authoritative", "optimistic_pending", "unknown"]
    );
    for (lifecycle, name) in [
        (UiEntityLifecycle::Authoritative, "authoritative"),
        (UiEntityLifecycle::OptimisticPending, "optimistic_pending"),
        (UiEntityLifecycle::Unknown, "unknown"),
    ] {
        assert_eq!(serde_json::to_value(lifecycle).unwrap(), json!(name));
        assert_eq!(lifecycle_name(lifecycle), name);
    }

    let mut reasons = plan.gap_reasons.clone();
    reasons.sort();
    reasons.dedup();
    assert_eq!(
        reasons.len(),
        plan.gap_reasons.len(),
        "gap reasons must stay unique"
    );
    for (reason, name) in [
        (UiStoreGapReason::SequenceGap, "sequence_gap"),
        (UiStoreGapReason::FeedGap, "feed_gap"),
        (UiStoreGapReason::EpochReset, "epoch_reset"),
        (UiStoreGapReason::SnapshotRequired, "snapshot_required"),
        (UiStoreGapReason::PendingConflict, "pending_conflict"),
    ] {
        assert_eq!(serde_json::to_value(&reason).unwrap(), json!(name));
        assert!(
            plan.gap_reasons.iter().any(|value| value == name),
            "plan must keep the {name} denial"
        );
        assert_eq!(gap_reason_name(&reason), name);
    }

    assert_eq!(plan.budgets.max_entities, UI_ENTITY_STORE_MAX_ENTITIES);
    assert_eq!(plan.budgets.max_events, UI_ENTITY_STORE_MAX_EVENTS);
    assert_eq!(plan.budgets.max_unknown, UI_ENTITY_STORE_MAX_UNKNOWN);
    assert!(plan.denied.iter().all(|line| !line.trim().is_empty()));
    assert!(plan
        .limitations
        .iter()
        .any(|line| line.contains("proof_level 上限为 source")));

    let source = include_str!("ui08_reducer_store.rs");
    for fixture in &plan.fixtures {
        assert!(
            source.contains(&format!("fn {fixture}(")),
            "plan fixture is not defined in ui08_reducer_store.rs: {fixture}"
        );
    }
    let store_source = include_str!("../src/ui_store.rs");
    for marker in &plan.required {
        assert!(
            store_source.contains(marker.as_str()),
            "reducer fence marker missing from ui_store.rs: {marker}"
        );
    }
    assert!(!store_source.contains("CapabilityBroker"));
    assert!(!store_source.contains("ModelClient"));
    assert!(!store_source.contains("std::fs::"));

    // The declared trace is the source of truth for the state graph: it replays to the same
    // digest twice and never loses a protected entry.
    let first = run_store_trace(&plan.trace);
    let second = run_store_trace(&plan.trace);
    assert_eq!(
        first.dehydrate().unwrap().snapshot_digest,
        second.dehydrate().unwrap().snapshot_digest,
        "the trace must replay deterministically"
    );
    assert_eq!(first.len(), 3);
    assert_eq!(first.count_of_kind(UiEntityKind::Run), 3);
    assert_eq!(first.pending_count(), 0);
    assert_eq!(first.epoch(), "epoch-1");
    assert_eq!(first.feed_sequence(), 0);
    assert_eq!(
        first.scope(),
        &UiStoreScope::new("/workspace", "session-1", "tab-1").unwrap()
    );
}
