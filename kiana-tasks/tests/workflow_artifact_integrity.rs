use kiana_tasks::{
    commit_immutable_artifacts_with_unique_event, initialize_local_hmac_key,
    initialize_workflow_run, inspect_workflow_integrity, read_workflow_events,
    WorkflowArtifactBatch, WorkflowArtifactInput, WorkflowEventKind, WorkflowInit,
    WorkflowInputKind, WorkflowProfile,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

static INTEGRITY_ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    INTEGRITY_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct IntegrityEnvGuard {
    previous_home: Option<OsString>,
    previous_key_file: Option<OsString>,
}

impl IntegrityEnvGuard {
    fn isolated(home: &Path) -> Self {
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
        "kiana-workflow-artifact-integrity-{name}-{}-{unique}",
        std::process::id()
    ))
}

fn sha256_prefixed(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("sha256:{:x}", digest.finalize())
}

fn signed_workflow(root: &Path) -> kiana_tasks::WorkflowRun {
    initialize_local_hmac_key().unwrap();
    initialize_workflow_run(
        root,
        WorkflowInit {
            request: "bind immutable artifacts".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap()
}

#[test]
fn workflow_integrity_rejects_tampered_dag_snapshot() {
    let _lock = env_lock();
    let root = temp_root("dag-tamper");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    let dag_path = run.artifact_dir.join("workflow_dag.json");
    let mut dag: Value = serde_json::from_slice(&std::fs::read(&dag_path).unwrap()).unwrap();
    dag["description"] = json!("tampered workflow DAG");
    std::fs::write(&dag_path, serde_json::to_vec_pretty(&dag).unwrap()).unwrap();

    let error = inspect_workflow_integrity(&run.artifact_dir).unwrap_err();

    assert!(error.to_string().contains("DAG integrity"), "{error}");
    let _ = std::fs::remove_dir_all(root);
}

fn commit_verification_artifact(
    run: &kiana_tasks::WorkflowRun,
    packet_id: &str,
    contents: &[u8],
) -> kiana_tasks::WorkflowArtifactCommit {
    commit_immutable_artifacts_with_unique_event(
        &run.artifact_dir,
        WorkflowEventKind::ArtifactWritten,
        "artifact_binding",
        "packet_id",
        packet_id,
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: format!("verification/{packet_id}.json"),
                    contents: contents.to_vec(),
                }],
                event_data: json!({"packet_id": packet_id}),
            })
        },
    )
    .unwrap()
}

