use kiana_client::{
    UiEntityKey, UiEntityKind, UiEntityLifecycle, UiEntityStore, UiOptimisticUpdate, UiStoreChange,
    UiStoreError, UiStoreEvent, UiStoreScope,
};
use kiana_protocol::{
    RequestId, UiActionDisposition, UiActionResult, UiCursor, UiError, UiErrorCode, UiFeedCursorV1,
    UiFeedFrameKind, UiFeedFrameV1, UiRetryDisposition, UI_ACTION_RESULT_SCHEMA, UI_ERROR_SCHEMA,
    UI_FEED_FRAME_SCHEMA,
};
use serde_json::json;

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
