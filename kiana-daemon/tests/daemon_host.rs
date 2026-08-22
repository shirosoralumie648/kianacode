use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ExecutionStatus, PermissionProfile, RequestEnvelope, RequestMetadata, ResponseEnvelope,
};
use kiana_runner::{
    KianaHarness, ModelClient, ModelOutput, ModelRequest, ModelRole, ModelToolCall, ScriptedModel,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

struct InProcessTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for InProcessTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        Ok(self.host.handle(request).await)
    }
}

fn trusted_metadata() -> RequestMetadata {
    trusted_metadata_in("/repo")
}

fn trusted_metadata_in(project_root: impl AsRef<Path>) -> RequestMetadata {
    let mut metadata = RequestMetadata::local("session-1", project_root.as_ref().to_string_lossy());
    metadata.project_trusted = true;
    metadata
}

fn trusted_write_metadata_in(project_root: impl AsRef<Path>) -> RequestMetadata {
    let mut metadata = trusted_metadata_in(project_root);
    metadata.permission_profile = PermissionProfile::Balanced;
    metadata
}

fn apply_patch_cassette() -> serde_json::Value {
    json!([
        {
            "text": "writing",
            "tool_calls": [{
                "id": "c1",
                "name": "apply_patch",
                "arguments": {
                    "patch": "*** Begin Patch\n*** Add File: GOLDEN_PATH.txt\n+hello\n*** End Patch\n"
                }
            }]
        },
        {"text": "created GOLDEN_PATH.txt"}
    ])
}

fn temp_project() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-daemon-harness-{stamp}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn scripted_host(outputs: serde_json::Value) -> Arc<DaemonHost> {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::from_json(&outputs).unwrap()));
    Arc::new(DaemonHost::with_harness(harness).expect("daemon with kiana harness"))
}

#[test]
fn local_daemon_constructs_without_a_model() {
    let _host = DaemonHost::local().expect("local daemon");
}

#[tokio::test]
async fn run_brokers_kiana_harness_tools() {
    let host = scripted_host(json!([
        {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
        {"text": "architecture mapped"}
    ]));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(trusted_metadata(), "map the architecture", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.output["schema"], "kiana.run-result.v1");
    assert_eq!(response.output["harness"], "kiana-harness");
    assert_eq!(response.output["sandbox"], "read-only");
    assert_eq!(
        response.output["output"]["schema"],
        "kiana.harness-result.v1"
    );
    assert_eq!(response.output["output"]["text"], "architecture mapped");
}

#[tokio::test]
async fn trusted_workspace_write_apply_patch_creates_file() {
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "create a file named GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["harness"], "kiana-harness");
    assert_eq!(response.output["sandbox"], "workspace-write");
    assert_eq!(response.output["role_id"], "builder");
    assert_eq!(response.output["department_id"], "executing");
    assert_eq!(
        fs::read_to_string(root.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
}

#[tokio::test]
async fn untrusted_workspace_write_does_not_create_file() {
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = RequestMetadata::local("session-1", root.to_string_lossy());
    metadata.permission_profile = PermissionProfile::Balanced;
    let response = client
        .run(
            metadata,
            "create a file named GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("workspace_write_requires_trusted_non_safe_profile")
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn trusted_read_only_apply_patch_does_not_create_file() {
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_metadata_in(&root),
            "create a file named GOLDEN_PATH.txt containing hello",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn empty_prompt_is_blocked_before_harness_start() {
    let host = scripted_host(json!([{"text": "should not run"}]));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client.run(trusted_metadata(), "   ", None).await.unwrap();
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("prompt_required"));
}

struct RecordingModel {
    marker: String,
    step: Mutex<u32>,
}

#[async_trait]
impl ModelClient for RecordingModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        let mut step = self
            .step
            .lock()
            .map_err(|_| "model_lock_poisoned".to_owned())?;
        *step += 1;
        match *step {
            1 => Ok(ModelOutput {
                text: "running ls".to_owned(),
                tool_calls: vec![ModelToolCall {
                    id: "c1".to_owned(),
                    name: "shell".to_owned(),
                    arguments: json!({ "command": "ls" }),
                }],
            }),
            2 => {
                let tool = request
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.role == ModelRole::Tool)
                    .ok_or_else(|| "missing_tool_result".to_owned())?;
                if !tool.text.contains(&self.marker) {
                    return Err(format!("tool_result_missing_stdout:{}", tool.text));
                }
                Ok(ModelOutput::text("architecture mapped"))
            }
            other => Err(format!("unexpected_model_step:{other}")),
        }
    }
}

#[tokio::test]
async fn shell_exec_stdout_is_fed_into_the_next_model_step() {
    let root = temp_project();
    fs::write(root.join("marker.txt"), "ok").unwrap();
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(Arc::new(RecordingModel {
            marker: "marker.txt".to_owned(),
            step: Mutex::new(0),
        })))
        .expect("daemon with recording model"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(trusted_metadata_in(&root), "map the architecture", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["harness"], "kiana-harness");
    assert_eq!(response.output["output"]["text"], "architecture mapped");
}

struct TimeoutModel {
    step: Mutex<u32>,
}

#[async_trait]
impl ModelClient for TimeoutModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        let mut step = self
            .step
            .lock()
            .map_err(|_| "model_lock_poisoned".to_owned())?;
        *step += 1;
        match *step {
            1 => Ok(ModelOutput {
                text: "sleeping".to_owned(),
                tool_calls: vec![ModelToolCall {
                    id: "c-timeout".to_owned(),
                    name: "shell".to_owned(),
                    arguments: json!({ "command": "sleep 8", "timeout_ms": 250 }),
                }],
            }),
            2 => {
                let tool = request
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.role == ModelRole::Tool)
                    .ok_or_else(|| "missing_tool_result".to_owned())?;
                let parsed: serde_json::Value = serde_json::from_str(&tool.text)
                    .map_err(|error| format!("tool_result_not_json:{error}:{}", tool.text))?;
                if parsed.get("timed_out") != Some(&json!(true)) {
                    return Err(format!("tool_result_missing_timed_out:{}", tool.text));
                }
                if parsed.get("exit_code") != Some(&json!(124)) {
                    return Err(format!("tool_result_missing_exit_124:{}", tool.text));
                }
                Ok(ModelOutput::text("command timed out"))
            }
            other => Err(format!("unexpected_model_step:{other}")),
        }
    }
}

#[tokio::test]
async fn shell_timeout_ms_is_brokered_as_a_timed_out_tool_result() {
    let root = temp_project();
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(Arc::new(TimeoutModel {
            step: Mutex::new(0),
        })))
        .expect("daemon with timeout model"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let started = std::time::Instant::now();
    let response = client
        .run(trusted_metadata_in(&root), "sleep then continue", None)
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["output"]["text"], "command timed out");
    assert!(
        elapsed.as_millis() < 2000,
        "brokered timeout should not wait for sleep 8, elapsed={elapsed:?}"
    );
}
