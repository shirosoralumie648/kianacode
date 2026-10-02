use kiana_domain::{AggregateVersion, EventId, RequestId, RuntimeEvent, TransitionBatch};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;

#[tokio::test]
async fn event_id_reuse_is_denied() {
    let store = MemoryEventLog::new();
    let request_id = RequestId::new();
    let first = RuntimeEvent::new(request_id, 1, "run.accepted", json!({"run_id":"run-1"}))
        .unwrap()
        .with_stream_metadata("run", "run-1", 1);
    let mut duplicate = first.clone();
    duplicate.sequence = 2;
    assert!(store.append(first).await.is_ok());
    assert!(matches!(
        store.append(duplicate).await,
        Err(PortError::Conflict(reason)) if reason == "event_id_duplicate"
    ));
}

#[tokio::test]
async fn same_request_different_command_digest_conflicts() {
    let store = MemoryEventLog::new();
    let command_id = RequestId::new();
    let first = transition(command_id, 'a');
    store.commit_transition(first).await.unwrap();
    let error = store
        .commit_transition(transition(command_id, 'b'))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        PortError::Conflict(reason) if reason == "event_store_command_digest_mismatch"
    ));
}

#[tokio::test]
async fn malformed_identity_links_are_denied_before_append() {
    let store = MemoryEventLog::new();
    let request_id = RequestId::new();
    let mut malformed = RuntimeEvent::new(request_id, 1, "run.accepted", json!({})).unwrap();
    malformed.causation_event_id = Some(malformed.event_id);
    assert!(matches!(
        store.append(malformed).await,
        Err(PortError::Failed(reason)) if reason == "event_identity_links_invalid:event_causation_self"
    ));

    let mut command_without_correlation =
        RuntimeEvent::new(request_id, 1, "run.accepted", json!({})).unwrap();
    command_without_correlation.command_id = Some(RequestId::new());
    command_without_correlation.correlation_id = None;
    assert!(matches!(
        store.append(command_without_correlation).await,
        Err(PortError::Failed(reason))
            if reason == "event_identity_links_invalid:event_command_requires_correlation"
    ));
}

#[tokio::test]
async fn idempotent_replay_rejects_identity_link_drift() {
    let store = MemoryEventLog::new();
    let request_id = RequestId::new();
    let first = RuntimeEvent::new(request_id, 1, "run.accepted", json!({}))
        .unwrap()
        .with_identity_links(Some(request_id), Some(request_id), None, None)
        .with_idempotency_key("er02-identity-key");
    store.append_idempotent(first).await.unwrap();

    let drifted = RuntimeEvent::new(request_id, 1, "run.accepted", json!({}))
        .unwrap()
        .with_identity_links(
            Some(request_id),
            Some(request_id),
            None,
            Some(EventId::new()),
        )
        .with_idempotency_key("er02-identity-key");
    assert!(matches!(
        store.append_idempotent(drifted).await,
        Err(PortError::Conflict(reason)) if reason == "event_idempotency_key_payload_mismatch"
    ));
}

fn transition(command_id: RequestId, digest: char) -> TransitionBatch {
    let event = RuntimeEvent::new(
        command_id,
        1,
        "run.accepted",
        json!({"run_id":"run-identity"}),
    )
    .unwrap()
    .with_stream_metadata("run", "run-identity", 1);
    TransitionBatch {
        command_id,
        command_digest: std::iter::repeat(digest).take(64).collect(),
        expected_versions: vec![AggregateVersion::new("run", "run-identity", 0)],
        events: vec![event],
    }
}

#[allow(dead_code)]
fn _event_id_is_a_stable_uuid() {
    let _ = EventId::new();
}
