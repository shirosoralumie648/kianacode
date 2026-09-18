use kiana_domain::{JournalFrame, JournalFramePayload, JournalHeader, RequestId, RuntimeEvent};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::EventStorePort;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd06-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("jsonl.lock"));
}

#[tokio::test]
async fn jsonl_frame_checksum_and_torn_tail_recovery_are_bounded() {
    let torn = temp_path("tail");
    let event =
        RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({"safe": true})).unwrap();
    let encoded = serde_json::to_string(&event).unwrap();
    fs::write(&torn, format!("{encoded}\n{{\"event_id\":")).unwrap();
    let store = JsonlEventLog::open(&torn).unwrap();
    assert_eq!(store.read_all().await.unwrap(), vec![event]);
    assert_eq!(fs::read_to_string(&torn).unwrap(), format!("{encoded}\n"));
    drop(store);
    cleanup(&torn);

    let tampered = temp_path("checksum");
    let frame = JournalFrame::new(JournalFramePayload::Event {
        event: RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap(),
    })
    .unwrap();
    let header = serde_json::to_string(&JournalHeader::default()).unwrap();
    let mut body = serde_json::to_value(frame).unwrap();
    body["body_sha256"] = json!("0".repeat(64));
    fs::write(
        &tampered,
        format!("{}\n{}\n", header, serde_json::to_string(&body).unwrap()),
    )
    .unwrap();
    assert!(JsonlEventLog::open(&tampered)
        .unwrap_err()
        .to_string()
        .contains("eventlog_corrupt"));
    cleanup(&tampered);
}

#[test]
fn jsonl_v2_source_contract_keeps_checksum_sync_lock_and_bounded_tail() {
    let source = include_str!("../src/jsonl.rs");
    for marker in [
        "JournalHeader",
        "JournalFrame",
        "body_sha256",
        "sync_all",
        "sync_directory",
        "libc::flock",
        "O_APPEND",
        "O_NOFOLLOW",
        "eventlog_repair_failed",
        "MAX_JOURNAL_LOG_BYTES",
        "uncertain_write",
    ] {
        assert!(
            source.contains(marker),
            "JSONL durability marker missing: {marker}"
        );
    }
}
