use kiana_domain::{JournalFrame, JournalFramePayload, JournalHeader, RequestId, RuntimeEvent};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::EventStorePort;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 以生产写入器使用的私有权限（0o600）写入 JSONL 夹具。
///
/// `fs::write` 建出的文件是 0o644（group/other 可读），而 `JsonlEventLog::open`
/// 在 unix 上会以 `mode & 0o077 != 0` 判定 `eventlog_permissions_too_broad` 并拒绝打开。
/// 那是正确的安全检查——world-readable 的事件日志本身就是泄漏面，生产写入器也确实
/// 用 `openat(..., 0o600)` 建文件。这里修的是夹具：让它们按生产契约造出合法输入。
fn write_fixture(path: &Path, contents: impl AsRef<[u8]>) {
    let contents = contents.as_ref();
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(contents).unwrap();
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents).unwrap();
    }
}

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
    write_fixture(&torn, format!("{encoded}\n{{\"event_id\":"));
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
    write_fixture(
        &tampered,
        format!("{}\n{}\n", header, serde_json::to_string(&body).unwrap()),
    );
    assert!(JsonlEventLog::open(&tampered)
        .unwrap_err()
        .to_string()
        .contains("eventlog_corrupt"));
    cleanup(&tampered);
}

#[test]
fn jsonl_v2_source_contract_keeps_checksum_sync_lock_and_bounded_tail() {
    let source = include_str!("../src/jsonl.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let journal_core = include_str!("../src/journal_core.rs");
    for marker in [
        "JournalHeader",
        "JournalFrame",
        ".accept_frame(frame)",
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
    for marker in [
        "body_sha256",
        "encoded.len() as u64 != self.body_len",
        "journal_sha256(&encoded) != self.body_sha256",
        "journal_frame_integrity_failed",
    ] {
        assert!(
            journal.contains(marker),
            "journal frame integrity marker missing: {marker}"
        );
    }
    let (_, accept_frame) = journal_core
        .split_once("pub fn accept_frame")
        .expect("journal core must accept decoded frames");
    let validation = accept_frame
        .find("frame.validate().map_err(invalid)?;")
        .expect("journal core must validate frame integrity");
    let body = accept_frame
        .find("match frame.body")
        .expect("journal core must apply validated frame contents");
    assert!(validation < body, "frame integrity must precede body apply");
}
