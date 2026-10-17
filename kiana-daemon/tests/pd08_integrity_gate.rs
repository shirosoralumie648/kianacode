//! PD-08 daemon-side fixtures: the startup storage integrity gate must classify the four store
//! classes and refuse to let an unhealthy journal proceed to execution.

use kiana_daemon::{StorageIntegrityAdmission, StorageIntegrityGate};
use kiana_domain::{RequestId, RuntimeEvent};
use kiana_eventlog::IntegrityScanStatus;
use kiana_ports::PortError;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd08-gate-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("jsonl.lock"));
}

fn write_fixture(path: &Path, contents: impl AsRef<[u8]>) {
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
        file.write_all(contents.as_ref()).unwrap();
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents.as_ref()).unwrap();
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

fn unknown_format_contents() -> String {
    let record = json!({
        "schema": "kiana.journal-header.v9",
        "required": true,
        "writer_version": 9,
    });
    format!("{}\n", serde_json::to_string(&record).unwrap())
}

#[tokio::test]
async fn empty_and_complete_stores_are_admitted() {
    let empty = temp_path("empty");
    let gate = StorageIntegrityGate::inspect(&empty).await;
    assert_eq!(gate.status(), IntegrityScanStatus::Empty);
    assert_eq!(gate.admission(), StorageIntegrityAdmission::Allowed);
    gate.admit_execution().unwrap();
    cleanup(&empty);

    let complete = temp_path("complete");
    write_fixture(&complete, legacy_event_line("pd08-gate-ready"));
    let gate = StorageIntegrityGate::inspect(&complete).await;
    assert_eq!(gate.status(), IntegrityScanStatus::Ready);
    assert_eq!(gate.admission(), StorageIntegrityAdmission::Allowed);
    gate.admit_execution().unwrap();
    cleanup(&complete);
}

#[tokio::test]
async fn repaired_tail_is_reported_and_still_admitted() {
    let recovered = temp_path("repaired-tail");
    let torn = "{\"schema\":\"kiana.journal-frame.v2\",\"body_sha256\":\"truncated";
    let contents = legacy_event_line("pd08-gate-recovered") + torn;
    write_fixture(&recovered, contents);

    let gate = StorageIntegrityGate::inspect(&recovered).await;
    assert_eq!(gate.status(), IntegrityScanStatus::Recovered);
    assert!(gate.recovery().repaired_tail);
    assert_eq!(gate.recovery().repaired_bytes, torn.len() as u64);
    assert_eq!(gate.report().recovery_action, "repair_tail_and_continue");
    assert_eq!(gate.report().source_cursor, 1);
    assert_eq!(gate.admission(), StorageIntegrityAdmission::Allowed);
    gate.admit_execution().unwrap();
    cleanup(&recovered);
}

#[tokio::test]
async fn corrupt_middle_frame_blocks_execution_and_requires_quarantine() {
    let corrupt = temp_path("corrupt-middle");
    let contents = legacy_event_line("pd08-gate-corrupt") + "this is not a json record\n";
    write_fixture(&corrupt, contents);
    let before = fs::metadata(&corrupt).unwrap().len();

    let gate = StorageIntegrityGate::inspect(&corrupt).await;
    assert_eq!(gate.status(), IntegrityScanStatus::Corrupt);
    assert!(gate.quarantine_required());
    assert_eq!(gate.admission(), StorageIntegrityAdmission::Blocked);
    assert_eq!(
        gate.admit_execution().unwrap_err(),
        PortError::Conflict("eventlog_integrity_quarantine_required".to_owned())
    );
    // Read-only gate: a corrupt store is never repaired or truncated in place.
    assert_eq!(fs::metadata(&corrupt).unwrap().len(), before);
    cleanup(&corrupt);
}

#[tokio::test]
async fn unknown_required_format_blocks_execution_and_is_not_an_empty_store() {
    let unknown = temp_path("unknown-format");
    write_fixture(&unknown, unknown_format_contents());
    let before = fs::metadata(&unknown).unwrap().len();

    let gate = StorageIntegrityGate::inspect(&unknown).await;
    assert_eq!(gate.status(), IntegrityScanStatus::Unknown);
    assert_ne!(
        gate.status(),
        IntegrityScanStatus::Empty,
        "an unknown major must not be admitted as an empty store"
    );
    assert!(!gate.quarantine_required());
    let incident = gate.report().incident_code.as_deref().unwrap_or_default();
    assert!(
        incident.contains("eventlog_required_record_unsupported"),
        "{incident}"
    );
    assert_eq!(gate.admission(), StorageIntegrityAdmission::Blocked);
    assert_eq!(
        gate.admit_execution().unwrap_err(),
        PortError::Unavailable("eventlog_integrity_unknown".to_owned())
    );
    assert_eq!(fs::metadata(&unknown).unwrap().len(), before);
    cleanup(&unknown);
}
