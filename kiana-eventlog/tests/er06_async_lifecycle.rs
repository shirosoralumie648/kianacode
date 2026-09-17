use kiana_domain::{EventStoreHealth, RequestId, RuntimeEvent};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_log(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-er06-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("jsonl.lock"));
}

#[tokio::test]
async fn jsonl_flush_health_cursor_and_close_are_explicit_acknowledgements() {
    let path = temp_log("lifecycle");
    let store = JsonlEventLog::open(&path).unwrap();
    let initial = store.health().await.unwrap();
    assert_eq!(initial.last_durable_cursor, 0);
    assert!(initial.validate().is_ok());

    store
        .append(RuntimeEvent::new(RequestId::new(), 1, "legacy", Value::Null).unwrap())
        .await
        .unwrap();
    let flushed = store.flush().await.unwrap();
    assert!(flushed.durable);
    assert_eq!(flushed.last_durable_cursor, 1);
    assert!(flushed.validate().is_ok());
    assert_eq!(store.last_durable_cursor().await.unwrap(), 1);

    let closed = store.close().await.unwrap();
    assert!(closed.closed);
    assert!(closed.validate().is_ok());
    assert!(
        matches!(store.health().await, Err(PortError::Failed(reason)) if reason == "eventlog_closed")
    );
    assert!(
        matches!(store.flush().await, Err(PortError::Failed(reason)) if reason == "eventlog_closed")
    );
    cleanup(&path);
}

#[test]
fn er06_eventlog_contract_is_bounded_and_observable() {
    let jsonl = include_str!("../src/jsonl.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let daemon_stream = include_str!("../../kiana-daemon/src/run_stream.rs");
    let core = include_str!("../../kiana-core/src/receipts.rs");
    for marker in [
        "MAX_STORAGE_WORKERS",
        "try_acquire_owned",
        "spawn_blocking",
        "eventlog_worker_queue_full",
        "eventlog_worker_failed",
        "AtomicU8",
        "compare_exchange",
        "eventlog_close_in_progress",
        "flush_store",
        "sync_all",
    ] {
        assert!(
            jsonl.contains(marker),
            "ER-06 JSONL marker missing: {marker}"
        );
    }
    for marker in [
        "async fn flush",
        "async fn health",
        "async fn last_durable_cursor",
        "async fn close",
        "event_store_flush_unsupported",
        "event_store_close_unsupported",
    ] {
        assert!(
            ports.contains(marker),
            "ER-06 port marker missing: {marker}"
        );
    }
    for marker in [
        "flush_event_store",
        "event_store_health",
        "last_durable_cursor",
        "close_event_store",
    ] {
        assert!(
            daemon.contains(marker) || daemon_stream.contains(marker) || core.contains(marker),
            "ER-06 lifecycle delegation missing: {marker}"
        );
    }
    let empty = EventStoreHealth::new(0, true, false, 0, false);
    assert!(empty.validate().is_ok());
}
