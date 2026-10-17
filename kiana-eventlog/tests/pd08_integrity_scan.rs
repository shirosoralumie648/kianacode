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

fn legacy_event_line(run_id: &str) -> String {
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "run.started",
        json!({"run_id": run_id, "sequence": 1}),
    )
    .unwrap()
    .with_stream_metadata("run", run_id, 1);
    serde_json::to_string(&event).unwrap() + "\n"
}

/// A torn tail: a truncated JSON frame with no trailing newline, after a committed record.
const TORN_TAIL: &str = "{\"schema\":\"kiana.journal-frame.v2\",\"body_sha256\":\"truncated";

/// A required record written by a version this build does not understand.
fn unknown_format_contents() -> String {
    let record = json!({
        "schema": "kiana.journal-header.v9",
        "required": true,
        "writer_version": 9,
    });
    format!("{}\n", serde_json::to_string(&record).unwrap())
}

/// A corrupt middle record: a v2 frame whose checksum does not match its body.
fn corrupt_middle_contents() -> String {
    let frame = JournalFrame::new(JournalFramePayload::Event {
        event: RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap(),
    })
    .unwrap();
    let header = serde_json::to_string(&JournalHeader::default()).unwrap();
    let mut value = serde_json::to_value(frame).unwrap();
    value["body_sha256"] = json!("0".repeat(64));
    let encoded = serde_json::to_string(&value).unwrap();
    format!("{}\n{}\n", header, encoded)
}

// 空库：文件不存在也不含有任何记录，必须与“未知格式”和“损坏”区分开。
#[tokio::test]
async fn integrity_scan_reports_an_empty_store_as_empty() {
    let empty = temp_path("empty");
    let report = scan_jsonl(&empty).await;
    assert_eq!(report.status, IntegrityScanStatus::Empty);
    assert_eq!(report.source_cursor, 0);
    assert!(!report.quarantine_required);
    assert!(!report.recovery.repaired_tail);
    assert_eq!(report.recovery_action, "continue");
    report.health_gate().unwrap();
    cleanup(&empty);
}

// 完整库：全部记录通过校验，必须报告 Ready 且不声称做过恢复。
#[tokio::test]
async fn integrity_scan_reports_a_complete_store_as_ready() {
    let ready = temp_path("ready");
    write_fixture(&ready, legacy_event_line("pd08-ready-run"));

    let report = scan_jsonl(&ready).await;
    assert_eq!(report.status, IntegrityScanStatus::Ready, "{report:?}");
    assert_eq!(report.source_cursor, 1);
    assert!(!report.recovery.repaired_tail);
    assert_eq!(report.recovery.repaired_bytes, 0);
    report.health_gate().unwrap();
    cleanup(&ready);
}

// 可修复尾部：已提交前缀后跟着一个被截断的尾记录，加载器回退到上一个已提交边界。
// 这一条必须有**独立的** Recovered 状态和恢复报告，不能静默当成 Ready 或 Empty。
#[tokio::test]
async fn integrity_scan_reports_a_repaired_tail_as_recovered() {
    let recovered = temp_path("repaired-tail");
    let contents = legacy_event_line("pd08-recovered-run") + TORN_TAIL;
    write_fixture(&recovered, contents);
    let before = fs::metadata(&recovered).unwrap().len();

    let report = scan_jsonl(&recovered).await;
    assert_eq!(report.status, IntegrityScanStatus::Recovered, "{report:?}");
    assert!(report.recovery.repaired_tail);
    assert_eq!(report.recovery.repaired_bytes, TORN_TAIL.len() as u64);
    assert_eq!(report.recovery_action, "repair_tail_and_continue");
    assert_eq!(report.source_cursor, 1, "committed prefix must survive");
    assert!(!report.quarantine_required);
    // 修复过尾部仍然通过健康门：只有未提交的尾字节被回退。
    report.health_gate().unwrap();

    // 磁盘上确实只剩已提交前缀，说明恢复动作可观察而不是“看起来没问题”。
    let after = fs::metadata(&recovered).unwrap().len();
    assert_eq!(after, before - TORN_TAIL.len() as u64);
    cleanup(&recovered);
}

// 损坏中段：v2 帧校验和不匹配。损坏记录绝不能被跳过或当成空库，必须隔离。
#[tokio::test]
async fn integrity_scan_reports_a_corrupt_middle_frame_as_corrupt() {
    let corrupt = temp_path("corrupt");
    write_fixture(&corrupt, corrupt_middle_contents());

    let report = scan_jsonl(&corrupt).await;
    assert_eq!(report.status, IntegrityScanStatus::Corrupt, "{report:?}");
    assert!(report.quarantine_required);
    assert_eq!(report.source_cursor, 0, "a corrupt store exposes no facts");
    let incident = report.incident_code.as_deref().unwrap_or_default();
    assert!(incident.contains("corrupt"), "{incident}");
    assert_eq!(report.recovery_action, "quarantine_and_reconcile");
    assert!(report.health_gate().is_err());
    cleanup(&corrupt);
}

// 未知格式：带 schema/required/writer_version 的必需记录是当前代码不认识的版本。
// 它必须被当成 Unknown（暂停调查），绝不能被当成一个空库继续执行。
#[tokio::test]
async fn integrity_scan_reports_an_unknown_required_format_as_unknown() {
    let unknown = temp_path("unknown-format");
    write_fixture(&unknown, unknown_format_contents());

    let report = scan_jsonl(&unknown).await;
    assert_eq!(report.status, IntegrityScanStatus::Unknown, "{report:?}");
    assert_ne!(
        report.status,
        IntegrityScanStatus::Empty,
        "an unknown major must never be reported as an empty store"
    );
    assert!(!report.quarantine_required);
    let incident = report.incident_code.as_deref().unwrap_or_default();
    assert!(
        incident.contains("eventlog_required_record_unsupported"),
        "{incident}"
    );
    assert_eq!(report.recovery_action, "pause_and_reconcile");
    assert!(report.health_gate().is_err());
    cleanup(&unknown);
}

// 四类 fixture 必须落到四个互不相同的状态上。
#[tokio::test]
async fn integrity_scan_statuses_are_pairwise_distinct_across_store_classes() {
    let empty = temp_path("distinct-empty");
    let empty_status = scan_jsonl(&empty).await.status;
    cleanup(&empty);

    let recovered = temp_path("distinct-recovered");
    let contents = legacy_event_line("pd08-distinct-run") + TORN_TAIL;
    write_fixture(&recovered, contents);
    let recovered_status = scan_jsonl(&recovered).await.status;
    cleanup(&recovered);

    let corrupt = temp_path("distinct-corrupt");
    write_fixture(&corrupt, corrupt_middle_contents());
    let corrupt_status = scan_jsonl(&corrupt).await.status;
    cleanup(&corrupt);

    let unknown = temp_path("distinct-unknown");
    write_fixture(&unknown, unknown_format_contents());
    let unknown_status = scan_jsonl(&unknown).await.status;
    cleanup(&unknown);

    assert_eq!(empty_status, IntegrityScanStatus::Empty);
    assert_eq!(recovered_status, IntegrityScanStatus::Recovered);
    assert_eq!(corrupt_status, IntegrityScanStatus::Corrupt);
    assert_eq!(unknown_status, IntegrityScanStatus::Unknown);
    assert_ne!(empty_status, recovered_status);
    assert_ne!(recovered_status, corrupt_status);
    assert_ne!(corrupt_status, unknown_status);
}
