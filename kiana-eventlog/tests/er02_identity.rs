use kiana_domain::{AggregateVersion, EventId, RequestId, RuntimeEvent, TransitionBatch};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::EventStorePort;
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
    assert_eq!(
        store.append(duplicate).await.unwrap_err().to_string(),
        "event_id_duplicate"
    );
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
    assert_eq!(error.to_string(), "event_store_command_digest_mismatch");
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
