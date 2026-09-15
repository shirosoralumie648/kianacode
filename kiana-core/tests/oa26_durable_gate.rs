use kiana_domain::{
    json_digest, EntryPointKind, EventStoreCapabilities, RequestId, RunId, RuntimeEvent,
};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::{EventStorePort, ObservabilityQueue, QueuedObservabilityItem};
use serde_json::json;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-oa26-{stamp}.jsonl"))
}

fn event(run_id: RunId, request_id: RequestId, sequence: u64, kind: &str) -> RuntimeEvent {
    RuntimeEvent::new(
        request_id,
        sequence,
        kind,
        json!({
            "run_id": run_id,
            "session_id": "session-1",
            "actor_id": "local-user",
            "project_root": "/repo",
            "authority_epoch": 1,
            "data_epoch": 1
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn jsonl_restart_preserves_facts_and_rebuilds_unknown_projection() {
    let path = temp_path();
    let run_id = RunId::new();
    let request_id = RequestId::new();
    {
        let store = JsonlEventLog::open(&path).unwrap();
        assert!(store.capabilities().durable_commits);
        store
            .append(event(run_id, request_id, 1, "run.authorized"))
            .await
            .unwrap();
        store
            .append(event(run_id, request_id, 2, "invocation.executing"))
            .await
            .unwrap();
        store
            .append(event(run_id, request_id, 3, "run.result_unknown"))
            .await
            .unwrap();
    }
    let reopened = JsonlEventLog::open(&path).unwrap();
    let events = reopened.read_all().await.unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].kind, "run.authorized");
    assert_eq!(events[2].kind, "run.result_unknown");
    let first =
        kiana_core::project_entrypoint_parity(EntryPointKind::Cli, Some(run_id), &events).unwrap();
    let second =
        kiana_core::project_entrypoint_parity(EntryPointKind::Desktop, Some(run_id), &events)
            .unwrap();
    assert_eq!(first.status, kiana_domain::ExecutionStatus::ResultUnknown);
    assert_eq!(first.source_cursor, second.source_cursor);
    assert_eq!(first.source_event_ids, second.source_event_ids);
    assert_eq!(first.receipt_digest, second.receipt_digest);
    assert_eq!(first.audit_digest, second.audit_digest);
    assert_eq!(first.health_digest, second.health_digest);
    assert_eq!(
        json_digest(&serde_json::to_value(&events).unwrap()).starts_with("sha256:"),
        true
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn durable_gate_keeps_capability_and_queue_limits_explicit() {
    let capabilities = EventStoreCapabilities::default();
    assert!(!capabilities.durable_commits);
    assert!(!capabilities.command_receipts);
    let queue = ObservabilityQueue::new(1).unwrap();
    queue
        .try_enqueue(QueuedObservabilityItem::critical(
            kiana_ports::ObservabilityQueueClass::Event,
            1,
        ))
        .unwrap();
    assert!(queue
        .try_enqueue(QueuedObservabilityItem::critical(
            kiana_ports::ObservabilityQueueClass::Terminal,
            2,
        ))
        .is_err());
    assert_eq!(queue.stats().critical_rejected_total, 1);
}
