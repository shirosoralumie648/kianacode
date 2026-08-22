use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ExecutionStatus, PermissionProfile, RequestEnvelope, RequestMetadata, ResponseEnvelope,
    RoleSpec, RunId, SessionId, WorkPacket,
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

struct CapturingModel {
    inner: ScriptedModel,
    seen: Mutex<Vec<ModelRequest>>,
}

impl CapturingModel {
    fn from_json(outputs: serde_json::Value) -> Arc<Self> {
        Arc::new(Self {
            inner: ScriptedModel::from_json(&outputs).unwrap(),
            seen: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl ModelClient for CapturingModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        self.seen.lock().unwrap().push(request.clone());
        self.inner.complete(request).await
    }
}

#[test]
fn local_daemon_constructs_without_a_model() {
    let home = temp_project();
    std::env::set_var("KIANA_HOME", &home);
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
    assert_eq!(response.output["files_changed"][0], "GOLDEN_PATH.txt");
    assert_eq!(
        fs::read_to_string(root.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
}

fn apply_patch_cassette_for(path: &str) -> serde_json::Value {
    json!([
        {
            "text": "writing",
            "tool_calls": [{
                "id": "c1",
                "name": "apply_patch",
                "arguments": {
                    "patch": format!("*** Begin Patch\n*** Add File: {path}\n+hello\n*** End Patch\n")
                }
            }]
        },
        {"text": format!("created {path}")}
    ])
}

#[tokio::test]
async fn planning_pm_cannot_apply_patch_source() {
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette_for("GOLDEN_PATH.txt"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_write_metadata_in(&root);
    metadata.assign_role(&RoleSpec::pm());
    let response = client
        .run(
            metadata,
            "create a file named GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("role_path_denied"));
    assert_eq!(response.output["role_id"], "pm");
    assert_eq!(response.output["department_id"], "planning");
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn planning_pm_can_apply_patch_plan_artifact() {
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette_for("plan/WORK.md"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_write_metadata_in(&root);
    metadata.assign_role(&RoleSpec::pm());
    let response = client
        .run(
            metadata,
            "write plan/WORK.md",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["role_id"], "pm");
    assert_eq!(response.output["department_id"], "planning");
    assert_eq!(response.output["files_changed"][0], "plan/WORK.md");
    assert_eq!(
        fs::read_to_string(root.join("plan").join("WORK.md")).unwrap(),
        "hello\n"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn architect_workspace_write_is_role_sandbox_read_only() {
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette_for("plan/WORK.md"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_write_metadata_in(&root);
    metadata.assign_role(&RoleSpec::architect());
    let response = client
        .run(
            metadata,
            "write plan/WORK.md",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("role_sandbox_read_only"));
    assert!(!root.join("plan").join("WORK.md").exists());
}

#[tokio::test]
async fn unknown_role_is_rejected() {
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_write_metadata_in(&root);
    metadata.role_id = "ceo".to_owned();
    let response = client
        .run(
            metadata,
            "create GOLDEN_PATH.txt",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("role_unknown"));
    assert!(!root.join("GOLDEN_PATH.txt").exists());
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

#[tokio::test]
async fn continue_on_the_same_host_reuses_the_run() {
    let host = scripted_host(json!([
        {"text": "first turn"},
        {"text": "continued"}
    ]));
    let client = KianaClient::new(InProcessTransport { host });
    let started = client.run(trusted_metadata(), "hello", None).await.unwrap();
    assert_eq!(started.status, ExecutionStatus::Completed, "{started:?}");
    let run_id = started.output["run_id"].clone();
    let continued = client
        .continue_run(trusted_metadata(), "keep going", None, None)
        .await
        .unwrap();
    assert_eq!(
        continued.status,
        ExecutionStatus::Completed,
        "{continued:?}"
    );
    assert_eq!(continued.output["run_id"], run_id);
    assert_eq!(continued.output["output"]["text"], "continued");
    assert_eq!(continued.output["session_id"], "session-1");
}

#[tokio::test]
async fn continue_unknown_session_does_not_start_a_new_run() {
    let host = scripted_host(json!([{"text": "should not run"}]));
    let client = KianaClient::new(InProcessTransport { host });
    let continued = client
        .continue_run(trusted_metadata(), "keep going", None, None)
        .await
        .unwrap();
    assert_ne!(continued.status, ExecutionStatus::Completed);
    assert_eq!(continued.error.as_deref(), Some("session_not_found"));
}

struct CancelProbeModel;

#[async_trait]
impl ModelClient for CancelProbeModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Ok(ModelOutput {
            text: "sleeping".to_owned(),
            tool_calls: vec![ModelToolCall {
                id: "c-cancel".to_owned(),
                name: "shell".to_owned(),
                arguments: json!({
                    "command": "sh -c 'sleep 8; echo pwned > CANCELLED.txt'",
                    "timeout_ms": 20000
                }),
            }],
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_stops_in_flight_shell_before_it_writes() {
    let root = temp_project();
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(Arc::new(CancelProbeModel)))
            .expect("daemon with cancel probe"),
    );
    let runner = KianaClient::new(InProcessTransport {
        host: Arc::clone(&host),
    });
    let canceller = KianaClient::new(InProcessTransport { host });
    // EventLog sequences are per request_id; run and cancel are concurrent requests.
    let run_metadata = trusted_write_metadata_in(&root);
    let cancel_metadata = trusted_write_metadata_in(&root);
    let run_task = tokio::spawn(async move {
        runner
            .run(
                run_metadata,
                "write after sleeping",
                Some("workspace-write".to_owned()),
            )
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let cancelled = canceller
        .cancel_run(cancel_metadata, None, "user")
        .await
        .unwrap();
    assert_ne!(
        cancelled.status,
        ExecutionStatus::Completed,
        "{cancelled:?}"
    );
    assert!(
        cancelled
            .error
            .as_deref()
            .unwrap_or_default()
            .starts_with("cancelled:"),
        "{cancelled:?}"
    );
    let started = run_task.await.unwrap().unwrap();
    assert_ne!(started.status, ExecutionStatus::Completed, "{started:?}");
    assert!(!root.join("CANCELLED.txt").exists());
}

fn other_file_cassette() -> serde_json::Value {
    json!([
        {
            "text": "writing",
            "tool_calls": [{
                "id": "c2",
                "name": "apply_patch",
                "arguments": {
                    "patch": "*** Begin Patch\n*** Add File: OTHER.txt\n+world\n*** End Patch\n"
                }
            }]
        },
        {"text": "created OTHER.txt"}
    ])
}

#[tokio::test]
async fn disk_receipts_survive_restart_and_do_not_overwrite_the_first_run() {
    let root = temp_project();
    let events = root.join("sessions").join("events.jsonl");
    let first_run_id;
    {
        let host = Arc::new(
            DaemonHost::with_harness_on_disk(
                KianaHarness::new(Arc::new(
                    ScriptedModel::from_json(&apply_patch_cassette()).unwrap(),
                )),
                &events,
            )
            .expect("disk daemon"),
        );
        let client = KianaClient::new(InProcessTransport { host });
        let response = client
            .run(
                trusted_write_metadata_in(&root),
                "create GOLDEN_PATH.txt",
                Some("workspace-write".to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
        assert_eq!(response.output["files_changed"][0], "GOLDEN_PATH.txt");
        first_run_id = response.output["run_id"].as_str().unwrap().to_owned();
    }
    {
        let host = Arc::new(
            DaemonHost::with_harness_on_disk(
                KianaHarness::new(Arc::new(
                    ScriptedModel::from_json(&other_file_cassette()).unwrap(),
                )),
                &events,
            )
            .expect("second disk daemon"),
        );
        let client = KianaClient::new(InProcessTransport { host });
        let response = client
            .run(
                trusted_write_metadata_in(&root),
                "create OTHER.txt",
                Some("workspace-write".to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
        assert_eq!(response.output["files_changed"][0], "OTHER.txt");
        assert_ne!(response.output["run_id"], first_run_id);
    }
    let host = Arc::new(
        DaemonHost::with_harness_on_disk(
            KianaHarness::new(Arc::new(ScriptedModel::from_json(&json!([])).unwrap())),
            &events,
        )
        .expect("receipt daemon"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let first = client
        .receipt(
            trusted_write_metadata_in(&root),
            RunId::parse_str(&first_run_id),
        )
        .await
        .unwrap();
    assert_eq!(first.status, ExecutionStatus::Completed, "{first:?}");
    assert_eq!(first.output["run_id"], first_run_id);
    assert_eq!(first.output["files_changed"][0], "GOLDEN_PATH.txt");
    assert!(
        first.output["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["operation"] == "apply_patch"),
        "{first:?}"
    );
    assert!(root.join("GOLDEN_PATH.txt").exists());
    assert!(root.join("OTHER.txt").exists());
    let log = fs::read_to_string(&events).unwrap();
    assert!(log.contains(&first_run_id));
    assert_eq!(log.matches("run.completed").count(), 2);
}

#[tokio::test]
async fn spawn_builder_from_packet_does_not_copy_planner_transcript() {
    let root = temp_project();
    let mut outputs = apply_patch_cassette().as_array().cloned().unwrap();
    outputs.insert(0, json!({"text": "planned"}));
    let model = CapturingModel::from_json(json!(outputs));
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(model.clone())).expect("recording daemon"),
    );
    let client = KianaClient::new(InProcessTransport { host });

    let mut planner = trusted_write_metadata_in(&root);
    planner.session_id = SessionId::new("planner-1");
    planner.assign_role(&RoleSpec::pm());
    let planned = client
        .run(
            planner,
            "PLANNER_SECRET_TOKEN write a packet",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(planned.status, ExecutionStatus::Completed, "{planned:?}");

    let mut builder = trusted_write_metadata_in(&root);
    builder.session_id = SessionId::new("builder-1");
    let packet = WorkPacket::builder_task("wp-1", "create GOLDEN_PATH.txt containing hello");
    let spawned = client
        .spawn(builder, packet, Some("workspace-write".to_owned()))
        .await
        .unwrap();
    assert_eq!(spawned.status, ExecutionStatus::Completed, "{spawned:?}");
    assert_eq!(spawned.output["session_id"], "builder-1");
    assert_ne!(spawned.output["session_id"], planned.output["session_id"]);
    assert_eq!(spawned.output["role_id"], "builder");
    assert_eq!(spawned.output["department_id"], "executing");
    assert_eq!(spawned.output["work_packet_id"], "wp-1");
    assert_eq!(spawned.output["input"], "work_packet");
    assert_eq!(spawned.output["files_changed"][0], "GOLDEN_PATH.txt");
    assert_eq!(
        fs::read_to_string(root.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );

    let seen = model.seen.lock().unwrap();
    assert!(seen.len() >= 2, "{seen:?}");
    let planner_text: String = seen[0]
        .messages
        .iter()
        .filter(|message| message.role == ModelRole::User)
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        planner_text.contains("PLANNER_SECRET_TOKEN"),
        "{planner_text}"
    );
    let builder_text: String = seen
        .iter()
        .skip(1)
        .flat_map(|request| request.messages.iter())
        .filter(|message| message.role == ModelRole::User)
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !builder_text.contains("PLANNER_SECRET_TOKEN"),
        "{builder_text}"
    );
    assert!(builder_text.contains("Work packet wp-1"), "{builder_text}");
    assert!(
        builder_text.contains("Goal: create GOLDEN_PATH.txt containing hello"),
        "{builder_text}"
    );
}

#[tokio::test]
async fn spawn_reuses_of_a_live_session_fail_closed() {
    let root = temp_project();
    let host = scripted_host(json!([
        {"text": "first"},
        {"text": "should not spawn"}
    ]));
    let client = KianaClient::new(InProcessTransport { host });
    let started = client
        .run(trusted_write_metadata_in(&root), "hello", None)
        .await
        .unwrap();
    assert_eq!(started.status, ExecutionStatus::Completed, "{started:?}");
    let spawned = client
        .spawn(
            trusted_write_metadata_in(&root),
            WorkPacket::builder_task("wp-1", "create GOLDEN_PATH.txt"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(spawned.status, ExecutionStatus::Blocked, "{spawned:?}");
    assert_eq!(spawned.error.as_deref(), Some("spawn_session_not_fresh"));
}

#[tokio::test]
async fn anti_meeting_writes_decision_without_model_then_spawn() {
    let root = temp_project();
    let model = CapturingModel::from_json(apply_patch_cassette());
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(model.clone())).expect("recording daemon"),
    );
    let client = KianaClient::new(InProcessTransport { host });

    let mut chair = trusted_write_metadata_in(&root);
    chair.session_id = SessionId::new("chair-1");
    chair.assign_role(&RoleSpec::pm());
    let convened = client
        .convene(
            chair,
            "create GOLDEN_PATH.txt containing hello",
            true,
            4,
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Completed, "{convened:?}");
    assert_eq!(convened.output["builder_present"], false);
    assert_eq!(convened.output["skipped_meeting"], true);
    assert_eq!(
        convened.output["decision"]["schema"],
        "kiana.decision-record.v1"
    );
    assert_eq!(convened.output["packet"]["schema"], "kiana.work-packet.v1");
    assert!(root.join("plan/DECISION.json").exists());
    assert!(root.join("packet/TASK.json").exists());
    let seen = model.seen.lock().unwrap();
    assert!(seen.is_empty(), "{seen:?}");
    drop(seen);

    let mut builder = trusted_write_metadata_in(&root);
    builder.session_id = SessionId::new("builder-1");
    let packet: WorkPacket =
        serde_json::from_value(convened.output["packet"].clone()).expect("packet");
    let spawned = client
        .spawn(builder, packet, Some("workspace-write".to_owned()))
        .await
        .unwrap();
    assert_eq!(spawned.status, ExecutionStatus::Completed, "{spawned:?}");
    assert_eq!(spawned.output["role_id"], "builder");
    assert_eq!(
        fs::read_to_string(root.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
}

#[tokio::test]
async fn convene_then_spawn_keeps_architect_on_blackboard_not_pm_transcript() {
    let root = temp_project();
    let mut outputs = vec![
        json!({"text": "choose the vertical slice"}),
        json!({"text": "agree with one slice"}),
    ];
    outputs.extend(apply_patch_cassette().as_array().cloned().unwrap());
    let model = CapturingModel::from_json(json!(outputs));
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(model.clone())).expect("recording daemon"),
    );
    let client = KianaClient::new(InProcessTransport { host });

    let mut chair = trusted_write_metadata_in(&root);
    chair.session_id = SessionId::new("chair-1");
    chair.assign_role(&RoleSpec::pm());
    let convened = client
        .convene(
            chair,
            "one vertical slice vs two packets",
            false,
            1,
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Completed, "{convened:?}");
    assert_eq!(convened.output["builder_present"], false);
    assert_eq!(convened.output["skipped_meeting"], false);

    let seen = model.seen.lock().unwrap().clone();
    assert!(seen.len() >= 2, "{seen:?}");
    let pm_text: String = seen[0]
        .messages
        .iter()
        .filter(|message| message.role == ModelRole::User)
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(pm_text.contains("Speak as pm"), "{pm_text}");
    assert!(!pm_text.contains("PLANNER_SECRET_TOKEN"), "{pm_text}");

    let architect_text: String = seen[1]
        .messages
        .iter()
        .filter(|message| message.role == ModelRole::User)
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        architect_text.contains("Speak as architect"),
        "{architect_text}"
    );
    assert!(
        architect_text.contains("choose the vertical slice"),
        "{architect_text}"
    );
    assert!(
        architect_text.contains("Blackboard claims:"),
        "{architect_text}"
    );
    assert!(!architect_text.contains("Speak as pm"), "{architect_text}");
    assert!(
        !architect_text.contains("PLANNER_SECRET_TOKEN"),
        "{architect_text}"
    );

    let mut builder = trusted_write_metadata_in(&root);
    builder.session_id = SessionId::new("builder-1");
    let packet: WorkPacket =
        serde_json::from_value(convened.output["packet"].clone()).expect("packet");
    let spawned = client
        .spawn(builder, packet, Some("workspace-write".to_owned()))
        .await
        .unwrap();
    assert_eq!(spawned.status, ExecutionStatus::Completed, "{spawned:?}");
    assert_eq!(
        fs::read_to_string(root.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
}

#[tokio::test]
async fn review_after_builder_uses_new_session_without_model() {
    let root = temp_project();
    let model = CapturingModel::from_json(apply_patch_cassette());
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(model.clone())).expect("recording daemon"),
    );
    let client = KianaClient::new(InProcessTransport { host });

    let mut builder = trusted_write_metadata_in(&root);
    builder.session_id = SessionId::new("builder-1");
    let built = client
        .run(
            builder,
            "create GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(built.status, ExecutionStatus::Completed, "{built:?}");
    assert_eq!(built.output["role_id"], "builder");
    assert_eq!(
        fs::read_to_string(root.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
    let seen_after_build = model.seen.lock().unwrap().len();
    assert!(seen_after_build >= 1, "{seen_after_build}");

    let mut reviewer = trusted_write_metadata_in(&root);
    reviewer.session_id = SessionId::new("reviewer-1");
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = client.review(reviewer, "builder-1", None).await.unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Completed, "{reviewed:?}");
    assert_eq!(reviewed.output["schema"], "kiana.review-result.v1");
    assert_eq!(reviewed.output["role_id"], "reviewer");
    assert_eq!(reviewed.output["department_id"], "monitoring");
    assert_eq!(reviewed.output["session_id"], "reviewer-1");
    assert_eq!(reviewed.output["author_session_id"], "builder-1");
    assert_ne!(reviewed.output["session_id"], built.output["session_id"]);
    assert_eq!(reviewed.output["verdict"], "pass");
    assert_eq!(reviewed.output["files_reviewed"][0], "GOLDEN_PATH.txt");
    let packet = fs::read_to_string(root.join("gate").join("REVIEW.json")).unwrap();
    assert!(packet.contains("kiana.review-packet.v1"), "{packet}");
    assert!(packet.contains("\"verdict\": \"pass\""), "{packet}");
    let seen_after_review = model.seen.lock().unwrap().len();
    assert_eq!(seen_after_review, seen_after_build);
}

#[tokio::test]
async fn review_same_session_as_author_fails_closed() {
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let mut builder = trusted_write_metadata_in(&root);
    builder.session_id = SessionId::new("builder-1");
    let built = client
        .run(
            builder,
            "create GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(built.status, ExecutionStatus::Completed, "{built:?}");

    let mut reviewer = trusted_write_metadata_in(&root);
    reviewer.session_id = SessionId::new("builder-1");
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = client.review(reviewer, "builder-1", None).await.unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Blocked, "{reviewed:?}");
    assert_eq!(
        reviewed.error.as_deref(),
        Some("review_author_session_denied")
    );
}

fn mcp_echo_cassette() -> serde_json::Value {
    json!([
        {
            "text": "calling mcp",
            "tool_calls": [{
                "id": "c-mcp",
                "name": "mcp",
                "arguments": {
                    "server": "mock",
                    "tool": "echo",
                    "arguments": { "message": "hello" }
                }
            }]
        },
        {"text": "mcp echoed hello"}
    ])
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn write_mock_mcp_server() -> PathBuf {
    let path = temp_project().join("mock-mcp.py");
    fs::write(
        &path,
        r#"
import json
import sys

for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "mock", "version": "1.0.0"}
            }
        }), flush=True)
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {"tools": [{
                "name": "echo",
                "description": "Echo a message",
                "inputSchema": {"type": "object"}
            }]}
        }), flush=True)
    elif method == "tools/call":
        args = msg.get("params", {}).get("arguments", {})
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {"content": [{"type": "text", "text": args.get("message", "")}]}
        }), flush=True)
    else:
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "error": {"code": -32601, "message": method}
        }), flush=True)
"#,
    )
    .unwrap();
    path
}

fn python3_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok()
}

fn tool_result_text(seen: &[ModelRequest]) -> &str {
    seen.get(1)
        .and_then(|request| {
            request
                .messages
                .iter()
                .find(|message| message.role == ModelRole::Tool)
                .map(|message| message.text.as_str())
        })
        .unwrap_or("")
}

#[tokio::test]
async fn trusted_builder_stdio_mcp_echoes_through_daemon() {
    if !python3_available() {
        eprintln!("skipping MCP stdio test because python3 is unavailable");
        return;
    }
    let script = write_mock_mcp_server();
    let config = json!([{
        "name": "mock",
        "transport": "stdio",
        "command": "python3",
        "args": ["-u", script.display().to_string()]
    }]);
    let _guard = EnvGuard::set("KIANA_MCP_SERVERS_JSON", config.to_string());
    let model = CapturingModel::from_json(mcp_echo_cassette());
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(model.clone()))
            .expect("daemon with kiana harness"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let root = temp_project();
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "echo hello via mcp",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["harness"], "kiana-harness");
    assert_eq!(response.output["output"]["text"], "mcp echoed hello");
    let seen = model.seen.lock().unwrap();
    assert!(seen.len() >= 2, "expected a tool result turn: {seen:?}");
    let tool_text = tool_result_text(&seen);
    assert!(
        tool_text.contains("hello") && tool_text.contains("kiana.mcp-result.v1"),
        "{tool_text}"
    );
}

#[tokio::test]
async fn untrusted_mcp_does_not_spawn_a_server() {
    let model = CapturingModel::from_json(mcp_echo_cassette());
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(model.clone()))
            .expect("daemon with kiana harness"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let root = temp_project();
    let metadata = RequestMetadata::local("session-1", root.to_string_lossy());
    let response = client
        .run(metadata, "echo hello via mcp", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let seen = model.seen.lock().unwrap();
    let tool_text = tool_result_text(&seen);
    assert!(tool_text.contains("project_untrusted"), "{tool_text}");
}

#[tokio::test]
async fn reviewer_cannot_call_mcp() {
    let host = scripted_host(mcp_echo_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let root = temp_project();
    let mut metadata = trusted_metadata_in(&root);
    metadata.assign_role(&RoleSpec::reviewer());
    let response = client
        .run(metadata, "echo hello via mcp", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("role_tool_denied"));
    assert_eq!(response.output["role_id"], "reviewer");
    assert_eq!(response.output["department_id"], "monitoring");
    assert_eq!(response.output["sandbox"], "read-only");
}

#[tokio::test]
async fn http_mcp_is_unsupported_this_slice() {
    let config = json!([{
        "name": "mock",
        "transport": "http",
        "url": "https://example.invalid/mcp"
    }]);
    let _guard = EnvGuard::set("KIANA_MCP_SERVERS_JSON", config.to_string());
    let model = CapturingModel::from_json(mcp_echo_cassette());
    let host = Arc::new(
        DaemonHost::with_harness(KianaHarness::new(model.clone()))
            .expect("daemon with kiana harness"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let root = temp_project();
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "echo hello via mcp",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let seen = model.seen.lock().unwrap();
    let tool_text = tool_result_text(&seen);
    assert!(
        tool_text.contains("mcp_transport_unsupported"),
        "{tool_text}"
    );
}
