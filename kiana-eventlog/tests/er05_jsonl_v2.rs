use kiana_domain::{
    canonical_journal_bytes, JournalFrame, JournalFramePayload, JournalHeader, RequestId,
    RuntimeEvent, TransitionBatch,
};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::EventStorePort;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_log(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-er05-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("jsonl.lock"));
}

fn run_event(request_id: RequestId, run_id: &str, sequence: u64, kind: &str) -> RuntimeEvent {
    RuntimeEvent::new(
        request_id,
        sequence,
        kind,
        json!({"run_id":run_id,"sequence":sequence}),
    )
    .unwrap()
    .with_stream_metadata("run", run_id, sequence)
    .with_idempotency_key(format!("er05:{run_id}:{sequence}"))
}

#[tokio::test]
async fn jsonl_v2_transaction_page_is_atomic_and_replays_after_reopen() {
    let path = temp_log("page");
    let request_id = RequestId::new();
    let first = run_event(request_id, "run-er05", 1, "run.started");
    let second = run_event(request_id, "run-er05", 2, "run.completed");
    let batch = TransitionBatch {
        command_id: request_id,
        command_digest: "a".repeat(64),
        expected_versions: vec![kiana_domain::AggregateVersion::new("run", "run-er05", 0)],
        events: vec![first.clone(), second.clone()],
    };

    let store = JsonlEventLog::open(&path).unwrap();
    assert!(store.supports_atomic_transitions());
    let committed = store.commit_transition(batch.clone()).await.unwrap();
    assert!(committed.receipt().is_some());
    let page = store.read_from(0, 1).await.unwrap();
    assert_eq!(page.events, vec![first.clone(), second.clone()]);
    assert_eq!(page.cursor, 2);
    assert!(page.has_more == false);
    assert!(store.read_from(1, 1).await.is_err());
    drop(store);

    let bytes = fs::read_to_string(&path).unwrap();
    assert!(bytes.contains("kiana.journal-header.v2"));
    assert!(bytes.contains("kiana.transition-frame.v1"));
    let reopened = JsonlEventLog::open(&path).unwrap();
    assert_eq!(reopened.read_all().await.unwrap(), vec![first, second]);
    cleanup(&path);
}

#[test]
fn jsonl_v2_rejects_malformed_first_record_and_tampered_frame() {
    let malformed = temp_log("malformed");
    fs::write(&malformed, b"{\"schema\":\n").unwrap();
    let error = JsonlEventLog::open(&malformed).unwrap_err();
    assert!(error.to_string().contains("eventlog_corrupt"));
    cleanup(&malformed);

    let tampered = temp_log("checksum");
    let event = RuntimeEvent::new(RequestId::new(), 1, "legacy", Value::Null).unwrap();
    let frame = JournalFrame::new(JournalFramePayload::Event { event }).unwrap();
    let mut header = serde_json::to_string(&JournalHeader::default()).unwrap();
    header.push('\n');
    let mut value = serde_json::to_value(frame).unwrap();
    value["body_sha256"] = json!("0".repeat(64));
    let frame = serde_json::to_string(&value).unwrap();
    fs::write(&tampered, format!("{header}{frame}\n")).unwrap();
    let error = JsonlEventLog::open(&tampered).unwrap_err();
    assert!(error.to_string().contains("eventlog_corrupt"));
    cleanup(&tampered);
}

#[tokio::test]
async fn jsonl_v2_repairs_only_a_torn_tail_and_rejects_legacy_after_upgrade() {
    let torn = temp_log("torn");
    let first = RuntimeEvent::new(RequestId::new(), 1, "legacy", Value::Null).unwrap();
    let encoded = serde_json::to_string(&first).unwrap();
    fs::write(&torn, format!("{encoded}\n{{\"event_id\":")).unwrap();
    let store = JsonlEventLog::open(&torn).unwrap();
    assert_eq!(store.read_all().await.unwrap(), vec![first.clone()]);
    assert_eq!(fs::read_to_string(&torn).unwrap(), format!("{encoded}\n"));
    cleanup(&torn);

    let upgraded = temp_log("upgrade");
    let legacy = RuntimeEvent::new(RequestId::new(), 1, "legacy", Value::Null).unwrap();
    let header = serde_json::to_string(&JournalHeader::default()).unwrap();
    fs::write(
        &upgraded,
        format!("{header}\n{}\n", serde_json::to_string(&legacy).unwrap()),
    )
    .unwrap();
    let error = JsonlEventLog::open(&upgraded).unwrap_err();
    assert!(error
        .to_string()
        .contains("eventlog_legacy_writer_after_upgrade"));
    cleanup(&upgraded);
}

#[test]
fn jsonl_v2_source_contract_keeps_lock_sync_and_bounded_recovery() {
    let source = include_str!("../src/jsonl.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    for marker in [
        "libc::flock",
        "libc::openat",
        "O_NOFOLLOW",
        "sync_all",
        "sync_directory",
        "verify_named_file",
        "read_bounded_line",
        "eventlog_legacy_writer_after_upgrade",
        "eventlog_frame_header_missing",
        "eventlog_corrupt",
        "MAX_JOURNAL_FRAME_BYTES",
        "MAX_JOURNAL_LOG_BYTES",
    ] {
        assert!(
            source.contains(marker),
            "ER-05 storage marker missing: {marker}"
        );
    }
    for marker in [
        "JOURNAL_HEADER_SCHEMA",
        "JOURNAL_FRAME_SCHEMA",
        "JournalHeader",
        "JournalFrame",
        "body_len",
        "body_sha256",
        "logical_events",
        "journal_frame_integrity_failed",
    ] {
        assert!(
            journal.contains(marker),
            "ER-05 domain marker missing: {marker}"
        );
    }
    let frame = JournalFrame::new(JournalFramePayload::Event {
        event: RuntimeEvent::new(RequestId::new(), 1, "legacy", Value::Null).unwrap(),
    })
    .unwrap();
    assert_eq!(
        frame.body_len as usize,
        canonical_journal_bytes(&frame.body).unwrap().len()
    );
}
