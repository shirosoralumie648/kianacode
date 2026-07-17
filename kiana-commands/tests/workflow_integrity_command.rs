use kiana_commands::tasks::TasksCommand;
use kiana_commands::{Command, CommandContext};
use kiana_tasks::{
    commit_immutable_artifacts_with_unique_event, WorkflowArtifactBatch, WorkflowArtifactInput,
    WorkflowEventKind,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

static ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct IntegrityEnv {
    previous_home: Option<OsString>,
    previous_key_file: Option<OsString>,
}

impl IntegrityEnv {
    fn install(home: &Path) -> Self {
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

impl Drop for IntegrityEnv {
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

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "kiana-workflow-integrity-command-{label}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        ROOT_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy()))]),
    }
}

async fn run(args: impl Into<String>, root: &Path) -> anyhow::Result<String> {
    Ok(TasksCommand.execute(context(args, root)).await?.value)
}

async fn create_workflow(root: &Path, request: &str) -> Value {
    let output = run(format!("workflow init --json {request}"), root)
        .await
        .unwrap();
    serde_json::from_str(&output).unwrap()
}

fn key_path(home: &Path) -> PathBuf {
    home.join("trust").join("workflow-integrity-key.json")
}

fn workflow_dir(root: &Path, run_id: &str) -> PathBuf {
    root.join(".kiana").join("workflows").join(run_id)
}

fn commit_verification_artifact(root: &Path, run_id: &str, packet_id: &str) {
    commit_immutable_artifacts_with_unique_event(
        workflow_dir(root, run_id),
        WorkflowEventKind::ArtifactWritten,
        "artifact_binding",
        "packet_id",
        packet_id,
        |_, _| {
            Ok(WorkflowArtifactBatch {
                artifacts: vec![WorkflowArtifactInput {
                    relative_path: format!("verification/{packet_id}.json"),
                    contents: br#"{"status":"pass"}"#.to_vec(),
                }],
                event_data: json!({"packet_id": packet_id}),
            })
        },
    )
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_init_is_idempotent_and_never_exposes_secret() {
    let _lock = env_lock();
    let root = temp_root("init");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);

    let first = run("workflow integrity init --json", &root).await.unwrap();
    let first_json: Value = serde_json::from_str(&first).unwrap();
    let second = run("workflow integrity init --json", &root).await.unwrap();
    let second_json: Value = serde_json::from_str(&second).unwrap();
    let human = run("workflow integrity status", &root).await.unwrap();
    let key_file: Value = serde_json::from_slice(&std::fs::read(key_path(&home)).unwrap()).unwrap();
    let secret = key_file["secret_hex"].as_str().unwrap();

    assert_eq!(
        first_json["schema"],
        "kiana.workflow-integrity-key-status.v1"
    );
    assert_eq!(first_json["configured"], true);
    assert_eq!(first_json["key_id"], second_json["key_id"]);
    for output in [&first, &second, &human] {
        assert!(!output.contains("secret_hex"));
        assert!(!output.contains(secret));
    }

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_status_reports_missing_key_without_creating_one() {
    let _lock = env_lock();
    let root = temp_root("missing-status");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);

    let output = run("workflow integrity status --json", &root)
        .await
        .unwrap();
    let status: Value = serde_json::from_str(&output).unwrap();

    assert_eq!(status["schema"], "kiana.workflow-integrity-key-status.v1");
    assert_eq!(status["configured"], false);
    assert_eq!(status["permission_state"], "missing");
    assert!(!key_path(&home).exists());

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_verify_uses_latest_run_and_reports_unsigned_legacy() {
    let _lock = env_lock();
    let root = temp_root("legacy-verify");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);
    let workflow = create_workflow(&root, "verify legacy workflow").await;

    let output = run("workflow integrity verify --json", &root)
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output).unwrap();

    assert_eq!(report["schema"], "kiana.workflow-integrity-report.v1");
    assert_eq!(report["workflow_id"], workflow["workflow_id"]);
    assert_eq!(report["status"], "unsigned_legacy");
    assert_eq!(report["release_binding"]["workflow"]["status"], "running");
    assert_eq!(
        report["release_binding"]["workflow"]["current_node"],
        "capture"
    );
    assert_eq!(
        report["release_binding"]["workflow"]["last_event_kind"],
        "node_entered"
    );
    assert!(
        report["release_binding"]["workflow"]["last_event_seq"]
            .as_u64()
            .unwrap()
            > 0
    );
    for field in ["state_sha256", "eventlog_sha256"] {
        let value = report["release_binding"]["workflow"][field]
            .as_str()
            .unwrap();
        assert_eq!(value.len(), 64);
        assert!(value.chars().all(|character| character.is_ascii_hexdigit()));
    }
    assert!(report["event_count"].as_u64().unwrap() > 0);
    assert_eq!(report["verified_event_count"], 0);
    assert_eq!(
        report["recovery_integrity"]["schema"],
        "kiana.swarm-recovery-integrity-report.v1"
    );
    assert_eq!(report["recovery_integrity"]["status"], "verified");
    assert_eq!(report["recovery_integrity"]["active_journal_count"], 0);
    assert_eq!(report["recovery_integrity"]["archived_journal_count"], 0);

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_seal_authenticates_legacy_prefix_and_is_idempotent() {
    let _lock = env_lock();
    let root = temp_root("seal");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);
    let workflow = create_workflow(&root, "seal legacy workflow").await;
    let run_id = workflow["run_id"].as_str().unwrap();
    run("workflow integrity init --json", &root).await.unwrap();

    let first = run(format!("workflow integrity seal --json {run_id}"), &root)
        .await
        .unwrap();
    let second = run(format!("workflow integrity seal --json {run_id}"), &root)
        .await
        .unwrap();
    let first: Value = serde_json::from_str(&first).unwrap();
    let second: Value = serde_json::from_str(&second).unwrap();
    let verify = run(format!("workflow integrity verify --json {run_id}"), &root)
        .await
        .unwrap();
    let verify: Value = serde_json::from_str(&verify).unwrap();

    assert_eq!(first["schema"], "kiana.workflow-integrity-seal-result.v1");
    assert_eq!(first["workflow_id"], workflow["workflow_id"]);
    assert_eq!(first["seal"], second["seal"]);
    assert_eq!(verify["status"], "sealed_legacy_prefix");
    assert_eq!(verify["genesis_seal_path"], "integrity/genesis-seal.json");
    assert!(workflow_dir(&root, run_id)
        .join("integrity/genesis-seal.json")
        .is_file());

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_verify_reports_signed_run_with_missing_key_as_unverifiable() {
    let _lock = env_lock();
    let root = temp_root("missing-key-verify");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);
    run("workflow integrity init --json", &root).await.unwrap();
    let workflow = create_workflow(&root, "signed workflow").await;
    let run_id = workflow["run_id"].as_str().unwrap();
    std::fs::remove_file(key_path(&home)).unwrap();

    let output = run(format!("workflow integrity verify --json {run_id}"), &root)
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output).unwrap();

    assert_eq!(report["status"], "unverifiable_key_missing");
    assert!(report["verified_event_count"].as_u64().unwrap() > 0);
    assert!(report["key_id"].as_str().is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_verify_rejects_tampered_signed_event() {
    let _lock = env_lock();
    let root = temp_root("tampered");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);
    run("workflow integrity init --json", &root).await.unwrap();
    let workflow = create_workflow(&root, "tampered signed workflow").await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let eventlog = workflow_dir(&root, run_id).join("eventlog.jsonl");
    let contents = std::fs::read_to_string(&eventlog).unwrap();
    let mut records = contents
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    records[0]["data"]["tampered"] = json!(true);
    let tampered = records
        .iter()
        .map(|record| serde_json::to_string(record).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&eventlog, format!("{tampered}\n")).unwrap();

    let error = run(format!("workflow integrity verify --json {run_id}"), &root)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("integrity_payload_mismatch"), "{error}");

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_commands_reject_unknown_options_with_usage() {
    let _lock = env_lock();
    let root = temp_root("unknown-option");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);

    let error = run("workflow integrity verify --wat", &root)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("unknown workflow integrity verify option '--wat'"));
    assert!(error.contains("workflow integrity verify"));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "current_thread")]
