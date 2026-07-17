use kiana_commands::audit::AuditCommand;
use kiana_commands::{Command, CommandContext};
use kiana_tasks::{
    commit_immutable_artifacts_with_unique_event, initialize_local_hmac_key,
    initialize_workflow_run, WorkflowArtifactBatch, WorkflowArtifactInput, WorkflowEventKind,
    WorkflowInit, WorkflowInputKind, WorkflowProfile,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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

fn root() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-workflow-integrity-audit-{}-{unique}",
        std::process::id()
    ))
}

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy()))]),
    }
}

#[tokio::test]
async fn strict_audit_blocks_authenticated_artifact_mismatch() {
    let _lock = env_lock();
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "audit artifact integrity".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    commit_immutable_artifacts_with_unique_event(
        &run.artifact_dir,
        WorkflowEventKind::ArtifactWritten,
        "artifact_binding",
        "packet_id",
        "vp-1",
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: "verification/vp-1.json".to_string(),
                    contents: br#"{"status":"pass"}"#.to_vec(),
                }],
                event_data: json!({"packet_id": "vp-1"}),
            })
        },
    )
    .unwrap();
    std::fs::write(
        run.artifact_dir.join("verification/vp-1.json"),
        br#"{"status":"forged"}"#,
    )
    .unwrap();

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| {
            finding["category"] == "workflow_integrity_artifact_mismatch"
                && finding["severity"] == "block"
                && finding["next_action"] == "restore_or_regenerate_authenticated_artifact"
        }));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn strict_audit_blocks_unverifiable_swarm_recovery_journal() {
    let _lock = env_lock();
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "audit recovery integrity".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let recovery_dir = run
        .artifact_dir
        .join("integrations/integration-test/recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();
    std::fs::write(recovery_dir.join("journal.json"), br#"{"schema":"legacy"}"#).unwrap();

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| {
            finding["category"] == "workflow_recovery_integrity"
                && finding["severity"] == "block"
                && finding["next_action"] == "resume_or_restore_authenticated_recovery_journal"
        }));

    let _ = std::fs::remove_dir_all(root);
}
