use kiana_domain::{JournalFrame, JournalFramePayload, JournalHeader, RequestId, RuntimeEvent};
use kiana_eventlog::{scan_jsonl, IntegrityScanStatus};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd08-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("jsonl.lock"));
}

#[tokio::test]
async fn integrity_scan_distinguishes_empty_ready_and_corrupt() {
    let empty = temp_path("empty");
    let empty_report = scan_jsonl(&empty).await;
    assert_eq!(empty_report.status, IntegrityScanStatus::Empty);
    empty_report.health_gate().unwrap();
    cleanup(&empty);

    let ready = temp_path("ready");
    let event = RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap();
    fs::write(&ready, serde_json::to_string(&event).unwrap() + "\n").unwrap();
    let ready_report = scan_jsonl(&ready).await;
    assert_eq!(ready_report.status, IntegrityScanStatus::Ready);
    assert_eq!(ready_report.source_cursor, 1);
    ready_report.health_gate().unwrap();
    cleanup(&ready);

    let corrupt = temp_path("corrupt");
    let frame = JournalFrame::new(JournalFramePayload::Event {
        event: RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap(),
    })
    .unwrap();
    let header = serde_json::to_string(&JournalHeader::default()).unwrap();
    let mut value = serde_json::to_value(frame).unwrap();
    value["body_sha256"] = json!("0".repeat(64));
    fs::write(
        &corrupt,
        format!("{}\n{}\n", header, serde_json::to_string(&value).unwrap()),
    )
    .unwrap();
    let corrupt_report = scan_jsonl(&corrupt).await;
    assert_eq!(corrupt_report.status, IntegrityScanStatus::Corrupt);
    assert!(corrupt_report.quarantine_required);
    assert!(corrupt_report.health_gate().is_err());
    cleanup(&corrupt);
}