async fn integrity_verify_reports_artifact_counts_and_rejects_mutation() {
    let _lock = env_lock();
    let root = temp_root("artifact-counts");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);
    run("workflow integrity init --json", &root).await.unwrap();
    let workflow = create_workflow(&root, "artifact integrity workflow").await;
    let run_id = workflow["run_id"].as_str().unwrap();
    commit_verification_artifact(&root, run_id, "vp-1");

    let json_output = run(format!("workflow integrity verify --json {run_id}"), &root)
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&json_output).unwrap();
    assert_eq!(report["run_id"], run_id);
    assert_eq!(
        report["release_binding"]["schema"],
        "kiana.workflow-release-binding.v1"
    );
    assert_eq!(report["release_binding"]["run_id"], run_id);
    assert_eq!(
        report["release_binding"]["workflow_id"],
        report["workflow_id"]
    );
    assert_eq!(report["release_binding"]["git"]["repository"], false);
    assert_eq!(report["release_binding"]["git"]["head"], "none");
    for field in ["index_diff_sha256", "worktree_diff_sha256", "status_sha256"] {
        assert_eq!(
            report["release_binding"]["git"][field]
                .as_str()
                .unwrap()
                .len(),
            64
        );
    }
    assert_eq!(report["artifact_descriptor_count"], 1);
    assert_eq!(report["verified_artifact_count"], 1);
    assert_eq!(report["orphan_artifact_count"], 0);

    let human = run(format!("workflow integrity verify {run_id}"), &root)
        .await
        .unwrap();
    assert!(human.contains("artifact_descriptors: 1"));
    assert!(human.contains("verified_artifacts: 1"));
    assert!(human.contains("orphan_artifacts: 0"));
    assert!(human.contains("recovery_active: 0"));
    assert!(human.contains("recovery_active_verified: 0"));
    assert!(human.contains("recovery_archived: 0"));
    assert!(human.contains("recovery_archived_verified: 0"));
    assert!(human.contains("release_git_head: none"));

    std::fs::write(
        workflow_dir(&root, run_id).join("verification/vp-1.json"),
        br#"{"status":"forged"}"#,
    )
    .unwrap();
    let error = run(format!("workflow integrity verify --json {run_id}"), &root)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("integrity_artifact_mismatch"), "{error}");

    let _ = std::fs::remove_dir_all(root);
}
