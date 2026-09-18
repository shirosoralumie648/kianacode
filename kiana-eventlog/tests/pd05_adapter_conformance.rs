use async_trait::async_trait;
use kiana_domain::{AggregateVersion, CommitOutcome, RequestId, RuntimeEvent, TransitionBatch};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd05-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
}

fn batch(command_id: RequestId, aggregate: &str, digest: char) -> TransitionBatch {
    let event = RuntimeEvent::new(command_id, 1, "run.started", json!({"run_id": aggregate}))
        .unwrap()
        .with_stream_metadata("run", aggregate, 1)
        .with_idempotency_key(format!("pd05:{aggregate}"));
    TransitionBatch {
        command_id,
        command_digest: std::iter::repeat(digest).take(64).collect(),
        expected_versions: vec![AggregateVersion::new("run", aggregate, 0)],
        events: vec![event],
    }
}

async fn assert_conformance(store: &dyn EventStorePort) {
    assert!(store.supports_atomic_transitions());
    let command_id = RequestId::new();
    let first = batch(command_id, "pd05-run", 'a');
    let committed = store.commit_transition(first.clone()).await.unwrap();
    assert!(matches!(committed, CommitOutcome::Committed { .. }));
    let replay = store.commit_transition(first).await.unwrap();
    assert!(matches!(replay, CommitOutcome::Replayed { .. }));
    let receipt = store.read_command(&command_id).await.unwrap();
    assert!(receipt.is_some());
    let page = store.read_from(0, 1).await.unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.cursor, 1);
    assert!(!page.has_more);
    let stale = store
        .commit_transition(batch(RequestId::new(), "pd05-run", 'b'))
        .await
        .unwrap();
    assert!(matches!(stale, CommitOutcome::Conflict { .. }));
    assert_eq!(store.read_stream("run", "pd05-run").await.unwrap().len(), 1);
}

#[tokio::test]
async fn memory_and_jsonl_share_eventstore_conformance_semantics() {
    let memory = MemoryEventLog::new();
    assert_conformance(&memory).await;

    let path = temp_path("jsonl");
    let jsonl = JsonlEventLog::open(&path).unwrap();
    #[cfg(unix)]
    {
        assert_conformance(&jsonl).await;
        jsonl.flush().await.unwrap();
    }
    #[cfg(not(unix))]
    assert!(!jsonl.supports_atomic_transitions());
    drop(jsonl);
    cleanup(&path);
}

struct NonAtomicStore;

#[async_trait]
impl EventStorePort for NonAtomicStore {
    async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
        Ok(())
    }

    async fn read_request(&self, _request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn non_atomic_adapter_is_rejected_before_transition_write() {
    let store = NonAtomicStore;
    assert!(!store.supports_atomic_transitions());
    assert_eq!(
        store
            .commit_transition(batch(RequestId::new(), "pd05-non-atomic", 'a'))
            .await
            .unwrap_err(),
        PortError::Unavailable("event_store_atomic_transitions_unsupported".to_owned())
    );
}