#[test]
fn immutable_commit_injects_sorted_authenticated_artifact_descriptors() {
    let _lock = env_lock();
    let root = temp_root("descriptor");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    let verification = br#"{"status":"pass"}"#.to_vec();
    let review = b"# Review\n".to_vec();

    let commit_once = || {
        commit_immutable_artifacts_with_unique_event(
            &run.artifact_dir,
            WorkflowEventKind::ArtifactWritten,
            "artifact_binding",
            "packet_id",
            "packet-1",
            |_, _| {
                Ok(WorkflowArtifactBatch {
                    artifacts: vec![
                        WorkflowArtifactInput {
                            relative_path: "verification/vp-1.json".to_string(),
                            contents: verification.clone(),
                        },
                        WorkflowArtifactInput {
                            relative_path: "review/rp-1.md".to_string(),
                            contents: review.clone(),
                        },
                    ],
                    event_data: json!({"packet_id": "packet-1"}),
                })
            },
        )
    };

    let first = commit_once().unwrap();
    let second = commit_once().unwrap();
    assert!(!first.reused_event);
    assert!(second.reused_event);
    assert_eq!(first.event.seq, second.event.seq);
    assert_eq!(first.event.data, second.event.data);

    let events = read_workflow_events(&run.artifact_dir).unwrap();
    let descriptors = events
        .last()
        .unwrap()
        .data
        .get("integrity_artifacts")
        .and_then(Value::as_array)
        .expect("missing integrity_artifacts");
    assert_eq!(descriptors.len(), 2);
    assert_eq!(descriptors[0]["path"], "review/rp-1.md");
    assert_eq!(descriptors[1]["path"], "verification/vp-1.json");
    assert_eq!(
        descriptors[0]["schema"],
        "kiana.workflow-artifact-descriptor.v1"
    );
    assert_eq!(descriptors[0]["media_type"], "text/markdown");
    assert_eq!(descriptors[0]["role"], "review_packet");
    assert_eq!(descriptors[0]["size"], review.len() as u64);
    assert_eq!(descriptors[0]["sha256"], sha256_prefixed(&review));
    assert_eq!(descriptors[1]["media_type"], "application/json");
    assert_eq!(descriptors[1]["role"], "verification_packet");
    assert_eq!(descriptors[1]["size"], verification.len() as u64);
    assert_eq!(descriptors[1]["sha256"], sha256_prefixed(&verification));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn immutable_commit_rejects_caller_supplied_integrity_artifacts_before_writing() {
    let _lock = env_lock();
    let root = temp_root("reserved");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    let initial_events = read_workflow_events(&run.artifact_dir).unwrap().len();
    let relative_path = "verification/forged.json";

    let error = commit_immutable_artifacts_with_unique_event(
        &run.artifact_dir,
        WorkflowEventKind::ArtifactWritten,
        "artifact_binding",
        "packet_id",
        "packet-forged",
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: relative_path.to_string(),
                    contents: b"{}".to_vec(),
                }],
                event_data: json!({
                    "packet_id": "packet-forged",
                    "integrity_artifacts": []
                }),
            })
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("integrity_artifacts is reserved"), "{error}");
    assert!(!run.artifact_dir.join(relative_path).exists());
    assert_eq!(
        read_workflow_events(&run.artifact_dir).unwrap().len(),
        initial_events
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_integrity_report_counts_verified_artifact_descriptors() {
    let _lock = env_lock();
    let root = temp_root("report");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    commit_verification_artifact(&run, "vp-1", br#"{"status":"pass"}"#);

    let report =
        serde_json::to_value(inspect_workflow_integrity(&run.artifact_dir).unwrap()).unwrap();
    assert_eq!(report["artifact_descriptor_count"], 1);
    assert_eq!(report["verified_artifact_count"], 1);
    assert_eq!(report["orphan_artifact_count"], 0);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_integrity_rejects_authenticated_artifact_content_mutation() {
    let _lock = env_lock();
    let root = temp_root("mutation");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    commit_verification_artifact(&run, "vp-1", br#"{"status":"pass"}"#);
    std::fs::write(
        run.artifact_dir.join("verification/vp-1.json"),
        br#"{"status":"forged"}"#,
    )
    .unwrap();

    let error = inspect_workflow_integrity(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("integrity_artifact_mismatch: verification/vp-1.json"),
        "{error}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_integrity_rejects_missing_authenticated_artifact() {
    let _lock = env_lock();
    let root = temp_root("missing");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    commit_verification_artifact(&run, "vp-1", br#"{"status":"pass"}"#);
    std::fs::remove_file(run.artifact_dir.join("verification/vp-1.json")).unwrap();

    let error = inspect_workflow_integrity(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("integrity_artifact_mismatch: verification/vp-1.json"),
        "{error}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_integrity_rejects_unbound_managed_artifact() {
    let _lock = env_lock();
    let root = temp_root("orphan");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    std::fs::write(run.artifact_dir.join("verification/orphan.json"), b"{}").unwrap();

    let error = inspect_workflow_integrity(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("integrity_artifact_mismatch: orphan:verification/orphan.json"),
        "{error}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_integrity_allows_mutable_active_recovery_journal_without_descriptor() {
    let _lock = env_lock();
    let root = temp_root("active-recovery-journal");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    let recovery_dir = run
        .artifact_dir
        .join("integrations/integration-test/recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();
    std::fs::write(recovery_dir.join("journal.json"), b"{}").unwrap();

    let report =
        serde_json::to_value(inspect_workflow_integrity(&run.artifact_dir).unwrap()).unwrap();

    assert_eq!(report["orphan_artifact_count"], 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workflow_integrity_requires_descriptor_for_recovery_history_archive() {
    let _lock = env_lock();
    let root = temp_root("recovery-history-orphan");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = signed_workflow(&root);
    let history_dir = run
        .artifact_dir
        .join("integrations/integration-test/recovery/history");
    std::fs::create_dir_all(&history_dir).unwrap();
    std::fs::write(history_dir.join("journal-1.json"), b"{}").unwrap();

    let error = inspect_workflow_integrity(&run.artifact_dir)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains(
            "integrity_artifact_mismatch: orphan:integrations/integration-test/recovery/history/journal-1.json"
        ),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unsigned_legacy_workflow_does_not_claim_artifact_verification() {
    let _lock = env_lock();
    let root = temp_root("legacy");
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "legacy workflow".to_string(),
            input_kind: WorkflowInputKind::Feature,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    std::fs::write(run.artifact_dir.join("verification/legacy.json"), b"{}").unwrap();

    let report =
        serde_json::to_value(inspect_workflow_integrity(&run.artifact_dir).unwrap()).unwrap();
    assert_eq!(report["status"], "unsigned_legacy");
    assert_eq!(report["artifact_descriptor_count"], 0);
    assert_eq!(report["verified_artifact_count"], 0);
    assert_eq!(report["orphan_artifact_count"], 0);

    let _ = std::fs::remove_dir_all(root);
}
