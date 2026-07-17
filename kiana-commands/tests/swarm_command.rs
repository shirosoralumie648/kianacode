use kiana_commands::tasks::TasksCommand;
use kiana_commands::{Command, CommandContext};
use kiana_tasks::{
    append_workflow_event_with_data, initialize_local_hmac_key_at, WorkflowEventKind,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);
static SWARM_RUNNER_ENV_LOCK: Mutex<()> = Mutex::new(());
static SWARM_WORKFLOW_INTEGRITY_KEY: OnceLock<PathBuf> = OnceLock::new();

fn root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "kiana-swarm-command-{}-{}-{}",
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
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]),
    }
}

#[derive(Serialize)]
struct TestGitStateSnapshot {
    repository: bool,
    head: String,
    index_diff_sha256: String,
    worktree_diff_sha256: String,
    status_sha256: String,
}

fn test_sha256_bytes(value: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(value);
    format!("{:x}", digest.finalize())
}

fn test_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(target_os = "linux")]
fn test_metadata_descriptor(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path).unwrap();
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let length = unsafe { libc::llistxattr(path.as_ptr(), std::ptr::null_mut(), 0) };
    assert!(length >= 0, "{}", std::io::Error::last_os_error());
    let mut names = vec![0u8; length as usize];
    if length > 0 {
        let read =
            unsafe { libc::llistxattr(path.as_ptr(), names.as_mut_ptr().cast(), names.len()) };
        assert!(read >= 0, "{}", std::io::Error::last_os_error());
        names.truncate(read as usize);
    }
    let mut xattrs = Vec::new();
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name_c = std::ffi::CString::new(name).unwrap();
        let value_length =
            unsafe { libc::lgetxattr(path.as_ptr(), name_c.as_ptr(), std::ptr::null_mut(), 0) };
        assert!(value_length >= 0, "{}", std::io::Error::last_os_error());
        let mut value = vec![0u8; value_length as usize];
        if value_length > 0 {
            let read = unsafe {
                libc::lgetxattr(
                    path.as_ptr(),
                    name_c.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            assert!(read >= 0, "{}", std::io::Error::last_os_error());
            value.truncate(read as usize);
        }
        xattrs.push((test_hex(name), test_hex(&value)));
    }
    xattrs.sort();
    let mut payload = Vec::new();
    for (name, value) in xattrs {
        payload.extend_from_slice(name.as_bytes());
        payload.push(0);
        payload.extend_from_slice(value.as_bytes());
        payload.push(0xff);
    }
    format!(
        "uid={}:gid={}:xattrs_sha256={}",
        metadata.uid(),
        metadata.gid(),
        test_sha256_bytes(&payload)
    )
}

#[cfg(not(target_os = "linux"))]
fn test_metadata_descriptor(_path: &Path) -> String {
    format!("uid=0:gid=0:xattrs_sha256={}", test_sha256_bytes(&[]))
}

#[cfg(unix)]
fn test_file_mode(path: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;

    format!(
        "{:04o}",
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    )
}

#[cfg(target_os = "linux")]
fn test_checkpoint_metadata_value(path: &Path) -> Value {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path).unwrap();
    json!({
        "uid": metadata.uid(),
        "gid": metadata.gid(),
        "xattrs": []
    })
}

#[cfg(not(target_os = "linux"))]
fn test_checkpoint_metadata_value(_path: &Path) -> Value {
    json!({"uid": 0, "gid": 0, "xattrs": []})
}

fn test_working_tree_fingerprint(root: &Path) -> String {
    fn collect(root: &Path, current: &Path, manifest: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if matches!(
                name.to_string_lossy().as_ref(),
                ".git" | ".kiana" | "target" | "node_modules" | ".venv" | "dist" | "build"
            ) {
                continue;
            }
            let path = entry.path();
            let file_type = entry.file_type().unwrap();
            if file_type.is_dir() {
                collect(root, &path, manifest);
            } else if file_type.is_file() {
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    format!(
                        "{:04o}",
                        std::fs::symlink_metadata(&path)
                            .unwrap()
                            .permissions()
                            .mode()
                            & 0o7777
                    )
                };
                #[cfg(not(unix))]
                let mode = if std::fs::symlink_metadata(&path)
                    .unwrap()
                    .permissions()
                    .readonly()
                {
                    "readonly".to_string()
                } else {
                    "writable".to_string()
                };
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                manifest.insert(
                    relative,
                    format!(
                        "file:mode={mode}:{}:sha256:{}",
                        test_metadata_descriptor(&path),
                        test_sha256_bytes(&std::fs::read(&path).unwrap())
                    ),
                );
            } else if file_type.is_symlink() {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                manifest.insert(
                    relative,
                    format!(
                        "symlink:{}:target_sha256:{}",
                        test_metadata_descriptor(&path),
                        test_sha256_bytes(
                            std::fs::read_link(&path)
                                .unwrap()
                                .to_string_lossy()
                                .as_bytes()
                        )
                    ),
                );
            }
        }
    }

    let absent = test_sha256_bytes(b"not-a-git-repository");
    let git_state = TestGitStateSnapshot {
        repository: false,
        head: "none".to_string(),
        index_diff_sha256: absent.clone(),
        worktree_diff_sha256: absent.clone(),
        status_sha256: absent,
    };
    let mut manifest = BTreeMap::new();
    collect(root, root, &mut manifest);
    format!(
        "sha256:{}",
        test_sha256_bytes(&serde_json::to_vec(&(git_state, manifest)).unwrap())
    )
}

fn append_test_workflow_event(artifact_dir: &Path, kind: &str, node_id: &str, mut data: Value) {
    let kind: WorkflowEventKind = serde_json::from_value(json!(kind)).unwrap();
    append_workflow_event_with_data(artifact_dir, kind, node_id, move |sequence, at_ms| {
        data["sequence"] = json!(sequence);
        data["created_at_ms"] = json!(at_ms);
        data
    })
    .unwrap();
}

fn append_test_recovery_anchor_events(
    artifact_dir: &Path,
    plan: &Value,
    dispatch_id: &str,
    checkpoint_sha256: &str,
    applied_fingerprint: &str,
) {
    let integration_id = plan["integration_id"].as_str().unwrap();
    append_test_workflow_event(
        artifact_dir,
        "swarm_integration_started",
        "swarm_integration",
        json!({
            "integration_event_id": format!("{integration_id}:applying"),
            "integration_id": integration_id,
            "dispatch_id": dispatch_id,
            "status": "applying",
            "packet_path": null,
            "plan_sha256": plan["plan_sha256"],
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": checkpoint_sha256,
            "working_tree_fingerprint": plan["baseline"]["working_tree_fingerprint"],
        }),
    );
    append_test_workflow_event(
        artifact_dir,
        "swarm_worker_applied",
        "swarm_integration",
        json!({
            "worker_apply_id": format!("{integration_id}:api:applied"),
            "integration_id": integration_id,
            "dispatch_id": dispatch_id,
            "task_id": "api",
            "plan_sha256": plan["plan_sha256"],
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": checkpoint_sha256,
            "working_tree_fingerprint": applied_fingerprint,
            "applied_task_ids": ["api"],
        }),
    );
}

fn write_task(root: &Path, id: &str, priority: i64, allowed_paths: &[&str]) {
    write_task_with_metadata(root, id, priority, allowed_paths, json!({}));
}

