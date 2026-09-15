use kiana_domain::{AggregateVersion, CommitOutcome, RequestId, RuntimeEvent, TransitionBatch};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;

fn transition(
    command_id: RequestId,
    aggregate: &str,
    expected: u64,
    digest: char,
) -> TransitionBatch {
    let event = RuntimeEvent::new(
        command_id,
        expected.saturating_add(1),
        "run.authorized",
        json!({"run_id":aggregate,"decision":"allow"}),
    )
    .unwrap()
    .with_stream_metadata("run", aggregate, expected.saturating_add(1));
    TransitionBatch {
        command_id,
        command_digest: std::iter::repeat(digest).take(64).collect(),
        expected_versions: vec![AggregateVersion::new("run", aggregate, expected)],
        events: vec![event],
    }
}

#[tokio::test]
async fn cp_transition_conflict_changes_no_aggregate() {
    let store = MemoryEventLog::new();
    let first = transition(RequestId::new(), "run-cas", 0, 'a');
    assert!(matches!(
        store.commit_transition(first).await.unwrap(),
        CommitOutcome::Committed { .. }
    ));
    let stale = transition(RequestId::new(), "run-cas", 0, 'b');
    let outcome = store.commit_transition(stale).await.unwrap();
    assert!(matches!(outcome, CommitOutcome::Conflict { .. }));
    assert_eq!(store.read_stream("run", "run-cas").await.unwrap().len(), 1);
}

#[tokio::test]
async fn cp_same_command_different_payload_is_conflict() {
    let store = MemoryEventLog::new();
    let command = RequestId::new();
    store
        .commit_transition(transition(command, "run-command", 0, 'a'))
        .await
        .unwrap();
    let error = store
        .commit_transition(transition(command, "run-command", 0, 'b'))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PortError::Conflict("event_store_command_digest_mismatch".to_owned())
    );
    assert_eq!(store.read_all().await.unwrap().len(), 1);
}

#[tokio::test]
async fn cp_stale_allow_is_recomputed_after_cas_conflict() {
    let store = MemoryEventLog::new();
    let aggregate = "run-recompute";
    store
        .commit_transition(transition(RequestId::new(), aggregate, 0, 'a'))
        .await
        .unwrap();
    let outcome = store
        .commit_transition(transition(RequestId::new(), aggregate, 0, 'c'))
        .await
        .unwrap();
    let changed = match outcome {
        CommitOutcome::Conflict { changed } => changed,
        other => panic!("expected CAS conflict, got {other:?}"),
    };
    assert_eq!(changed[0].version, 1);
    assert_eq!(store.read_stream("run", aggregate).await.unwrap().len(), 1);
}
