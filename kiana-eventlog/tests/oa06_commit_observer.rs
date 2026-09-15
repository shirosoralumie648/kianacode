use async_trait::async_trait;
use kiana_domain::{
    AggregateVersion, CommitOutcome, EventId, RequestId, RuntimeEvent, TransitionBatch,
};
use kiana_eventlog::{MemoryEventLog, StreamEventStore};
use kiana_ports::{CommittedTransition, EventStoreCommitObserver, EventStorePort, PortError};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

fn transition(command_id: RequestId, aggregate_id: &str, version: u64) -> TransitionBatch {
    let event = RuntimeEvent::new(
        command_id,
        version,
        "run.accepted",
        json!({"run_id": aggregate_id}),
    )
    .unwrap()
    .with_stream_metadata("run", aggregate_id, version);
    TransitionBatch {
        command_id,
        command_digest: format!("{:064x}", version),
        expected_versions: vec![AggregateVersion::new("run", aggregate_id, version - 1)],
        events: vec![event],
    }
}

#[derive(Clone)]
struct RecordingObserver {
    seen: Arc<Mutex<Vec<CommittedTransition>>>,
    fail_next: Arc<AtomicBool>,
    store: Arc<MemoryEventLog>,
}

#[async_trait]
impl EventStoreCommitObserver for RecordingObserver {
    async fn on_committed(&self, transition: CommittedTransition) -> Result<(), PortError> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(PortError::Failed("observer_injected_failure".to_owned()));
        }
        let receipt = self
            .store
            .read_command(&transition.batch.command_id)
            .await?
            .ok_or_else(|| PortError::Failed("observer_saw_precommit".to_owned()))?;
        if receipt != transition.receipt {
            return Err(PortError::Conflict("observer_receipt_mismatch".to_owned()));
        }
        self.seen.lock().unwrap().push(transition);
        Ok(())
    }
}

#[tokio::test]
async fn only_fresh_committed_transitions_notify_and_replay_links_original_receipt() {
    let store = Arc::new(MemoryEventLog::new());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = Arc::new(RecordingObserver {
        seen: seen.clone(),
        fail_next: Arc::new(AtomicBool::new(false)),
        store: store.clone(),
    });
    let observed = StreamEventStore::with_observer(store.clone(), observer).unwrap();
    let command_id = RequestId::new();
    let batch = transition(command_id, "run-a", 1);

    let first = observed.commit_transition(batch.clone()).await.unwrap();
    let receipt = match first {
        CommitOutcome::Committed { receipt } => receipt,
        other => panic!("expected committed outcome, got {other:?}"),
    };
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(seen.lock().unwrap()[0].source_cursor(), receipt.cursor);
    assert_eq!(observed.committed_cursor(), receipt.cursor);

    let replay = observed.commit_transition(batch).await.unwrap();
    assert_eq!(replay, CommitOutcome::Replayed { original: receipt });
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(observed.committed_cursor(), 1);

    let conflict = transition(RequestId::new(), "run-a", 2);
    assert!(matches!(
        observed.commit_transition(conflict).await.unwrap(),
        CommitOutcome::Conflict { .. }
    ));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn observer_failure_is_diagnostic_and_never_rewrites_a_committed_outcome() {
    let store = Arc::new(MemoryEventLog::new());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fail_next = Arc::new(AtomicBool::new(true));
    let observer = Arc::new(RecordingObserver {
        seen: seen.clone(),
        fail_next: fail_next.clone(),
        store: store.clone(),
    });
    let observed = StreamEventStore::with_observer(store, observer).unwrap();
    let batch = transition(RequestId::new(), "run-b", 1);
    assert!(matches!(
        observed.commit_transition(batch.clone()).await.unwrap(),
        CommitOutcome::Committed { .. }
    ));
    assert_eq!(observed.observer_failures().len(), 1);
    assert!(observed.observer_failures()[0]
        .reason
        .contains("observer_injected_failure"));
    assert!(matches!(
        observed.commit_transition(batch).await.unwrap(),
        CommitOutcome::Replayed { .. }
    ));
    assert_eq!(observed.observer_failures().len(), 1);
    assert!(seen.lock().unwrap().is_empty());
}

#[derive(Default)]
struct UnknownStore;

#[async_trait]
impl EventStorePort for UnknownStore {
    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        Ok(CommitOutcome::Unknown {
            command_id: batch.command_id,
            reason: "injected_unknown".to_owned(),
        })
    }

    async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
        Err(PortError::Unavailable("unused".to_owned()))
    }

    async fn read_request(&self, _request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Err(PortError::Unavailable("unused".to_owned()))
    }
}

#[tokio::test]
async fn conflict_and_unknown_outcomes_never_publish_observer_notifications() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let unknown = StreamEventStore::with_observer(
        Arc::new(UnknownStore),
        Arc::new(RecordingObserver {
            seen: seen.clone(),
            fail_next: Arc::new(AtomicBool::new(false)),
            store: Arc::new(MemoryEventLog::new()),
        }),
    )
    .unwrap();
    let outcome = unknown
        .commit_transition(transition(RequestId::new(), "run-c", 1))
        .await
        .unwrap();
    assert!(matches!(outcome, CommitOutcome::Unknown { .. }));
    assert!(unknown.observer_failures().is_empty());
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn committed_transition_rejects_forged_cursor_or_event_identity() {
    let command_id = RequestId::new();
    let batch = transition(command_id, "run-d", 1);
    let receipt = kiana_domain::CommandReceipt {
        command_id,
        command_digest: batch.command_digest.clone(),
        commit_id: EventId::new(),
        first_cursor: 1,
        cursor: 2,
        event_ids: batch.events.iter().map(|event| event.event_id).collect(),
        versions: vec![AggregateVersion::new("run", "run-d", 1)],
    };
    assert!(CommittedTransition::new(batch, receipt).is_err());
}