fn write_task_with_metadata(
    root: &Path,
    id: &str,
    priority: i64,
    allowed_paths: &[&str],
    extra_metadata: Value,
) {
    let dir = root.join(".kiana/tasks/default");
    std::fs::create_dir_all(&dir).unwrap();
    let mut metadata = serde_json::Map::from_iter([
        ("priority".to_string(), json!(priority)),
        (
            "verification_commands".to_string(),
            json!([format!("verify-{id}")]),
        ),
        ("allowed_paths".to_string(), json!(allowed_paths)),
    ]);
    for (key, value) in extra_metadata.as_object().cloned().unwrap_or_default() {
        metadata.insert(key, value);
    }
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_vec_pretty(&json!({
            "id": id,
            "title": format!("Task {id}"),
            "status": "pending",
            "blockedBy": [],
            "blocks": [],
            "created_at": priority,
            "updated_at": priority,
            "metadata": metadata
        }))
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn swarm_plan_json_selects_independent_tasks_and_skips_conflicts() {
    let root = root();
    write_task(&root, "api", 30, &["src/api/**"]);
    write_task(&root, "docs", 20, &["docs/**"]);
    write_task(&root, "client", 10, &["src/api/client.rs"]);

    let result = TasksCommand
        .execute(context("swarm plan --json --max-workers 3", &root))
        .await
        .unwrap();
    let plan: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(plan["schema"], "kiana.swarm-plan.v1");
    assert_eq!(plan["execution_mode"], "plan_only");
    assert_eq!(plan["status"], "ready");
    assert_eq!(plan["summary"]["ready_tasks"], 3);
    assert_eq!(plan["summary"]["dispatched"], 2);
    assert_eq!(plan["assignments"][0]["task_id"], "api");
    assert_eq!(plan["assignments"][1]["task_id"], "docs");
    assert_eq!(plan["skipped"][0]["task_id"], "client");
    assert_eq!(plan["skipped"][0]["reason"], "path_conflict");
    assert_eq!(
        plan["assignments"][0]["workpacket"]["verification_commands"][0],
        "verify-api"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn swarm_plan_rejects_invalid_or_duplicate_options() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();

    for (args, expected) in [
        ("swarm plan --max-workers 0", "between 2 and 32"),
        ("swarm plan --max-workers 1", "between 2 and 32"),
        ("swarm plan --max-workers 33", "between 2 and 32"),
        (
            "swarm plan --max-workers 2 --max-workers 3",
            "duplicate option --max-workers",
        ),
        ("swarm plan --unknown", "unknown swarm plan option"),
    ] {
        let error = TasksCommand
            .execute(context(args, &root))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "args={args} error={error}");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn swarm_plan_human_output_lists_assignments_skips_and_locks() {
    let root = root();
    write_task(&root, "api", 30, &["src/api/**"]);
    write_task(&root, "docs", 20, &["docs/**"]);
    write_task(&root, "client", 10, &["src/api/client.rs"]);

    let result = TasksCommand
        .execute(context("swarm plan --max-workers 3", &root))
        .await
        .unwrap();

    assert!(result.value.contains("api"));
    assert!(result.value.contains("docs"));
    assert!(result.value.contains("client"));
    assert!(result.value.contains("path_conflict"));
    assert!(result.value.contains("src/api"));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn swarm_plan_denies_unapproved_and_restricted_tasks() {
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    write_task_with_metadata(
        &root,
        "approval",
        40,
        &["release/approval/**"],
        json!({"approval_required": true}),
    );
    write_task_with_metadata(
        &root,
        "release",
        30,
        &["release/artifacts/**"],
        json!({"task_type": "release", "risk_level": "high"}),
    );
    write_task_with_metadata(
        &root,
        "risky",
        25,
        &["risky/**"],
        json!({"task_type": "implementation", "risk_level": "high"}),
    );

    let result = TasksCommand
        .execute(context("swarm plan --json --max-workers 4", &root))
        .await
        .unwrap();
    let plan: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(plan["summary"]["dispatched"], 2);
    assert_eq!(plan["assignments"][0]["task_id"], "api");
    assert_eq!(plan["assignments"][1]["task_id"], "docs");
    assert_eq!(
        plan["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["task_id"] == "approval")
            .unwrap()["reason"],
        "approval_required"
    );
    assert_eq!(
        plan["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["task_id"] == "release")
            .unwrap()["reason"],
        "restricted_task_type"
    );
    assert_eq!(
        plan["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["task_id"] == "risky")
            .unwrap()["reason"],
        "high_risk_task"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn swarm_plan_allows_recorded_approval_and_preserves_requirement_in_preview() {
    let root = root();
    write_task_with_metadata(
        &root,
        "approved",
        20,
        &["src/approved/**"],
        json!({"approval_required": true, "approval_status": "approved"}),
    );
    write_task(&root, "docs", 10, &["docs/**"]);

    let result = TasksCommand
        .execute(context("swarm plan --json --max-workers 2", &root))
        .await
        .unwrap();
    let plan: Value = serde_json::from_str(&result.value).unwrap();
    let approved = plan["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["task_id"] == "approved")
        .unwrap();

    assert_eq!(
        approved["workpacket"]["schema"],
        "kiana.swarm-workpacket-preview.v1"
    );
    assert_eq!(approved["workpacket"]["approval_required"], true);

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_plan_resolves_symlink_aliases_before_conflict_detection() {
    use std::os::unix::fs::symlink;

    let root = root();
    std::fs::create_dir_all(root.join("src/real")).unwrap();
    symlink(root.join("src/real"), root.join("src/alias")).unwrap();
    write_task(&root, "alias", 30, &["src/alias/file.rs"]);
    write_task(&root, "docs", 20, &["docs/**"]);
    write_task(&root, "real", 10, &["src/real/file.rs"]);

    let result = TasksCommand
        .execute(context("swarm plan --json --max-workers 3", &root))
        .await
        .unwrap();
    let plan: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(plan["summary"]["dispatched"], 2);
    let skipped = plan["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["task_id"] == "real")
        .unwrap();
    assert_eq!(skipped["reason"], "path_conflict");
    assert_eq!(skipped["conflicts_with"][0], "alias");

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_plan_rejects_symlink_scopes_that_escape_project_root() {
    use std::os::unix::fs::symlink;

    let root = root();
    let outside = root.with_extension("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    symlink(&outside, root.join("external")).unwrap();
    write_task(&root, "escape", 30, &["external/file.rs"]);
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);

    let result = TasksCommand
        .execute(context("swarm plan --json --max-workers 3", &root))
        .await
        .unwrap();
    let plan: Value = serde_json::from_str(&result.value).unwrap();

    assert_eq!(plan["summary"]["dispatched"], 2);
    let skipped = plan["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["task_id"] == "escape")
        .unwrap();
    assert_eq!(skipped["reason"], "invalid_allowed_path");

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

async fn init_workflow(project_root: &Path) -> Value {
    let key_path = SWARM_WORKFLOW_INTEGRITY_KEY.get_or_init(|| {
        let key_path = root().join("trust/workflow-integrity-key.json");
        initialize_local_hmac_key_at(&key_path).unwrap();
        key_path
    });
    std::env::set_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", key_path);
    let result = TasksCommand
        .execute(context(
            "workflow init --json --type feature --profile gated Implement bounded swarm dispatch",
            project_root,
        ))
        .await
        .unwrap();
    serde_json::from_str(&result.value).unwrap()
}

#[tokio::test]
async fn swarm_dispatch_persists_packets_and_is_idempotent() {
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let eventlog = artifact_dir.join("eventlog.jsonl");
    let initial_event_count = std::fs::read_to_string(&eventlog).unwrap().lines().count();

    let args = format!(
        "swarm dispatch --json --workflow {run_id} --max-workers 2 --max-attempts 2 --max-commands 12 --timeout-seconds 600 --max-output-bytes 4096"
    );
    let first = TasksCommand.execute(context(&args, &root)).await.unwrap();
    let second = TasksCommand.execute(context(&args, &root)).await.unwrap();
    let first_json: Value = serde_json::from_str(&first.value).unwrap();
    let second_json: Value = serde_json::from_str(&second.value).unwrap();

    assert_eq!(first_json["schema"], "kiana.swarm-dispatch-result.v1");
    assert_eq!(first_json["execution_mode"], "persist_only");
    assert_eq!(first_json["status"], "persisted");
    assert_eq!(first_json["next_action"], "run_swarm_start");
    assert_eq!(first_json["dispatch_id"], second_json["dispatch_id"]);
    assert_eq!(first_json["reused_event"], false);
    assert_eq!(second_json["reused_event"], true);
    assert_eq!(first_json["event_seq"], second_json["event_seq"]);
    assert!(!first.value.contains(root.to_string_lossy().as_ref()));

    let manifest_path = first_json["manifest_path"].as_str().unwrap();
    assert!(artifact_dir.join(manifest_path).is_file());
    for path in first_json["workpacket_paths"].as_array().unwrap() {
        assert!(artifact_dir.join(path.as_str().unwrap()).is_file());
    }
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(artifact_dir.join(manifest_path)).unwrap())
            .unwrap();
    assert_eq!(manifest["budget"]["max_attempts"], 2);
    assert_eq!(manifest["budget"]["max_commands"], 12);
    assert_eq!(manifest["budget"]["timeout_seconds"], 600);
    assert_eq!(manifest["budget"]["max_output_bytes"], 4096);
    assert_eq!(manifest["next_action"], "run_swarm_start");
    assert_eq!(
        std::fs::read_to_string(&eventlog).unwrap().lines().count(),
        initial_event_count + 1
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn swarm_dispatch_rejects_missing_duplicate_and_invalid_options() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();

    for (args, expected) in [
        ("swarm dispatch --json", "missing option --workflow"),
        (
            "swarm dispatch --workflow one --workflow two",
            "duplicate option --workflow",
        ),
        (
            "swarm dispatch --workflow run --max-attempts 0",
            "max-attempts must be between 1 and 3",
        ),
        (
            "swarm dispatch --workflow run --max-commands 101",
            "max-commands must be between 1 and 100",
        ),
        (
            "swarm dispatch --workflow run --timeout-seconds 0",
            "timeout-seconds must be between 1 and 86400",
        ),
        (
            "swarm dispatch --workflow run --max-output-bytes 1023",
            "max-output-bytes must be between 1024 and 104857600",
        ),
    ] {
        let error = TasksCommand
            .execute(context(args, &root))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "args={args} error={error}");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn swarm_dispatch_rejects_inconsistent_workflow_without_artifacts() {
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let state_path = artifact_dir.join("state.json");
    let mut state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
    state["last_event_seq"] = json!(99);
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let error = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("state last_event_seq 99 does not match eventlog"));
    assert_eq!(
        std::fs::read_dir(artifact_dir.join("workpackets"))
            .unwrap()
            .count(),
        0
    );

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
fn write_fixture_runner(root: &Path) -> PathBuf {
    write_fixture_runner_with_body(
        root,
        "printf 'worker=%s task=%s\\n' \"$KIANA_SWARM_WORKER_ID\" \"$KIANA_SWARM_TASK_ID\"\nsleep 0.6",
    )
}

#[cfg(unix)]
fn write_fixture_runner_with_body(root: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("fixture-worker.sh");
    std::fs::write(&path, format!("#!/usr/bin/env bash\nset -eu\n{body}\n")).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

#[cfg(unix)]
async fn prepare_ready_swarm_integration_plan(root: &Path) -> (String, String, PathBuf) {
    let workflow = init_workflow(root).await;
    let run_id = workflow["run_id"].as_str().unwrap().to_string();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap().to_string();
    let runner = write_fixture_runner_with_body(
        root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\n// candidate\n' >> src/api/mod.rs; else printf '\ncandidate\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            root,
        ))
        .await
        .unwrap();
    let mut terminal = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let monitored = TasksCommand
            .execute(context(
                format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
                root,
            ))
            .await
            .unwrap();
        let monitored: Value = serde_json::from_str(&monitored.value).unwrap();
        if monitored["status"] == "terminal" {
            terminal = true;
            break;
        }
    }
    assert!(terminal, "workers did not reach terminal state");
    let planned = TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            root,
        ))
        .await
        .unwrap();
    let planned: Value = serde_json::from_str(&planned.value).unwrap();
    assert_eq!(planned["status"], "ready");
    (run_id, dispatch_id, artifact_dir)
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_start_launches_isolated_workers_and_status_recovers_them() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner(&root);
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);

    let first = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let first: Value = serde_json::from_str(&first.value).unwrap();
    assert_eq!(first["schema"], "kiana.swarm-start-result.v1");
    assert_eq!(first["dispatch_id"], dispatch_id);
    assert_eq!(first["status"], "running");
    assert_eq!(first["spawned"], 2);
    assert_eq!(first["reused"], 0);
    assert_eq!(first["workers"].as_array().unwrap().len(), 2);
    assert!(first["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["pid"].as_u64().unwrap() > 0));
    assert_ne!(first["workers"][0]["pid"], first["workers"][1]["pid"]);
    assert!(first["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["isolation_strategy"] == "snapshot_copy"));
    for worker in first["workers"].as_array().unwrap() {
        let state_path = artifact_dir.join(worker["state_path"].as_str().unwrap());
        let state: Value =
            serde_json::from_str(&std::fs::read_to_string(state_path).unwrap()).unwrap();
        assert_eq!(state["schema"], "kiana.swarm-worker-state.v2");
        assert_eq!(state["process_identity_status"], "verified");
        assert_eq!(
            state["process_identity"]["schema"],
            "kiana.swarm-process-identity.v1"
        );
        assert_eq!(state["process_identity"]["platform"], "linux_procfs");
        assert_eq!(state["process_identity"]["pid"], state["pid"]);
        assert!(state["process_identity"]["process_group_id"]
            .as_u64()
            .is_some_and(|value| value > 0));
        assert!(state["process_identity"]["start_time_ticks"]
            .as_u64()
            .is_some_and(|value| value > 0));
        assert!(state["process_identity"]["command_sha256"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert!(!serde_json::to_string(&state)
            .unwrap()
            .contains(root.to_string_lossy().as_ref()));
    }

    let second = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let second: Value = serde_json::from_str(&second.value).unwrap();
    assert_eq!(second["spawned"], 0);
    assert_eq!(second["reused"], 2);
    assert_eq!(second["workers"][0]["pid"], first["workers"][0]["pid"]);
    assert_eq!(second["workers"][1]["pid"], first["workers"][1]["pid"]);

    let status = TasksCommand
        .execute(context(
            format!("swarm status --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let status: Value = serde_json::from_str(&status.value).unwrap();
    assert_eq!(status["schema"], "kiana.swarm-status.v1");
    assert_eq!(status["dispatch_id"], dispatch_id);
    assert_eq!(
        status["process_identity_backend"]["schema"],
        "kiana.swarm-process-identity-backend.v1"
    );
    assert_eq!(
        status["process_identity_backend"]["backend"],
        "linux_procfs"
    );
    assert_eq!(status["process_identity_backend"]["supported"], true);
    assert_eq!(
        status["process_identity_backend"]["safe_to_start_workers"],
        true
    );
    assert_eq!(
        status["process_identity_backend"]["safe_to_monitor_workers"],
        true
    );
    assert_eq!(
        status["process_identity_backend"]["safe_to_cancel_workers"],
        true
    );
    assert_eq!(status["workers"].as_array().unwrap().len(), 2);
    assert!(status["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| matches!(worker["status"].as_str(), Some("running" | "completed"))));
    assert!(status["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["state_path"]
            .as_str()
            .unwrap()
            .starts_with("workers/")));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    tokio::time::sleep(Duration::from_millis(800)).await;
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn swarm_status_marks_reused_pid_as_lost() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(&root, "sleep 5");
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    let target = &started["workers"][0];
    let target_pid = target["pid"].as_u64().unwrap();
    let state_path = artifact_dir.join(target["state_path"].as_str().unwrap());
    let original_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
    let mut tampered_state = original_state.clone();
    tampered_state["process_identity"]["start_time_ticks"] = json!(
        original_state["process_identity"]["start_time_ticks"]
            .as_u64()
            .unwrap()
            + 1
    );
    std::fs::write(
        &state_path,
        serde_json::to_vec_pretty(&tampered_state).unwrap(),
    )
    .unwrap();

    let status = TasksCommand
        .execute(context(
            format!("swarm status --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let status: Value = serde_json::from_str(&status.value).unwrap();
    let target_status = status["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["pid"] == target_pid)
        .unwrap()
        .clone();
    let target_still_alive = ProcessCommand::new("kill")
        .args(["-0", &target_pid.to_string()])
        .status()
        .unwrap()
        .success();

    std::fs::write(
        &state_path,
        serde_json::to_vec_pretty(&original_state).unwrap(),
    )
    .unwrap();
    let _ = TasksCommand
        .execute(context(
            format!("swarm cancel --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await;
    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(target_status["status"], "lost");
    assert_eq!(target_status["process_identity_status"], "mismatch");
    assert!(target_status["process_identity_reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("start_time_ticks")));
    assert!(target_still_alive, "status must not signal the worker");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn legacy_running_worker_state_without_identity_fails_closed() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(&root, "sleep 5");
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    let target = started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();
    let target_pid = target["pid"].as_u64().unwrap();
    let state_path = artifact_dir.join(target["state_path"].as_str().unwrap());
    let mut legacy_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
    legacy_state["schema"] = json!("kiana.swarm-worker-state.v1");
    legacy_state
        .as_object_mut()
        .unwrap()
        .remove("process_identity");
    legacy_state
        .as_object_mut()
        .unwrap()
        .remove("process_identity_status");
    legacy_state
        .as_object_mut()
        .unwrap()
        .remove("process_identity_reason");
    std::fs::write(
        &state_path,
        serde_json::to_vec_pretty(&legacy_state).unwrap(),
    )
    .unwrap();

    let status = TasksCommand
        .execute(context(
            format!("swarm status --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let status: Value = serde_json::from_str(&status.value).unwrap();
    let target_status = status["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap()
        .clone();
    let cancel = TasksCommand
        .execute(context(
            format!("swarm cancel --json --workflow {run_id} --dispatch {dispatch_id} --task api"),
            &root,
        ))
        .await;
    let cancel_error = cancel.as_ref().err().map(ToString::to_string);
    let target_alive_after = ProcessCommand::new("kill")
        .args(["-0", &target_pid.to_string()])
        .status()
        .unwrap()
        .success();

    for worker in started["workers"].as_array().unwrap() {
        let pid = worker["pid"].as_u64().unwrap();
        let _ = ProcessCommand::new("kill")
            .args(["-TERM", "--", &format!("-{pid}")])
            .status();
    }
    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(target_status["status"], "lost");
    assert_eq!(target_status["process_identity_status"], "missing");
    assert!(cancel_error
        .as_deref()
        .is_some_and(|error| error.contains("process_identity_missing")));
    assert!(
        target_alive_after,
        "legacy state must not authorize signalling"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn swarm_start_rejects_symlinked_worker_output_files() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let outside = root.with_extension("outside-worker-output");
    std::fs::write(&outside, "sentinel\n").unwrap();
    let worker_dir = artifact_dir.join("workers").join(dispatch_id).join("api");
    std::fs::create_dir_all(&worker_dir).unwrap();
    std::os::unix::fs::symlink(&outside, worker_dir.join("pid")).unwrap();
    let runner = write_fixture_runner_with_body(&root, "true");
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);

    let result = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let error = result.err().map(|error| error.to_string());
    let outside_contents = std::fs::read_to_string(&outside).unwrap();

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_file(&outside);

    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("symlink")),
        "unexpected start result: {error:?}"
    );
    assert_eq!(outside_contents, "sentinel\n");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn swarm_worker_sandbox_blocks_relative_escape_into_project_root() {
    if !ProcessCommand::new("bwrap")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "printf 'escape\\n' > ../../../../escaped.txt 2>/dev/null || true\nif [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\\n// allowed\\n' >> src/api/mod.rs; else printf '\\nallowed\\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let mut monitored = None;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let result = TasksCommand
            .execute(context(
                format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
                &root,
            ))
            .await
            .unwrap();
        let result: Value = serde_json::from_str(&result.value).unwrap();
        if result["status"] == "terminal" {
            monitored = Some(result);
            break;
        }
    }
    let monitored = monitored.expect("sandbox workers did not reach terminal state");

    assert!(!root.join("escaped.txt").exists());
    assert!(monitored["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["status"] == "completed"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_start_uses_detached_worktrees_for_clean_git_projects() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    assert!(ProcessCommand::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    assert!(ProcessCommand::new("git")
        .args(["add", "src", "docs"])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    assert!(ProcessCommand::new("git")
        .args([
            "-c",
            "user.name=Kiana Test",
            "-c",
            "user.email=kiana@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner_root = root.with_extension("runner");
    std::fs::create_dir_all(&runner_root).unwrap();
    let runner = write_fixture_runner(&runner_root);
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);

    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    assert!(started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["isolation_strategy"] == "git_worktree"));
    for worker in started["workers"].as_array().unwrap() {
        let isolation = root.join(worker["isolation_path"].as_str().unwrap());
        assert!(isolation.join(".git").exists());
    }

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    tokio::time::sleep(Duration::from_millis(800)).await;
    for worker in started["workers"].as_array().unwrap() {
        let isolation = root.join(worker["isolation_path"].as_str().unwrap());
        let _ = ProcessCommand::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&isolation)
            .current_dir(&root)
            .status();
    }
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(runner_root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_cancel_stops_one_worker_without_stopping_its_peer() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(&root, "sleep 5");
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    let docs_pid = started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "docs")
        .unwrap()["pid"]
        .as_u64()
        .unwrap();

    let cancelled = TasksCommand
        .execute(context(
            format!("swarm cancel --json --workflow {run_id} --dispatch {dispatch_id} --task api"),
            &root,
        ))
        .await
        .unwrap();
    let cancelled: Value = serde_json::from_str(&cancelled.value).unwrap();
    assert_eq!(cancelled["schema"], "kiana.swarm-cancel-result.v1");
    assert_eq!(cancelled["cancelled"], 1);
    assert_eq!(cancelled["workers"][0]["task_id"], "api");
    assert_eq!(cancelled["workers"][0]["status"], "cancelled");

    let status = TasksCommand
        .execute(context(
            format!("swarm status --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let status: Value = serde_json::from_str(&status.value).unwrap();
    let api = status["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();
    let docs = status["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "docs")
        .unwrap();
    assert_eq!(api["status"], "cancelled");
    assert_eq!(docs["status"], "running");
    assert_eq!(docs["pid"], docs_pid);

    let _ = TasksCommand
        .execute(context(
            format!("swarm cancel --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn swarm_cancel_refuses_foreign_process_identity_without_signaling() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(&root, "sleep 5");
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    let target = started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();
    let peer = started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "docs")
        .unwrap();
    let target_pid = target["pid"].as_u64().unwrap();
    let peer_pid = peer["pid"].as_u64().unwrap();
    let target_state_path = artifact_dir.join(target["state_path"].as_str().unwrap());
    let original_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&target_state_path).unwrap()).unwrap();
    let mut tampered_state = original_state.clone();
    tampered_state["process_identity"]["start_time_ticks"] = json!(
        original_state["process_identity"]["start_time_ticks"]
            .as_u64()
            .unwrap()
            + 1
    );
    std::fs::write(
        &target_state_path,
        serde_json::to_vec_pretty(&tampered_state).unwrap(),
    )
    .unwrap();
    let eventlog_path = artifact_dir.join("eventlog.jsonl");
    let cancelled_events_before = std::fs::read_to_string(&eventlog_path)
        .unwrap()
        .matches("worker_cancelled")
        .count();

    let cancelled = TasksCommand
        .execute(context(
            format!("swarm cancel --json --workflow {run_id} --dispatch {dispatch_id} --task api"),
            &root,
        ))
        .await;
    let cancel_error = cancelled.as_ref().err().map(ToString::to_string);
    let target_alive_after = ProcessCommand::new("kill")
        .args(["-0", &target_pid.to_string()])
        .status()
        .unwrap()
        .success();
    let peer_alive_after = ProcessCommand::new("kill")
        .args(["-0", &peer_pid.to_string()])
        .status()
        .unwrap()
        .success();
    let state_after: Value =
        serde_json::from_str(&std::fs::read_to_string(&target_state_path).unwrap()).unwrap();
    let cancelled_events_after = std::fs::read_to_string(&eventlog_path)
        .unwrap()
        .matches("worker_cancelled")
        .count();

    for pid in [target_pid, peer_pid] {
        let _ = ProcessCommand::new("kill")
            .args(["-TERM", "--", &format!("-{pid}")])
            .status();
    }
    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(&root);

    assert!(cancel_error
        .as_deref()
        .is_some_and(|error| error.contains("process_identity_mismatch")));
    assert!(target_alive_after, "target worker must not be signalled");
    assert!(peer_alive_after, "peer worker must not be signalled");
    assert_ne!(state_after["status"], "cancelled");
    assert_eq!(cancelled_events_after, cancelled_events_before);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn swarm_monitor_records_identity_mismatch_without_signaling() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(&root, "sleep 5");
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    let target = started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();
    let peer = started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "docs")
        .unwrap();
    let target_pid = target["pid"].as_u64().unwrap();
    let peer_pid = peer["pid"].as_u64().unwrap();
    let target_state_path = artifact_dir.join(target["state_path"].as_str().unwrap());
    let mut target_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&target_state_path).unwrap()).unwrap();
    target_state["process_identity"]["start_time_ticks"] = json!(
        target_state["process_identity"]["start_time_ticks"]
            .as_u64()
            .unwrap()
            + 1
    );
    std::fs::write(
        &target_state_path,
        serde_json::to_vec_pretty(&target_state).unwrap(),
    )
    .unwrap();

    let monitored = TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let monitored: Value = serde_json::from_str(&monitored.value).unwrap();
    let target_result = monitored["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap()
        .clone();
    let state_after: Value =
        serde_json::from_str(&std::fs::read_to_string(&target_state_path).unwrap()).unwrap();
    let packet: Value = serde_json::from_str(
        &std::fs::read_to_string(artifact_dir.join(target_result["result_path"].as_str().unwrap()))
            .unwrap(),
    )
    .unwrap();
    let target_alive_after = ProcessCommand::new("kill")
        .args(["-0", &target_pid.to_string()])
        .status()
        .unwrap()
        .success();
    let peer_alive_after = ProcessCommand::new("kill")
        .args(["-0", &peer_pid.to_string()])
        .status()
        .unwrap()
        .success();

    for pid in [target_pid, peer_pid] {
        let _ = ProcessCommand::new("kill")
            .args(["-TERM", "--", &format!("-{pid}")])
            .status();
    }
    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(target_result["status"], "lost");
    assert_eq!(state_after["status"], "lost");
    assert_eq!(state_after["process_identity_status"], "mismatch");
    assert!(state_after["termination_reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("process_identity_mismatch")));
    assert_eq!(packet["status"], "lost");
    assert!(packet["termination_reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("process_identity_mismatch")));
    assert!(target_alive_after, "monitor must not signal the target");
    assert!(peer_alive_after, "monitor must not signal the peer");
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_monitor_commits_idempotent_result_packets_for_completed_workers() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\\n// worker\\n' >> src/api/mod.rs; else printf '\\nworker\\n' >> docs/readme.md; fi\nprintf '{\"commands_run\":[\"fixture-edit\"]}\\n' > \"$KIANA_SWARM_TELEMETRY_PATH\"",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let _ = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let first = TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let first: Value = serde_json::from_str(&first.value).unwrap();
    assert_eq!(first["schema"], "kiana.swarm-monitor-result.v1");
    assert_eq!(first["status"], "terminal");
    assert_eq!(first["result_packets_created"], 2);
    assert_eq!(first["result_packets_reused"], 0);
    assert!(first["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["status"] == "completed"));
    for worker in first["workers"].as_array().unwrap() {
        let result_path = worker["result_path"].as_str().unwrap();
        let result: Value =
            serde_json::from_str(&std::fs::read_to_string(artifact_dir.join(result_path)).unwrap())
                .unwrap();
        assert_eq!(result["schema"], "kiana.swarm-result-packet.v1");
        assert_eq!(result["termination_reason"], "completed");
        assert_eq!(result["commands_run"], json!(["fixture-edit"]));
        assert_eq!(
            result["telemetry"]["schema"],
            "kiana.swarm-worker-telemetry.v1"
        );
        assert_eq!(result["telemetry"]["provided"], true);
        assert_eq!(result["telemetry"]["command_count"], 1);
        assert_eq!(result["telemetry"]["commands_run"], json!(["fixture-edit"]));
        assert_eq!(result["telemetry"]["invalid_command_entries"], 0);
        assert!(result["telemetry"]["raw_sha256"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert_eq!(result["scope_deviations"], json!([]));
        assert_eq!(result["changed_files"].as_array().unwrap().len(), 1);
    }

    let event_count = std::fs::read_to_string(artifact_dir.join("eventlog.jsonl"))
        .unwrap()
        .lines()
        .count();
    let second = TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let second: Value = serde_json::from_str(&second.value).unwrap();
    assert_eq!(second["result_packets_created"], 0);
    assert_eq!(second["result_packets_reused"], 2);
    assert_eq!(
        std::fs::read_to_string(artifact_dir.join("eventlog.jsonl"))
            .unwrap()
            .lines()
            .count(),
        event_count
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_monitor_retries_worker_failed_until_success() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2 --max-attempts 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ] && [ \"$KIANA_SWARM_ATTEMPT\" = 1 ]; then exit 7; fi\nif [ \"$KIANA_SWARM_TASK_ID\" = api ]; then\n  printf '{\"commands_run\":[\"retry-success\"]}\n' > \"$KIANA_SWARM_TELEMETRY_PATH\"\nelse\n  printf '{\"commands_run\":[\"peer-success\"]}\n' > \"$KIANA_SWARM_TELEMETRY_PATH\"\nfi\nexit 0",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let _ = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let first = TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let first: Value = serde_json::from_str(&first.value).unwrap();
    assert_eq!(first["status"], "running");
    assert_eq!(first["running"], 1);
    assert_eq!(first["result_packets_created"], 1);
    assert_eq!(
        first["process_identity_backend"]["schema"],
        "kiana.swarm-process-identity-backend.v1"
    );
    assert_eq!(first["process_identity_backend"]["backend"], "linux_procfs");
    assert_eq!(first["process_identity_backend"]["supported"], true);
    assert_eq!(
        first["process_identity_backend"]["safe_to_monitor_workers"],
        true
    );
    let worker = first["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();
    assert_eq!(worker["status"], "running");
    assert_eq!(worker["retrying"], true);
    assert_eq!(worker["attempt"], 2);
    assert_eq!(worker["previous_status"], "failed");
    assert_eq!(worker["previous_termination_reason"], "worker_failed");
    assert_eq!(worker["health"]["schema"], "kiana.swarm-worker-health.v1");
    assert_eq!(worker["health"]["state"], "retrying");
    assert_eq!(worker["health"]["reason"], "retrying_after_worker_failed");
    assert_eq!(worker["health"]["attempt"]["current"], 2);
    assert_eq!(worker["health"]["attempt"]["max"], 2);
    assert_eq!(worker["health"]["attempt"]["remaining"], 0);
    assert_eq!(worker["health"]["attempt"]["retrying"], true);
    assert_eq!(worker["health"]["process"]["identity_status"], "verified");
    assert_eq!(worker["health"]["next_action"], "run_swarm_monitor");
    let worker_dir = artifact_dir.join("workers").join(dispatch_id).join("api");
    assert_eq!(
        std::fs::read_to_string(worker_dir.join("attempts/1/exit_code"))
            .unwrap()
            .trim(),
        "7"
    );
    let summary: Value = serde_json::from_str(
        &std::fs::read_to_string(worker_dir.join("attempts/1/attempt-summary.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(summary["schema"], "kiana.swarm-retry-attempt.v1");
    assert_eq!(summary["attempt"], 1);
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["termination_reason"], "worker_failed");
    assert!(!artifact_dir
        .join("workers")
        .join(dispatch_id)
        .join("api/result.json")
        .exists());

    tokio::time::sleep(Duration::from_millis(300)).await;
    let second = TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let second: Value = serde_json::from_str(&second.value).unwrap();
    assert_eq!(second["status"], "terminal");
    assert_eq!(second["result_packets_created"], 1);
    let final_worker = second["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();
    assert_eq!(final_worker["status"], "completed");
    assert_eq!(
        final_worker["health"]["schema"],
        "kiana.swarm-worker-health.v1"
    );
    assert_eq!(final_worker["health"]["state"], "completed");
    assert_eq!(final_worker["health"]["reason"], "completed");
    assert_eq!(final_worker["health"]["attempt"]["current"], 2);
    assert_eq!(
        final_worker["health"]["next_action"],
        "swarm_integration_review"
    );
    let result_path = final_worker["result_path"].as_str().unwrap();
    let result: Value =
        serde_json::from_str(&std::fs::read_to_string(artifact_dir.join(result_path)).unwrap())
            .unwrap();
    assert_eq!(result["termination_reason"], "completed");
    assert_eq!(result["commands_run"], json!(["retry-success"]));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_monitor_classifies_scope_output_and_command_budget_violations() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 30, &["src/api/**"]);
    write_task(&root, "docs", 20, &["docs/**"]);
    write_task(&root, "tests", 10, &["tests/**"]);
    for path in ["src/api", "docs", "tests"] {
        std::fs::create_dir_all(root.join(path)).unwrap();
    }
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 3 --max-commands 1 --max-output-bytes 1024"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "case \"$KIANA_SWARM_TASK_ID\" in\n  api) printf 'escape\\n' > outside.txt ;;\n  docs) head -c 2048 /dev/zero | tr '\\0' x ;;\n  tests) printf '{\"commands_run\":[\"one\",\"two\"]}\\n' > \"$KIANA_SWARM_TELEMETRY_PATH\" ;;\nesac",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let _ = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let monitored = TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let monitored: Value = serde_json::from_str(&monitored.value).unwrap();
    let status_for = |task: &str| {
        monitored["workers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|worker| worker["task_id"] == task)
            .unwrap()["status"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(status_for("api"), "scope_violation");
    assert_eq!(status_for("docs"), "budget_exhausted");
    assert_eq!(status_for("tests"), "budget_exhausted");
    let api_worker = monitored["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();
    assert_eq!(
        api_worker["health"]["schema"],
        "kiana.swarm-worker-health.v1"
    );
    assert_eq!(api_worker["health"]["state"], "attention_required");
    assert_eq!(api_worker["health"]["scope"]["scope_deviations"], 1);
    assert_eq!(api_worker["health"]["next_action"], "inspect_worker_result");
    let tests_worker = monitored["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "tests")
        .unwrap();
    let tests_result_path = tests_worker["result_path"].as_str().unwrap();
    let tests_result: Value = serde_json::from_str(
        &std::fs::read_to_string(
            PathBuf::from(workflow["artifact_dir"].as_str().unwrap()).join(tests_result_path),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        tests_result["telemetry"]["schema"],
        "kiana.swarm-worker-telemetry.v1"
    );
    assert_eq!(tests_result["telemetry"]["provided"], true);
    assert_eq!(tests_result["telemetry"]["command_count"], 2);
    assert_eq!(
        tests_result["telemetry"]["commands_run"],
        json!(["one", "two"])
    );
    assert!(monitored["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker.get("retrying").is_none()));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_monitor_terminates_workers_that_exceed_timeout() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let dispatch = TasksCommand
        .execute(context(
            format!(
                "swarm dispatch --json --workflow {run_id} --max-workers 2 --timeout-seconds 1"
            ),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(&root, "sleep 5");
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let _ = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let monitored = TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let monitored: Value = serde_json::from_str(&monitored.value).unwrap();
    assert_eq!(monitored["status"], "terminal");
    assert!(monitored["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["status"] == "timeout"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_plan_preflights_completed_non_overlapping_workers_without_editing_root() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let api_before = std::fs::read(root.join("src/api/mod.rs")).unwrap();
    let docs_before = std::fs::read(root.join("docs/readme.md")).unwrap();

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\\n// integrated candidate\\n' >> src/api/mod.rs; else printf '\\nintegrated candidate\\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let mut monitored = None;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let result = TasksCommand
            .execute(context(
                format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
                &root,
            ))
            .await
            .unwrap();
        let result: Value = serde_json::from_str(&result.value).unwrap();
        if result["status"] == "terminal" {
            monitored = Some(result);
            break;
        }
    }
    let monitored = monitored.expect("integration workers did not reach terminal state");
    assert!(monitored["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["status"] == "completed"));

    let planned = TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let planned: Value = serde_json::from_str(&planned.value).unwrap();

    assert_eq!(planned["schema"], "kiana.swarm-integration-plan.v1");
    assert_eq!(planned["status"], "ready");
    assert_eq!(planned["workers"].as_array().unwrap().len(), 2);
    assert_eq!(planned["blockers"], json!([]));
    assert_eq!(planned["conflicts"], json!([]));
    assert_eq!(planned["next_action"], "run_swarm_integrate_apply");
    assert!(planned["workers"].as_array().unwrap().iter().all(|worker| {
        artifact_dir
            .join(worker["patch_path"].as_str().unwrap())
            .is_file()
    }));
    assert_eq!(
        std::fs::read(root.join("src/api/mod.rs")).unwrap(),
        api_before
    );
    assert_eq!(
        std::fs::read(root.join("docs/readme.md")).unwrap(),
        docs_before
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_plan_is_idempotent_and_keeps_apply_usable() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["grep -q 'candidate' src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["grep -q 'candidate' docs/readme.md"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let plan_path = artifact_dir
        .join("integrations")
        .join(&dispatch_id)
        .join("plan.json");
    let first_plan: Value =
        serde_json::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();

    let replanned = TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let replanned: Value = serde_json::from_str(&replanned.value).unwrap();

    assert_eq!(replanned, first_plan);
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(&plan_path).unwrap()).unwrap(),
        first_plan
    );

    let applied = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();
    assert_eq!(applied["status"], "integrated");

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_monitor_blocks_main_tree_changes_after_workers_start() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\n// candidate\n' >> src/api/mod.rs; else printf '\ncandidate\n' >> docs/readme.md; fi; sleep 2",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    let baseline_path = started["project_baseline_path"].as_str().unwrap();
    assert!(artifact_dir.join(baseline_path).is_file());
    assert!(started["project_baseline_sha256"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));

    std::fs::write(
        root.join("main-tree-drift.txt"),
        "changed while workers ran\n",
    )
    .unwrap();
    let monitored = TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let monitored: Value = serde_json::from_str(&monitored.value).unwrap();
    assert_eq!(monitored["status"], "terminal");
    assert!(monitored["workers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|worker| worker["status"] == "scope_violation"));

    let packet: Value = serde_json::from_str(
        &std::fs::read_to_string(
            artifact_dir
                .join("workers")
                .join(dispatch_id)
                .join("api")
                .join("result.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(packet["status"], "scope_violation");
    assert_eq!(
        packet["termination_reason"],
        "main_tree_changed_during_worker_run"
    );
    assert_eq!(packet["project_baseline_path"], baseline_path);
    assert_eq!(
        packet["project_baseline_sha256"],
        started["project_baseline_sha256"]
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_plan_blocks_isolation_changes_after_result_packet_capture() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\\n// captured\\n' >> src/api/mod.rs; else printf '\\ncaptured\\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let api_isolation = started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .and_then(|worker| worker["isolation_path"].as_str())
        .map(|path| root.join(path))
        .unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(api_isolation.join("src/api/mod.rs"))
        .unwrap()
        .write_all(b"// not captured\n")
        .unwrap();

    let planned = TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let planned: Value = serde_json::from_str(&planned.value).unwrap();
    let api = planned["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();

    assert_eq!(planned["status"], "blocked");
    assert!(api["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker == "isolation_manifest_mismatch"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_plan_blocks_tampered_result_packet() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\n// captured\n' >> src/api/mod.rs; else printf '\ncaptured\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let result_path = artifact_dir
        .join("workers")
        .join(dispatch_id)
        .join("api")
        .join("result.json");
    let mut terminal = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let monitored = TasksCommand
            .execute(context(
                format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
                &root,
            ))
            .await
            .unwrap();
        let monitored: Value = serde_json::from_str(&monitored.value).unwrap();
        if monitored["status"] == "terminal" && result_path.exists() {
            terminal = true;
            break;
        }
    }
    assert!(terminal, "workers did not produce terminal result packets");

    let mut result: Value =
        serde_json::from_str(&std::fs::read_to_string(&result_path).unwrap()).unwrap();
    result["tampered"] = json!(true);
    std::fs::write(&result_path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();

    let planned = TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let planned: Value = serde_json::from_str(&planned.value).unwrap();
    let api = planned["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|worker| worker["task_id"] == "api")
        .unwrap();

    assert_eq!(planned["status"], "blocked");
    assert!(api["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker == "result_packet_event_mismatch"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_serially_applies_and_verifies_ready_workers() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["touch verification-escape.txt && grep -q 'api candidate' src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["grep -q 'docs candidate' docs/readme.md"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\\n// api candidate\\n' >> src/api/mod.rs; else printf '\\ndocs candidate\\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();

    let applied = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();

    assert_eq!(applied["schema"], "kiana.swarm-integration-packet.v1");
    assert_eq!(applied["status"], "integrated");
    assert_eq!(applied["integrated_task_ids"], json!(["api", "docs"]));
    assert_eq!(applied["automatic_commit"], false);
    assert_eq!(applied["automatic_push"], false);
    assert_eq!(applied["automatic_merge"], false);
    assert_eq!(applied["automatic_deploy"], false);
    assert!(std::fs::read_to_string(root.join("src/api/mod.rs"))
        .unwrap()
        .contains("api candidate"));
    assert!(std::fs::read_to_string(root.join("docs/readme.md"))
        .unwrap()
        .contains("docs candidate"));
    assert!(!root.join("verification-escape.txt").exists());
    assert!(artifact_dir
        .join("integrations")
        .join(dispatch_id)
        .join("packet.json")
        .is_file());

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_marks_plan_stale_when_main_tree_changes() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\\n// candidate\\n' >> src/api/mod.rs; else printf '\\ncandidate\\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    std::fs::write(root.join("external-change.txt"), "user edit\n").unwrap();

    let applied = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();

    assert_eq!(applied["schema"], "kiana.swarm-integration-state.v1");
    assert_eq!(applied["status"], "stale");
    assert_eq!(applied["failure"]["reason"], "main_tree_drift");
    assert!(!std::fs::read_to_string(root.join("src/api/mod.rs"))
        .unwrap()
        .contains("candidate"));
    assert!(!std::fs::read_to_string(root.join("docs/readme.md"))
        .unwrap()
        .contains("candidate"));
    assert_eq!(
        std::fs::read_to_string(root.join("external-change.txt")).unwrap(),
        "user edit\n"
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_marks_plan_stale_when_only_git_index_changes() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    assert!(ProcessCommand::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    assert!(ProcessCommand::new("git")
        .args(["add", "src", "docs"])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    assert!(ProcessCommand::new("git")
        .args([
            "-c",
            "user.name=Kiana Test",
            "-c",
            "user.email=kiana@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    std::fs::write(
        root.join("stage-only.txt"),
        "same bytes before and after plan\n",
    )
    .unwrap();

    let (run_id, dispatch_id, _) = prepare_ready_swarm_integration_plan(&root).await;
    assert!(ProcessCommand::new("git")
        .args(["add", "stage-only.txt"])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());

    let applied = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();

    assert_eq!(applied["status"], "stale");
    assert_eq!(applied["failure"]["reason"], "main_tree_drift");
    assert!(!std::fs::read_to_string(root.join("src/api/mod.rs"))
        .unwrap()
        .contains("candidate"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_marks_plan_stale_when_only_file_mode_changes() {
    use std::os::unix::fs::PermissionsExt;

    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, _) = prepare_ready_swarm_integration_plan(&root).await;
    let path = root.join("src/api/mod.rs");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();

    let applied = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();

    assert_eq!(applied["status"], "stale");
    assert_eq!(applied["failure"]["reason"], "main_tree_drift");
    assert!(!std::fs::read_to_string(root.join("src/api/mod.rs"))
        .unwrap()
        .contains("candidate"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn swarm_integrate_apply_marks_plan_stale_when_only_xattr_changes() {
    use std::os::unix::ffi::OsStrExt;

    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let target = root.join("src/api/mod.rs");
    std::fs::write(&target, "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, _) = prepare_ready_swarm_integration_plan(&root).await;
    let path = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
    let name = std::ffi::CString::new("user.kiana.plan-drift").unwrap();
    let value = b"changed-after-plan";
    let result = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    assert_eq!(result, 0, "{}", std::io::Error::last_os_error());

    let applied = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();

    assert_eq!(applied["status"], "stale");
    assert_eq!(applied["failure"]["reason"], "main_tree_drift");
    assert!(!std::fs::read_to_string(&target)
        .unwrap()
        .contains("candidate"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_recovers_interrupted_applying_state_before_retry() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let original_api = std::fs::read(root.join("src/api/mod.rs")).unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let integration_dir = artifact_dir.join("integrations").join(&dispatch_id);
    let plan: Value =
        serde_json::from_str(&std::fs::read_to_string(integration_dir.join("plan.json")).unwrap())
            .unwrap();
    let checkpoint_file = integration_dir.join("checkpoint/files/src/api/mod.rs");
    std::fs::create_dir_all(checkpoint_file.parent().unwrap()).unwrap();
    std::fs::write(&checkpoint_file, &original_api).unwrap();
    let checkpoint = json!({
        "schema": "kiana.swarm-integration-checkpoint.v2",
        "integration_id": plan["integration_id"],
        "dispatch_id": dispatch_id,
        "plan_sha256": plan["plan_sha256"],
        "baseline_fingerprint": plan["baseline"]["working_tree_fingerprint"],
        "files": [{
            "path": "src/api/mod.rs",
            "existed": true,
            "kind": "file",
            "backup_path": "checkpoint/files/src/api/mod.rs",
            "backup_sha256": format!("sha256:{}", test_sha256_bytes(&original_api)),
            "backup_mode": test_file_mode(&checkpoint_file),
            "symlink_target": null,
            "metadata": test_checkpoint_metadata_value(&checkpoint_file)
        }],
        "created_at_ms": 1
    });
    let checkpoint_bytes = serde_json::to_vec_pretty(&checkpoint).unwrap();
    std::fs::write(
        integration_dir.join("checkpoint/manifest.json"),
        &checkpoint_bytes,
    )
    .unwrap();
    let checkpoint_sha256 = format!("sha256:{}", test_sha256_bytes(&checkpoint_bytes));
    std::fs::write(
        root.join("src/api/mod.rs"),
        "pub fn api() {}\n// half applied\n",
    )
    .unwrap();
    let half_applied_fingerprint = test_working_tree_fingerprint(&root);
    std::fs::write(
        integration_dir.join("state.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": "kiana.swarm-integration-state.v1",
            "integration_id": plan["integration_id"],
            "dispatch_id": dispatch_id,
            "status": "applying",
            "plan_sha256": plan["plan_sha256"],
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": checkpoint_sha256,
            "working_tree_fingerprint": half_applied_fingerprint,
            "applied_task_ids": ["api"],
            "verified_task_ids": [],
            "current_task_id": "api",
            "failure": null,
            "updated_at_ms": 1,
            "next_action": "continue_integration"
        }))
        .unwrap(),
    )
    .unwrap();
    append_test_recovery_anchor_events(
        &artifact_dir,
        &plan,
        &dispatch_id,
        &checkpoint_sha256,
        &half_applied_fingerprint,
    );

    let recovered = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let recovered: Value = serde_json::from_str(&recovered.value).unwrap();

    assert_eq!(recovered["status"], "rolled_back");
    assert_eq!(
        recovered["failure"]["reason"],
        "interrupted_integration_recovered"
    );
    assert_eq!(
        std::fs::read(root.join("src/api/mod.rs")).unwrap(),
        original_api
    );
    assert!(!std::fs::read_to_string(root.join("docs/readme.md"))
        .unwrap()
        .contains("candidate"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_blocks_recovery_after_user_edits_interrupted_tree() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let original_api = std::fs::read(root.join("src/api/mod.rs")).unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let integration_dir = artifact_dir.join("integrations").join(&dispatch_id);
    let plan: Value =
        serde_json::from_str(&std::fs::read_to_string(integration_dir.join("plan.json")).unwrap())
            .unwrap();
    let checkpoint_file = integration_dir.join("checkpoint/files/src/api/mod.rs");
    std::fs::create_dir_all(checkpoint_file.parent().unwrap()).unwrap();
    std::fs::write(&checkpoint_file, &original_api).unwrap();
    let checkpoint = json!({
        "schema": "kiana.swarm-integration-checkpoint.v2",
        "integration_id": plan["integration_id"],
        "dispatch_id": dispatch_id,
        "plan_sha256": plan["plan_sha256"],
        "baseline_fingerprint": plan["baseline"]["working_tree_fingerprint"],
        "files": [{
            "path": "src/api/mod.rs",
            "existed": true,
            "kind": "file",
            "backup_path": "checkpoint/files/src/api/mod.rs",
            "backup_sha256": format!("sha256:{}", test_sha256_bytes(&original_api)),
            "backup_mode": test_file_mode(&checkpoint_file),
            "symlink_target": null,
            "metadata": test_checkpoint_metadata_value(&checkpoint_file)
        }],
        "created_at_ms": 1
    });
    let checkpoint_bytes = serde_json::to_vec_pretty(&checkpoint).unwrap();
    std::fs::write(
        integration_dir.join("checkpoint/manifest.json"),
        &checkpoint_bytes,
    )
    .unwrap();
    let checkpoint_sha256 = format!("sha256:{}", test_sha256_bytes(&checkpoint_bytes));

    std::fs::write(
        root.join("src/api/mod.rs"),
        "pub fn api() {}\n// half applied\n",
    )
    .unwrap();
    let known_half_applied_fingerprint = test_working_tree_fingerprint(&root);
    std::fs::write(
        integration_dir.join("state.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": "kiana.swarm-integration-state.v1",
            "integration_id": plan["integration_id"],
            "dispatch_id": dispatch_id,
            "status": "applying",
            "plan_sha256": plan["plan_sha256"],
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": checkpoint_sha256,
            "working_tree_fingerprint": known_half_applied_fingerprint,
            "applied_task_ids": ["api"],
            "verified_task_ids": [],
            "current_task_id": "api",
            "failure": null,
            "updated_at_ms": 1,
            "next_action": "continue_integration"
        }))
        .unwrap(),
    )
    .unwrap();
    append_test_recovery_anchor_events(
        &artifact_dir,
        &plan,
        &dispatch_id,
        &checkpoint_sha256,
        &known_half_applied_fingerprint,
    );
    let user_edit = "pub fn api() {}\n// half applied\n// user edit after interrupt\n";
    std::fs::write(root.join("src/api/mod.rs"), user_edit).unwrap();

    let recovered = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let recovered: Value = serde_json::from_str(&recovered.value).unwrap();

    assert_eq!(recovered["status"], "recovery_blocked");
    assert_eq!(
        recovered["failure"]["reason"],
        "interrupted_integration_tree_drift"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/api/mod.rs")).unwrap(),
        user_edit
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_blocks_recovery_when_checkpoint_backup_is_tampered() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let original_api = std::fs::read(root.join("src/api/mod.rs")).unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let integration_dir = artifact_dir.join("integrations").join(&dispatch_id);
    let plan: Value =
        serde_json::from_str(&std::fs::read_to_string(integration_dir.join("plan.json")).unwrap())
            .unwrap();
    let checkpoint_file = integration_dir.join("checkpoint/files/src/api/mod.rs");
    std::fs::create_dir_all(checkpoint_file.parent().unwrap()).unwrap();
    std::fs::write(&checkpoint_file, &original_api).unwrap();
    let mut checkpoint = json!({
        "schema": "kiana.swarm-integration-checkpoint.v2",
        "integration_id": plan["integration_id"],
        "dispatch_id": dispatch_id,
        "plan_sha256": plan["plan_sha256"],
        "baseline_fingerprint": plan["baseline"]["working_tree_fingerprint"],
        "files": [{
            "path": "src/api/mod.rs",
            "existed": true,
            "kind": "file",
            "backup_path": "checkpoint/files/src/api/mod.rs",
            "backup_sha256": format!("sha256:{}", test_sha256_bytes(&original_api)),
            "backup_mode": test_file_mode(&checkpoint_file),
            "symlink_target": null,
            "metadata": test_checkpoint_metadata_value(&checkpoint_file)
        }],
        "created_at_ms": 1
    });
    let checkpoint_bytes = serde_json::to_vec_pretty(&checkpoint).unwrap();
    std::fs::write(
        integration_dir.join("checkpoint/manifest.json"),
        &checkpoint_bytes,
    )
    .unwrap();
    let checkpoint_sha256 = format!("sha256:{}", test_sha256_bytes(&checkpoint_bytes));

    let half_applied = "pub fn api() {}\n// half applied\n";
    std::fs::write(root.join("src/api/mod.rs"), half_applied).unwrap();
    let half_applied_fingerprint = test_working_tree_fingerprint(&root);
    std::fs::write(
        integration_dir.join("state.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": "kiana.swarm-integration-state.v1",
            "integration_id": plan["integration_id"],
            "dispatch_id": dispatch_id,
            "status": "applying",
            "plan_sha256": plan["plan_sha256"],
            "checkpoint_path": "checkpoint/manifest.json",
            "checkpoint_sha256": checkpoint_sha256,
            "working_tree_fingerprint": half_applied_fingerprint,
            "applied_task_ids": ["api"],
            "verified_task_ids": [],
            "current_task_id": "api",
            "failure": null,
            "updated_at_ms": 1,
            "next_action": "continue_integration"
        }))
        .unwrap(),
    )
    .unwrap();
    append_test_recovery_anchor_events(
        &artifact_dir,
        &plan,
        &dispatch_id,
        &checkpoint_sha256,
        &half_applied_fingerprint,
    );

    let attacker_backup = b"pub fn attacker_controlled() {}\n";
    std::fs::write(&checkpoint_file, attacker_backup).unwrap();
    checkpoint["files"][0]["backup_sha256"] =
        json!(format!("sha256:{}", test_sha256_bytes(attacker_backup)));
    let tampered_checkpoint_bytes = serde_json::to_vec_pretty(&checkpoint).unwrap();
    std::fs::write(
        integration_dir.join("checkpoint/manifest.json"),
        &tampered_checkpoint_bytes,
    )
    .unwrap();
    let tampered_checkpoint_sha256 =
        format!("sha256:{}", test_sha256_bytes(&tampered_checkpoint_bytes));
    let integration_state_path = integration_dir.join("state.json");
    let mut tampered_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&integration_state_path).unwrap()).unwrap();
    tampered_state["checkpoint_sha256"] = json!(tampered_checkpoint_sha256);
    std::fs::write(
        integration_state_path,
        serde_json::to_vec_pretty(&tampered_state).unwrap(),
    )
    .unwrap();

    let recovered = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let recovered: Value = serde_json::from_str(&recovered.value).unwrap();

    assert_eq!(recovered["status"], "recovery_blocked");
    assert_eq!(
        recovered["failure"]["reason"],
        "interrupted_integration_checkpoint_invalid"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/api/mod.rs")).unwrap(),
        half_applied
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_rejects_forged_success_packet_before_touching_main_tree() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let api_before = std::fs::read(root.join("src/api/mod.rs")).unwrap();
    let docs_before = std::fs::read(root.join("docs/readme.md")).unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let integration_dir = artifact_dir.join("integrations").join(&dispatch_id);
    let plan: Value =
        serde_json::from_str(&std::fs::read_to_string(integration_dir.join("plan.json")).unwrap())
            .unwrap();
    std::fs::write(
        integration_dir.join("packet.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": "kiana.swarm-integration-packet.v1",
            "integration_id": plan["integration_id"],
            "workflow_id": plan["workflow_id"],
            "run_id": plan["run_id"],
            "dispatch_id": dispatch_id,
            "status": "integrated",
            "plan_sha256": plan["plan_sha256"],
            "integrated_task_ids": ["api", "docs"],
            "rejected_task_ids": [],
            "commands": [],
            "verification_packet_path": "integrations/forged/verification.json",
            "rollback": null,
            "automatic_commit": false,
            "automatic_push": false,
            "automatic_merge": false,
            "automatic_deploy": false,
            "created_at_ms": 1,
            "next_action": "run_swarm_cleanup_or_review"
        }))
        .unwrap(),
    )
    .unwrap();

    let error = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("integration packet event is missing"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(root.join("src/api/mod.rs")).unwrap(),
        api_before
    );
    assert_eq!(
        std::fs::read(root.join("docs/readme.md")).unwrap(),
        docs_before
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_rebuilds_missing_state_from_authenticated_completion() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["grep -q 'candidate' src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["grep -q 'candidate' docs/readme.md"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let command =
        format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}");
    let applied = TasksCommand
        .execute(context(command.clone(), &root))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();
    assert_eq!(applied["status"], "integrated");

    let integration_dir = artifact_dir.join("integrations").join(&dispatch_id);
    let state_path = integration_dir.join("state.json");
    std::fs::remove_file(&state_path).unwrap();

    let recovered = TasksCommand.execute(context(command, &root)).await.unwrap();
    let recovered: Value = serde_json::from_str(&recovered.value).unwrap();
    let rebuilt_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();

    assert_eq!(recovered, applied);
    assert_eq!(rebuilt_state["status"], "integrated");
    assert_eq!(rebuilt_state["plan_sha256"], recovered["plan_sha256"]);
    assert!(rebuilt_state["packet_sha256"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(
        std::fs::read_to_string(root.join("src/api/mod.rs"))
            .unwrap()
            .matches("// candidate")
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/readme.md"))
            .unwrap()
            .matches("candidate")
            .count(),
        1
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_recovers_packet_written_before_completed_event() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["grep -q 'candidate' src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["grep -q 'candidate' docs/readme.md"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let command =
        format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}");
    let applied = TasksCommand
        .execute(context(command.clone(), &root))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();
    assert_eq!(applied["status"], "integrated");

    let eventlog_path = artifact_dir.join("eventlog.jsonl");
    let mut events = std::fs::read_to_string(&eventlog_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let completed = events.pop().unwrap();
    assert_eq!(completed["kind"], "swarm_integration_completed");
    let previous_event = events.last().unwrap().clone();
    let mut eventlog = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    eventlog.push('\n');
    std::fs::write(&eventlog_path, eventlog).unwrap();

    let workflow_state_path = artifact_dir.join("state.json");
    let mut workflow_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&workflow_state_path).unwrap()).unwrap();
    workflow_state["last_event_seq"] = previous_event["seq"].clone();
    workflow_state["updated_at_ms"] = previous_event["at_ms"].clone();
    std::fs::write(
        &workflow_state_path,
        serde_json::to_vec_pretty(&workflow_state).unwrap(),
    )
    .unwrap();

    let integration_state_path = artifact_dir
        .join("integrations")
        .join(&dispatch_id)
        .join("state.json");
    let mut integration_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&integration_state_path).unwrap()).unwrap();
    integration_state["status"] = json!("verifying");
    integration_state["current_task_id"] = json!("docs");
    integration_state["packet_sha256"] = Value::Null;
    integration_state["next_action"] = json!("continue_integration");
    std::fs::write(
        &integration_state_path,
        serde_json::to_vec_pretty(&integration_state).unwrap(),
    )
    .unwrap();

    let recovered = TasksCommand.execute(context(command, &root)).await.unwrap();
    let recovered: Value = serde_json::from_str(&recovered.value).unwrap();
    assert_eq!(recovered, applied);
    assert_eq!(
        std::fs::read_to_string(root.join("src/api/mod.rs"))
            .unwrap()
            .matches("// candidate")
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/readme.md"))
            .unwrap()
            .matches("candidate")
            .count(),
        1
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_recovers_verification_event_written_before_packet() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["grep -q 'candidate' src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["grep -q 'candidate' docs/readme.md"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let command =
        format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}");
    let applied = TasksCommand
        .execute(context(command.clone(), &root))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();
    assert_eq!(applied["status"], "integrated");

    let eventlog_path = artifact_dir.join("eventlog.jsonl");
    let mut events = std::fs::read_to_string(&eventlog_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let completed = events.pop().unwrap();
    assert_eq!(completed["kind"], "swarm_integration_completed");
    let verification_event = events.last().unwrap().clone();
    assert_eq!(verification_event["kind"], "verification_completed");
    let mut eventlog = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    eventlog.push('\n');
    std::fs::write(&eventlog_path, eventlog).unwrap();

    let workflow_state_path = artifact_dir.join("state.json");
    let mut workflow_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&workflow_state_path).unwrap()).unwrap();
    workflow_state["last_event_seq"] = verification_event["seq"].clone();
    workflow_state["updated_at_ms"] = verification_event["at_ms"].clone();
    std::fs::write(
        &workflow_state_path,
        serde_json::to_vec_pretty(&workflow_state).unwrap(),
    )
    .unwrap();

    let integration_dir = artifact_dir.join("integrations").join(&dispatch_id);
    std::fs::remove_file(integration_dir.join("packet.json")).unwrap();
    let state_path = integration_dir.join("state.json");
    let mut state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
    state["status"] = json!("verifying");
    state["current_task_id"] = json!("docs");
    state["packet_sha256"] = Value::Null;
    state["next_action"] = json!("continue_integration");
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let recovered = TasksCommand.execute(context(command, &root)).await.unwrap();
    let recovered: Value = serde_json::from_str(&recovered.value).unwrap();

    assert_eq!(recovered, applied);
    assert_eq!(
        std::fs::read_to_string(root.join("src/api/mod.rs"))
            .unwrap()
            .matches("// candidate")
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(root.join("docs/readme.md"))
            .unwrap()
            .matches("candidate")
            .count(),
        1
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_rejects_orphan_packet_without_verification_event() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["grep -q 'candidate' src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["grep -q 'candidate' docs/readme.md"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let command =
        format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}");
    let applied = TasksCommand
        .execute(context(command.clone(), &root))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();
    assert_eq!(applied["status"], "integrated");
    let api_after = std::fs::read(root.join("src/api/mod.rs")).unwrap();
    let docs_after = std::fs::read(root.join("docs/readme.md")).unwrap();

    let eventlog_path = artifact_dir.join("eventlog.jsonl");
    let events = std::fs::read_to_string(&eventlog_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| {
            !matches!(
                event["kind"].as_str(),
                Some("swarm_integration_completed" | "verification_completed")
            )
        })
        .collect::<Vec<_>>();
    let previous_event = events.last().unwrap().clone();
    let mut eventlog = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    eventlog.push('\n');
    std::fs::write(&eventlog_path, eventlog).unwrap();

    let workflow_state_path = artifact_dir.join("state.json");
    let mut workflow_state: Value =
        serde_json::from_str(&std::fs::read_to_string(&workflow_state_path).unwrap()).unwrap();
    workflow_state["last_event_seq"] = previous_event["seq"].clone();
    workflow_state["updated_at_ms"] = previous_event["at_ms"].clone();
    std::fs::write(
        &workflow_state_path,
        serde_json::to_vec_pretty(&workflow_state).unwrap(),
    )
    .unwrap();

    let integration_dir = artifact_dir.join("integrations").join(&dispatch_id);
    let verification_path = integration_dir.join("verification.json");
    let mut verification: Value =
        serde_json::from_str(&std::fs::read_to_string(&verification_path).unwrap()).unwrap();
    verification["checks"] = json!([{
        "command": "false",
        "status": "pass",
        "exit_code": 0,
        "forged": true
    }]);
    std::fs::write(
        &verification_path,
        serde_json::to_vec_pretty(&verification).unwrap(),
    )
    .unwrap();

    let packet_path = integration_dir.join("packet.json");
    let mut packet: Value =
        serde_json::from_str(&std::fs::read_to_string(&packet_path).unwrap()).unwrap();
    packet["commands"] = verification["checks"].clone();
    packet["created_at_ms"] = verification["created_at_ms"].clone();
    std::fs::write(&packet_path, serde_json::to_vec_pretty(&packet).unwrap()).unwrap();

    let state_path = integration_dir.join("state.json");
    let mut state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
    state["status"] = json!("verifying");
    state["current_task_id"] = json!("docs");
    state["packet_sha256"] = Value::Null;
    state["next_action"] = json!("continue_integration");
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let error = TasksCommand
        .execute(context(command, &root))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("integration verification event is missing"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(root.join("src/api/mod.rs")).unwrap(),
        api_after
    );
    assert_eq!(
        std::fs::read(root.join("docs/readme.md")).unwrap(),
        docs_after
    );
    assert!(!std::fs::read_to_string(&eventlog_path)
        .unwrap()
        .contains("swarm_integration_completed"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_rejects_tampered_completed_packet() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["grep -q 'candidate' src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["grep -q 'candidate' docs/readme.md"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let command =
        format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}");
    let applied = TasksCommand
        .execute(context(command.clone(), &root))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();
    assert_eq!(applied["status"], "integrated");
    let api_after = std::fs::read(root.join("src/api/mod.rs")).unwrap();
    let docs_after = std::fs::read(root.join("docs/readme.md")).unwrap();

    let packet_path = artifact_dir
        .join("integrations")
        .join(&dispatch_id)
        .join("packet.json");
    let mut packet: Value =
        serde_json::from_str(&std::fs::read_to_string(&packet_path).unwrap()).unwrap();
    packet["next_action"] = json!("tampered_without_event_update");
    std::fs::write(&packet_path, serde_json::to_vec_pretty(&packet).unwrap()).unwrap();

    let error = TasksCommand
        .execute(context(command, &root))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("integration packet event authentication mismatch"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(root.join("src/api/mod.rs")).unwrap(),
        api_after
    );
    assert_eq!(
        std::fs::read(root.join("docs/readme.md")).unwrap(),
        docs_after
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_rejects_tampered_plan_before_applying_patches() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let api_before = std::fs::read(root.join("src/api/mod.rs")).unwrap();

    let (run_id, dispatch_id, artifact_dir) = prepare_ready_swarm_integration_plan(&root).await;
    let plan_path = artifact_dir
        .join("integrations")
        .join(&dispatch_id)
        .join("plan.json");
    let mut plan: Value =
        serde_json::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();
    plan["next_action"] = json!("tampered_without_rehash");
    std::fs::write(&plan_path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();

    let error = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("integration plan hash mismatch"), "{error}");
    assert_eq!(
        std::fs::read(root.join("src/api/mod.rs")).unwrap(),
        api_before
    );

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_integrate_apply_rolls_back_all_workers_when_later_verification_fails() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["grep -q 'api candidate' src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["false"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();
    let api_before = std::fs::read(root.join("src/api/mod.rs")).unwrap();
    let docs_before = std::fs::read(root.join("docs/readme.md")).unwrap();

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\\n// api candidate\\n' >> src/api/mod.rs; else printf '\\ndocs candidate\\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();

    let applied = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();

    assert_eq!(applied["status"], "rolled_back");
    assert_eq!(applied["rollback"]["restored"], true);
    assert_eq!(
        applied["rollback"]["recovery_journal_path"],
        "recovery/journal.json"
    );
    assert_eq!(applied["failure"]["task_id"], "docs");
    assert_eq!(applied["failure"]["phase"], "verification");
    assert_eq!(
        std::fs::read(root.join("src/api/mod.rs")).unwrap(),
        api_before
    );
    assert_eq!(
        std::fs::read(root.join("docs/readme.md")).unwrap(),
        docs_before
    );
    let journal_path = artifact_dir
        .join("integrations")
        .join(dispatch_id)
        .join("recovery/journal.json");
    let journal: Value = serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
    assert_eq!(
        journal["schema"],
        "kiana.swarm-integration-recovery-journal.v2"
    );
    assert!(journal["revision"].as_u64().unwrap() > 0);
    assert_eq!(
        journal["integrity"]["schema"],
        "kiana.integrity-envelope.v1"
    );
    assert_eq!(journal["integrity"]["algorithm"], "local_hmac_sha256_v1");
    assert_eq!(journal["status"], "completed");
    assert_eq!(
        journal["next_entry_index"].as_u64().unwrap(),
        journal["entries"].as_array().unwrap().len() as u64
    );
    assert!(journal["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["status"] == "completed"));

    let events = std::fs::read_to_string(artifact_dir.join("eventlog.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let recovery_event = events
        .iter()
        .find(|event| event["kind"] == "swarm_integration_recovery_started")
        .expect("recovery event must be persisted");
    assert_eq!(recovery_event["data"]["dispatch_id"], dispatch_id);
    assert_eq!(
        recovery_event["data"]["checkpoint_sha256"],
        journal["checkpoint_sha256"]
    );
    assert_eq!(
        recovery_event["data"]["journal_path"],
        "recovery/journal.json"
    );
    assert!(recovery_event["data"]["journal_binding_sha256"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_cleanup_removes_terminal_isolations_and_preserves_fact_packets() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task_with_metadata(
        &root,
        "api",
        20,
        &["src/api/**"],
        json!({"verification_commands": ["test -f src/api/mod.rs"]}),
    );
    write_task_with_metadata(
        &root,
        "docs",
        10,
        &["docs/**"],
        json!({"verification_commands": ["test -f docs/readme.md"]}),
    );
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let workflow = init_workflow(&root).await;
    let run_id = workflow["run_id"].as_str().unwrap();
    let artifact_dir = PathBuf::from(workflow["artifact_dir"].as_str().unwrap());
    let dispatch = TasksCommand
        .execute(context(
            format!("swarm dispatch --json --workflow {run_id} --max-workers 2"),
            &root,
        ))
        .await
        .unwrap();
    let dispatch: Value = serde_json::from_str(&dispatch.value).unwrap();
    let dispatch_id = dispatch["dispatch_id"].as_str().unwrap();
    let runner = write_fixture_runner_with_body(
        &root,
        "if [ \"$KIANA_SWARM_TASK_ID\" = api ]; then printf '\\n// candidate\\n' >> src/api/mod.rs; else printf '\\ncandidate\\n' >> docs/readme.md; fi",
    );
    std::env::set_var("KIANA_SWARM_WORKER_EXECUTABLE", &runner);
    let started = TasksCommand
        .execute(context(
            format!("swarm start --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let started: Value = serde_json::from_str(&started.value).unwrap();
    let isolation_paths = started["workers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|worker| root.join(worker["isolation_path"].as_str().unwrap()))
        .collect::<Vec<_>>();
    assert!(isolation_paths.iter().all(|path| path.is_dir()));
    tokio::time::sleep(Duration::from_millis(300)).await;
    TasksCommand
        .execute(context(
            format!("swarm monitor --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    TasksCommand
        .execute(context(
            format!("swarm integrate plan --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();

    let cleaned = TasksCommand
        .execute(context(
            format!("swarm cleanup --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let cleaned: Value = serde_json::from_str(&cleaned.value).unwrap();
    assert_eq!(cleaned["schema"], "kiana.swarm-cleanup-result.v1");
    assert_eq!(cleaned["status"], "cleaned");
    assert_eq!(cleaned["removed"], 2);
    assert!(isolation_paths.iter().all(|path| !path.exists()));
    for task_id in ["api", "docs"] {
        assert!(artifact_dir
            .join("workers")
            .join(dispatch_id)
            .join(task_id)
            .join("result.json")
            .is_file());
    }
    assert!(artifact_dir
        .join("integrations")
        .join(dispatch_id)
        .join("packet.json")
        .is_file());

    let repeated = TasksCommand
        .execute(context(
            format!("swarm cleanup --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let repeated: Value = serde_json::from_str(&repeated.value).unwrap();
    assert_eq!(repeated["removed"], 0);
    assert_eq!(repeated["reused"], true);

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn swarm_cleanup_rejects_isolation_changes_after_result_packet_capture() {
    let _env_guard = SWARM_RUNNER_ENV_LOCK.lock().unwrap();
    let root = root();
    write_task(&root, "api", 20, &["src/api/**"]);
    write_task(&root, "docs", 10, &["docs/**"]);
    std::fs::create_dir_all(root.join("src/api")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/api/mod.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("docs/readme.md"), "# docs\n").unwrap();

    let (run_id, dispatch_id, _) = prepare_ready_swarm_integration_plan(&root).await;
    let applied = TasksCommand
        .execute(context(
            format!("swarm integrate apply --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap();
    let applied: Value = serde_json::from_str(&applied.value).unwrap();
    assert_eq!(applied["status"], "rolled_back");

    let api_isolation = root
        .join(".kiana/swarm-worktrees")
        .join(&dispatch_id)
        .join("api");
    let docs_isolation = root
        .join(".kiana/swarm-worktrees")
        .join(&dispatch_id)
        .join("docs");
    std::fs::OpenOptions::new()
        .append(true)
        .open(api_isolation.join("src/api/mod.rs"))
        .unwrap()
        .write_all(b"// changed after monitor\n")
        .unwrap();

    let error = TasksCommand
        .execute(context(
            format!("swarm cleanup --json --workflow {run_id} --dispatch {dispatch_id}"),
            &root,
        ))
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("cleanup isolation manifest mismatch"),
        "{error}"
    );
    assert!(api_isolation.is_dir());
    assert!(docs_isolation.is_dir());

    std::env::remove_var("KIANA_SWARM_WORKER_EXECUTABLE");
    let _ = std::fs::remove_dir_all(root);
}
