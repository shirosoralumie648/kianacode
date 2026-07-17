use kiana_tasks::{
    append_workflow_event, initialize_local_hmac_key, initialize_workflow_run,
    inspect_workflow_integrity, read_workflow_events, seal_legacy_workflow,
    workflow_integrity_key_path, WorkflowEventKind, WorkflowInit, WorkflowInputKind,
    WorkflowProfile, WorkflowTrustStatus,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static INTEGRITY_ENV_LOCK: Mutex<()> = Mutex::new(());

struct IntegrityEnvGuard {
    previous_home: Option<std::ffi::OsString>,
    previous_key_file: Option<std::ffi::OsString>,
}

impl IntegrityEnvGuard {
    fn isolated(home: &std::path::Path) -> Self {
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_key_file = std::env::var_os("KIANA_WORKFLOW_INTEGRITY_KEY_FILE");
        std::env::set_var("KIANA_HOME", home);
        std::env::remove_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE");
        Self {
            previous_home,
            previous_key_file,
        }
    }
}

impl Drop for IntegrityEnvGuard {
    fn drop(&mut self) {
        match self.previous_home.take() {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match self.previous_key_file.take() {
            Some(value) => std::env::set_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", value),
            None => std::env::remove_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE"),
        }
    }
}

fn temp_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-workflow-integrity-{name}-{}-{unique}",
        std::process::id()
    ))
}

