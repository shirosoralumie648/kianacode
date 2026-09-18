use kiana_domain::{AggregateVersion, CommitOutcome, RequestId, RuntimeEvent, TransitionBatch};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_ports::EventStorePort;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn batch(command_id: RequestId, aggregate: &str, start: u64, count: u64) -> TransitionBatch {
    let events = (0..count)
        .map(|offset| {
            let sequence = start + offset;
            RuntimeEvent::new(
                command_id,
                sequence,
                "run.progress",
                json!({"run_id": aggregate, "sequence": sequence}),
            )
            .unwrap()
            .with_stream_metadata("run", aggregate, sequence)
            .with_idempotency_key(format!("pd07:{aggregate}:{sequence}"))
        })
        .collect();
    TransitionBatch {
        command_id,
        command_digest: "a".repeat(64),
        expected_versions: vec![AggregateVersion::new("run", aggregate, start - 1)],
        events,
    }
}

#[tokio::test]
async fn page_boundary_never_splits_a_transaction_and_indexes_match() {
    let store = MemoryEventLog::new();
    let command = RequestId::new();
    let first = batch(command, "pd07-run", 1, 3);
    assert!(matches!(
        store.commit_transition(first.clone()).await.unwrap(),
        CommitOutcome::Committed { .. }
    ));
    let second_command = RequestId::new();
    assert!(matches!(
        store
            .commit_transition(batch(second_command, "pd07-other", 1, 1))
            .await
            .unwrap(),
        CommitOutcome::Committed { .. }
    ));

    let page = store.read_from(0, 1).await.unwrap();
    assert_eq!(page.events.len(), 3);
    assert_eq!(page.cursor, 3);
    assert!(page.has_more);
    assert!(store.read_from(1, 1).await.is_err());
    let second_page = store.read_from(3, 1).await.unwrap();
    assert_eq!(second_page.events.len(), 1);
    assert_eq!(second_page.cursor, 4);
    assert!(!second_page.has_more);
    assert_eq!(store.read_request(&command).await.unwrap().len(), 3);
    assert_eq!(store.read_stream("run", "pd07-run").await.unwrap().len(), 3);
    assert!(store.read_command(&command).await.unwrap().is_some());
}

fn temp_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd07-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
}

#[tokio::test]
async fn jsonl_reopen_preserves_command_stream_and_page_indexes() {
    let path = temp_path();
    let command = RequestId::new();
    {
        let store = JsonlEventLog::open(&path).unwrap();
        if store.supports_atomic_transitions() {
            store
                .commit_transition(batch(command, "pd07-reopen", 1, 2))
                .await
                .unwrap();
            store.flush().await.unwrap();
        }
    }
    let reopened = JsonlEventLog::open(&path).unwrap();
    if reopened.supports_atomic_transitions() {
        assert_eq!(reopened.read_request(&command).await.unwrap().len(), 2);
        assert_eq!(
            reopened
                .read_stream("run", "pd07-reopen")
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(reopened.read_from(0, 1).await.unwrap().events.len(), 2);
    }
    drop(reopened);
    cleanup(&path);
}
