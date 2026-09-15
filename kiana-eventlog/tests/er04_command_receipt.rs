use kiana_domain::{AggregateVersion, CommitOutcome, RequestId, RuntimeEvent, TransitionBatch};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;

fn batch(command_id: RequestId, aggregate: &str, expected: u64, digest: char) -> TransitionBatch {
    let event = RuntimeEvent::new(
        command_id,
        expected.saturating_add(1),
        "run.authorized",
        json!({"run_id":aggregate}),
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
async fn transition_missing_dependency_is_denied() {
    let store = MemoryEventLog::new();
    let command_id = RequestId::new();
    let event = RuntimeEvent::new(
        command_id,
        1,
        "run.authorized",
        json!({"run_id":"run-missing"}),
    )
    .unwrap()
    .with_stream_metadata("run", "run-missing", 1);
    let missing = TransitionBatch {
        command_id,
        command_digest: "a".repeat(64),
        expected_versions: vec![AggregateVersion::new("authority", "authority-1", 0)],
        events: vec![event],
    };
    assert_eq!(
        store.commit_transition(missing).await.unwrap_err(),
        PortError::Failed("journal_write_not_in_read_set".to_owned())
    );
    assert!(store.read_all().await.unwrap().is_empty());
}

#[tokio::test]
async fn read_set_conflict_appends_nothing() {
    let store = MemoryEventLog::new();
    store
        .commit_transition(batch(RequestId::new(), "run-read-set", 0, 'a'))
        .await
        .unwrap();
    let stale = batch(RequestId::new(), "run-read-set", 0, 'b');
    assert!(matches!(
        store.commit_transition(stale).await.unwrap(),
        CommitOutcome::Conflict { .. }
    ));
    assert_eq!(store.read_all().await.unwrap().len(), 1);
}

#[tokio::test]
async fn unknown_commit_never_dispatches() {
    let store = UnknownStore;
    let command_id = RequestId::new();
    let outcome = store
        .commit_transition(batch(command_id, "run-unknown", 0, 'c'))
        .await
        .unwrap();
    assert!(matches!(outcome, CommitOutcome::Unknown { command_id: id, .. } if id == command_id));
    assert!(store.read_command(&command_id).await.unwrap().is_none());
}

struct UnknownStore;

#[async_trait::async_trait]
impl EventStorePort for UnknownStore {
    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        Ok(CommitOutcome::Unknown {
            command_id: batch.command_id,
            reason: "commit_unconfirmed".to_owned(),
        })
    }

    async fn read_command(
        &self,
        _command_id: &RequestId,
    ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
        Ok(None)
    }

    async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
        Err(PortError::Unavailable("unused".to_owned()))
    }

    async fn read_request(&self, _request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(Vec::new())
    }
}