#[test]
fn signed_workflow_eventlog_detects_middle_event_mutation() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("tamper");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "signed workflow".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::Learned,
        "learn",
        json!({"value": "original"}),
    )
    .unwrap();

    let eventlog_path = run.artifact_dir.join("eventlog.jsonl");
    let mut events = std::fs::read_to_string(&eventlog_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    events[1]["node_id"] = json!("tampered");
    let contents = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&eventlog_path, format!("{contents}\n")).unwrap();

    let error = read_workflow_events(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(error.contains("integrity_payload_mismatch"), "{error}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn signed_workflow_eventlog_uses_v2_chain_and_verifies() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("verified");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let key = initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "verified workflow".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();

    let lines = std::fs::read_to_string(run.artifact_dir.join("eventlog.jsonl")).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record["schema"] == "kiana.workflow-event.v2"));
    assert!(records
        .iter()
        .all(|record| record["integrity"]["key_id"].as_str() == key.key_id.as_deref()));
    assert_eq!(records[0]["integrity"]["previous_record_sha256"], "genesis");
    assert!(records[1]["integrity"]["previous_record_sha256"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(read_workflow_events(&run.artifact_dir).unwrap().len(), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn signed_workflow_eventlog_rejects_deleted_and_reordered_events() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("sequence");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "sequence workflow".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::Learned,
        "learn",
        json!({"value": "third"}),
    )
    .unwrap();
    let eventlog_path = run.artifact_dir.join("eventlog.jsonl");
    let original = std::fs::read_to_string(&eventlog_path).unwrap();
    let lines = original.lines().collect::<Vec<_>>();

    std::fs::write(&eventlog_path, format!("{}\n{}\n", lines[0], lines[2])).unwrap();
    let deleted_error = read_workflow_events(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(deleted_error.contains("expected 2"), "{deleted_error}");

    std::fs::write(
        &eventlog_path,
        format!("{}\n{}\n{}\n", lines[1], lines[0], lines[2]),
    )
    .unwrap();
    let reordered_error = read_workflow_events(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(reordered_error.contains("expected 1"), "{reordered_error}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn signed_workflow_eventlog_rejects_missing_and_wrong_key() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("key-errors");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "key workflow".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let key_path = workflow_integrity_key_path().unwrap();
    let original_key = std::fs::read(&key_path).unwrap();

    std::fs::remove_file(&key_path).unwrap();
    let missing_error = read_workflow_events(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(
        missing_error.contains("integrity_key_missing"),
        "{missing_error}"
    );

    initialize_local_hmac_key().unwrap();
    let wrong_error = read_workflow_events(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(
        wrong_error.contains("integrity_key_id_mismatch"),
        "{wrong_error}"
    );

    std::fs::write(&key_path, original_key).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(read_workflow_events(&run.artifact_dir).unwrap().len(), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn signed_workflow_eventlog_rejects_unsigned_downgrade() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("downgrade");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "downgrade workflow".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let eventlog_path = run.artifact_dir.join("eventlog.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&eventlog_path)
        .unwrap();
    use std::io::Write;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&json!({
            "schema": "kiana.workflow-event.v1",
            "seq": 3,
            "at_ms": 1,
            "kind": "learned",
            "node_id": "learn",
            "data": {}
        }))
        .unwrap()
    )
    .unwrap();

    let error = read_workflow_events(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(error.contains("integrity_downgrade_detected"), "{error}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unsigned_legacy_workflow_remains_readable_without_key() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("legacy");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "legacy workflow".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();

    let events = read_workflow_events(&run.artifact_dir).unwrap();
    assert_eq!(events.len(), 2);
    assert!(events
        .iter()
        .all(|event| event.schema == "kiana.workflow-event.v1"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn legacy_workflow_requires_seal_before_signed_append() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("seal-required");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "legacy before key".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();
    initialize_local_hmac_key().unwrap();

    let error = append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::Learned,
        "learn",
        json!({"value": "blocked"}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("integrity_seal_required"), "{error}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn integrity_seal_is_idempotent_and_starts_authenticated_chain() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("seal");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "legacy to seal".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();
    initialize_local_hmac_key().unwrap();

    let first = seal_legacy_workflow(&run.artifact_dir).unwrap();
    let second = seal_legacy_workflow(&run.artifact_dir).unwrap();
    assert_eq!(first.legacy_eventlog_sha256, second.legacy_eventlog_sha256);
    assert_eq!(first.key_id, second.key_id);
    assert_eq!(first.legacy_event_count, 2);
    assert!(run
        .artifact_dir
        .join("integrity/genesis-seal.json")
        .is_file());

    append_workflow_event(
        &run.artifact_dir,
        WorkflowEventKind::Learned,
        "learn",
        json!({"value": "authenticated"}),
    )
    .unwrap();
    let events = read_workflow_events(&run.artifact_dir).unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].schema, "kiana.workflow-event.v1");
    assert_eq!(events[1].schema, "kiana.workflow-event.v1");
    assert_eq!(events[2].schema, "kiana.workflow-event.v2");
    assert_eq!(events[3].schema, "kiana.workflow-event.v2");

    let report = inspect_workflow_integrity(&run.artifact_dir).unwrap();
    assert_eq!(report.status, WorkflowTrustStatus::SealedLegacyPrefix);
    assert_eq!(report.legacy_prefix_count, 2);
    assert_eq!(report.verified_event_count, 2);
    assert_eq!(report.first_authenticated_seq, Some(3));
    assert_eq!(report.last_authenticated_seq, Some(4));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn integrity_seal_rejects_tampered_legacy_prefix() {
    let _lock = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root("seal-tamper");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "legacy tamper".to_string(),
            input_kind: WorkflowInputKind::Task,
            profile: WorkflowProfile::Standard,
            approval_required: false,
        },
    )
    .unwrap();
    initialize_local_hmac_key().unwrap();
    seal_legacy_workflow(&run.artifact_dir).unwrap();

    let eventlog_path = run.artifact_dir.join("eventlog.jsonl");
    let mut records = std::fs::read_to_string(&eventlog_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    records[0]["data"]["request"] = json!("tampered legacy request");
    let contents = records
        .iter()
        .map(|record| serde_json::to_string(record).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&eventlog_path, format!("{contents}\n")).unwrap();

    let error = read_workflow_events(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(error.contains("integrity_chain_mismatch"), "{error}");

    let _ = std::fs::remove_dir_all(root);
}
