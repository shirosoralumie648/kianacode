use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_protocol::{
    ApprovalChallenge, ApprovalDecision, ExecutionStatus, PermissionProfile, RequestEnvelope,
    RequestMetadata, ResponseEnvelope, RoleSpec, RunId, RunStreamEvent, SessionId, WorkPacket,
};
use kiana_runner::{
    KianaHarness, ModelClient, ModelDelta, ModelOutput, ModelRequest, ModelRole, ModelToolCall,
    ScriptedModel,
};
use serde_json::json;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

struct InProcessTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for InProcessTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        Ok(self.host.handle(request).await)
    }
}

#[derive(Clone, Copy)]
struct FixedProjectTrustAuthority {
    trusted: bool,
}

impl ProjectTrustAuthority for FixedProjectTrustAuthority {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(self.trusted)
    }
}

fn harness_host(harness: KianaHarness, trusted: bool) -> Result<DaemonHost, String> {
    DaemonHost::with_harness_and_project_authority(
        harness,
        Arc::new(FixedProjectTrustAuthority { trusted }),
    )
    .map_err(|error| error.to_string())
}

fn trusted_harness_host(harness: KianaHarness) -> Result<DaemonHost, String> {
    harness_host(harness, true)
}

fn untrusted_harness_host(harness: KianaHarness) -> Result<DaemonHost, String> {
    harness_host(harness, false)
}

fn trusted_harness_host_on_disk(
    harness: KianaHarness,
    events_path: impl AsRef<Path>,
) -> Result<DaemonHost, String> {
    DaemonHost::with_harness_on_disk_and_project_authority(
        harness,
        events_path,
        Arc::new(FixedProjectTrustAuthority { trusted: true }),
    )
    .map_err(|error| error.to_string())
}

fn trusted_env_harness_host() -> Result<DaemonHost, String> {
    DaemonHost::with_env_harness_and_project_authority(Arc::new(FixedProjectTrustAuthority {
        trusted: true,
    }))
    .map_err(|error| error.to_string())
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
    Arc::new(trusted_harness_host(harness).expect("daemon with kiana harness"))
}

fn untrusted_scripted_host(outputs: serde_json::Value) -> Arc<DaemonHost> {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::from_json(&outputs).unwrap()));
    Arc::new(untrusted_harness_host(harness).expect("untrusted fixture daemon"))
}

fn compacting_host(outputs: serde_json::Value, trigger: usize, retain: usize) -> Arc<DaemonHost> {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::from_json(&outputs).unwrap()))
        .with_compact_budget(trigger, retain);
    Arc::new(trusted_harness_host(harness).expect("compacting daemon"))
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

#[derive(Debug)]
struct ChunkedModel {
    chunks: Vec<&'static str>,
}

#[async_trait]
impl ModelClient for ChunkedModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("chunked_model_complete_must_not_be_called".to_owned())
    }

    async fn complete_streaming(
        &self,
        _request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        let mut text = String::new();
        for chunk in &self.chunks {
            text.push_str(chunk);
            on_delta(ModelDelta::Text {
                text: (*chunk).to_owned(),
            })?;
        }
        Ok(ModelOutput::text(text))
    }
}

struct SplitSecretStreamingModel {
    seen: Mutex<Vec<ModelRequest>>,
    step: Mutex<u32>,
}

#[async_trait]
impl ModelClient for SplitSecretStreamingModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("split_secret_model_complete_must_not_be_called".to_owned())
    }

    async fn complete_streaming(
        &self,
        request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        self.seen.lock().unwrap().push(request);
        let mut step = self.step.lock().unwrap();
        *step += 1;
        match *step {
            1 => {
                for text in ["Authorization: Bear", "er stream-split-sentinel"] {
                    on_delta(ModelDelta::Text {
                        text: text.to_owned(),
                    })?;
                }
                Ok(ModelOutput {
                    text: "Authorization: Bearer stream-split-sentinel".to_owned(),
                    tool_calls: vec![ModelToolCall {
                        id: "c1".to_owned(),
                        name: "shell".to_owned(),
                        arguments: json!({"command": "true"}),
                    }],
                    ..ModelOutput::default()
                })
            }
            2 => Ok(ModelOutput::text("finished")),
            other => Err(format!("unexpected_model_step:{other}")),
        }
    }
}

struct HoldMidStreamModel {
    release: Notify,
    late_delta_emitted: Mutex<bool>,
}

#[async_trait]
impl ModelClient for HoldMidStreamModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("hold_mid_stream_complete_must_not_be_called".to_owned())
    }

    async fn complete_streaming(
        &self,
        _request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        on_delta(ModelDelta::Text {
            text: "before".to_owned(),
        })?;
        self.release.notified().await;
        on_delta(ModelDelta::Text {
            text: "after".to_owned(),
        })?;
        *self.late_delta_emitted.lock().unwrap() = true;
        Ok(ModelOutput::text("beforeafter"))
    }
}

#[test]
fn local_daemon_constructs_without_a_model() {
    let _env_lock = environment_lock();
    let home = temp_project();
    let _home = EnvGuard::set("KIANA_HOME", &home);
    let _host = DaemonHost::local().expect("local daemon");
}

#[tokio::test]
async fn default_host_rejects_forged_trust_without_a_stored_record() {
    let _env_lock = environment_lock();
    let home = temp_project();
    let root = temp_project();
    let _home = EnvGuard::set("KIANA_HOME", &home);
    let harness = KianaHarness::new(Arc::new(
        ScriptedModel::from_json(&apply_patch_cassette()).unwrap(),
    ));
    let host = Arc::new(DaemonHost::with_harness(harness).expect("stored-authority daemon"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = RequestMetadata::local("session-1", root.to_string_lossy());
    metadata.project_trusted = true;
    metadata.permission_profile = PermissionProfile::Autonomous;
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
async fn run_brokers_kiana_harness_tools() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(json!([
        {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
        {"text": "architecture mapped"}
    ]));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(trusted_metadata_in(&root), "map the architecture", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
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
async fn run_subscription_receives_ordered_deltas_and_terminal_response() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let harness = KianaHarness::new(Arc::new(ChunkedModel {
        chunks: vec!["alpha", " beta", " gamma"],
    }));
    let host = Arc::new(trusted_harness_host(harness).expect("streaming daemon"));
    let run_id = RunId::new();
    let mut subscription = host.subscribe_run(run_id);
    let mut metadata = trusted_metadata_in(&root);
    metadata.session_id = SessionId::new(run_id.to_string());

    let client = KianaClient::new(InProcessTransport { host });
    let response = client.run(metadata, "stream it", None).await.unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");

    for expected in ["alpha", " beta", " gamma"] {
        let envelope = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.recv())
            .await
            .expect("delta timeout")
            .expect("delta event");
        assert_eq!(
            envelope.event,
            RunStreamEvent::Delta {
                run_id,
                text: expected.to_owned(),
            }
        );
    }

    let terminal = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.recv())
        .await
        .expect("terminal timeout")
        .expect("terminal event");
    match terminal.event {
        RunStreamEvent::Terminal {
            run_id: terminal_run_id,
            response: terminal_response,
        } => {
            assert_eq!(terminal_run_id, run_id);
            assert_eq!(terminal_response.status, ExecutionStatus::Completed);
            assert_eq!(terminal_response.output["schema"], "kiana.run-result.v1");
            assert_eq!(
                terminal_response.output["output"]["text"],
                "alpha beta gamma"
            );
        }
        other => panic!("expected terminal event, got {other:?}"),
    }
}

#[tokio::test]
async fn run_without_subscription_keeps_the_complete_response_path() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let harness = KianaHarness::new(Arc::new(ChunkedModel {
        chunks: vec!["alpha", " beta", " gamma"],
    }));
    let host = Arc::new(trusted_harness_host(harness).expect("non-streaming daemon"));
    let client = KianaClient::new(InProcessTransport { host });

    let response = client
        .run(trusted_metadata_in(&root), "stream it", None)
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["output"]["text"], "alpha beta gamma");
}

#[tokio::test]
async fn legacy_run_without_subscription_keeps_the_exact_response_shape() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let harness = KianaHarness::new(Arc::new(ChunkedModel {
        chunks: vec!["legacy", " output"],
    }));
    let host = Arc::new(trusted_harness_host(harness).expect("legacy daemon"));
    let client = KianaClient::new(InProcessTransport { host });

    let response = client
        .run(trusted_metadata_in(&root), "stream it", None)
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let mut keys = serde_json::to_value(&response)
        .unwrap()
        .as_object()
        .expect("response object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    assert_eq!(
        keys,
        vec!["error", "output", "request_id", "schema", "status"]
    );
    assert_eq!(response.output["output"]["text"], "legacy output");
}

#[tokio::test]
async fn split_secret_across_stream_deltas_never_reaches_stdout_events_receipt_or_model_context() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let events = root.join("sessions").join("events.jsonl");
    let model = Arc::new(SplitSecretStreamingModel {
        seen: Mutex::new(Vec::new()),
        step: Mutex::new(0),
    });
    let host = Arc::new(
        trusted_harness_host_on_disk(KianaHarness::new(model.clone()), &events)
            .expect("disk daemon"),
    );
    let run_id = RunId::new();
    let mut metadata = trusted_metadata_in(&root);
    metadata.session_id = SessionId::new(run_id.to_string());
    let mut subscription = host.subscribe_run(run_id);
    let client = KianaClient::new(InProcessTransport { host });

    let response = client
        .run(metadata.clone(), "stream a split secret", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");

    let mut streamed = String::new();
    loop {
        let envelope = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.recv())
            .await
            .expect("stream timeout")
            .expect("stream event");
        match envelope.event {
            RunStreamEvent::Delta { text, .. } => streamed.push_str(&text),
            RunStreamEvent::Terminal { .. } => break,
            RunStreamEvent::Unknown => {}
        }
    }

    let receipt = client.receipt(metadata, Some(run_id)).await.unwrap();
    assert_eq!(receipt.status, ExecutionStatus::Completed, "{receipt:?}");
    let event_log = fs::read_to_string(&events).expect("durable event log");
    let model_context = model
        .seen
        .lock()
        .unwrap()
        .iter()
        .flat_map(|request| request.messages.iter())
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let response_text = serde_json::to_string(&response).expect("serialized response");
    let receipt_text = serde_json::to_string(&receipt).expect("serialized receipt");

    let violations = [
        ("stdout stream", streamed.as_str()),
        ("event log", event_log.as_str()),
        ("run response", response_text.as_str()),
        ("receipt", receipt_text.as_str()),
        ("next model request", model_context.as_str()),
    ]
    .into_iter()
    .filter_map(|(surface, text)| {
        text.contains("stream-split-sentinel")
            .then_some(format!("{surface}: {text}"))
    })
    .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "split secret reached: {}",
        violations.join("\n")
    );
}

#[tokio::test]
async fn cancelling_mid_stream_never_completes_or_emits_a_late_delta() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let events = root.join("sessions").join("events.jsonl");
    let model = Arc::new(HoldMidStreamModel {
        release: Notify::new(),
        late_delta_emitted: Mutex::new(false),
    });
    let host = Arc::new(
        trusted_harness_host_on_disk(KianaHarness::new(model.clone()), &events)
            .expect("disk daemon"),
    );
    let run_id = RunId::new();
    let mut metadata = trusted_metadata_in(&root);
    metadata.session_id = SessionId::new(run_id.to_string());
    let mut subscription = host.subscribe_run(run_id);
    let run_client = KianaClient::new(InProcessTransport { host: host.clone() });
    let cancel_client = KianaClient::new(InProcessTransport { host });
    let run_metadata = metadata.clone();

    let run_task = tokio::spawn(async move {
        run_client
            .run(run_metadata, "stream until cancelled", None)
            .await
            .unwrap()
    });

    let first = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.recv())
        .await
        .expect("first delta timeout")
        .expect("first delta");
    assert_eq!(
        first.event,
        RunStreamEvent::Delta {
            run_id,
            text: "before".to_owned(),
        }
    );

    let cancel = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        cancel_client.cancel_run(metadata, Some(run_id), "user"),
    )
    .await
    .expect("cancel timeout")
    .unwrap();
    model.release.notify_one();

    let response = tokio::time::timeout(std::time::Duration::from_secs(2), run_task)
        .await
        .expect("run timeout")
        .expect("run task");
    let mut late_stream = String::new();
    while let Ok(Ok(envelope)) =
        tokio::time::timeout(std::time::Duration::from_millis(50), subscription.recv()).await
    {
        match envelope.event {
            RunStreamEvent::Delta { text, .. } => late_stream.push_str(&text),
            RunStreamEvent::Terminal { .. } => break,
            RunStreamEvent::Unknown => {}
        }
    }
    let event_log = fs::read_to_string(&events).expect("durable event log");
    let mut violations = Vec::new();
    if cancel.status != ExecutionStatus::Cancelled {
        violations.push(format!("cancel status was {}", cancel.status.as_str()));
    }
    if response.status != ExecutionStatus::Cancelled {
        violations.push(format!(
            "run status after cancellation was {}",
            response.status.as_str()
        ));
    }
    if *model.late_delta_emitted.lock().unwrap() {
        violations.push("model emitted a delta after cancellation".to_owned());
    }
    if late_stream.contains("after") {
        violations.push("late delta reached the stream".to_owned());
    }
    if response.output.to_string().contains("after") {
        violations.push("late delta reached the run response".to_owned());
    }
    if event_log.contains("\"text\":\"after\"") {
        violations.push("late delta reached the event log".to_owned());
    }
    let terminal_count = [
        "\"kind\":\"run.completed\"",
        "\"kind\":\"run.failed\"",
        "\"kind\":\"run.cancelled\"",
        "\"kind\":\"run.result_unknown\"",
    ]
    .into_iter()
    .map(|needle| event_log.matches(needle).count())
    .sum::<usize>();
    if terminal_count != 1 {
        violations.push(format!("event log wrote {terminal_count} terminal events"));
    }
    if !event_log.contains("\"kind\":\"run.cancelled\"") {
        violations.push("event log did not write run.cancelled".to_owned());
    }
    assert!(
        violations.is_empty(),
        "mid-stream cancellation was not fail-closed: {}",
        violations.join("; ")
    );
}

#[tokio::test]
async fn malicious_shell_workdir_escape_is_denied_by_sandbox() {
    let _environment_lock = environment_lock();
    let parent = temp_project();
    let root = parent.join("project");
    fs::create_dir_all(&root).unwrap();
    let escaped = parent.join("escaped.txt");
    let events = parent.join("events.jsonl");
    let model = ScriptedModel::from_json(&json!([
        {
            "text": "escaping the workspace",
            "tool_calls": [{
                "id": "attack-shell",
                "name": "shell",
                "arguments": {
                    "command": "touch escaped.txt",
                    "workdir": ".."
                }
            }]
        },
        {"text": "sandbox blocked the escape"}
    ]))
    .unwrap();
    let host = Arc::new(
        trusted_harness_host_on_disk(KianaHarness::new(Arc::new(model)), &events)
            .expect("disk daemon"),
    );
    let client = KianaClient::new(InProcessTransport { host });

    let response = client
        .run(
            trusted_metadata_in(&root),
            "escape the workspace".to_owned(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert!(
        !escaped.exists(),
        "shell escaped the workspace and created {escaped:?}"
    );
    let event_log = fs::read_to_string(&events).expect("durable event log");
    assert!(event_log.contains("capability.failed"), "{event_log}");
    assert!(
        event_log.contains("harness_workdir_not_relative"),
        "{event_log}"
    );
}

#[tokio::test]
async fn trusted_workspace_write_apply_patch_creates_file() {
    let _environment_lock = environment_lock();
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
    let _environment_lock = environment_lock();
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
    let _environment_lock = environment_lock();
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
    let _environment_lock = environment_lock();
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
    let _environment_lock = environment_lock();
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
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = untrusted_scripted_host(apply_patch_cassette());
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
async fn forged_wire_trust_cannot_grant_workspace_write() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = untrusted_scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = RequestMetadata::local("session-1", root.to_string_lossy());
    metadata.project_trusted = true;
    metadata.permission_profile = PermissionProfile::Autonomous;
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
async fn forged_wire_permission_profile_cannot_bypass_read_only() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_metadata_in(&root);
    metadata.permission_profile = PermissionProfile::Autonomous;
    let response = client
        .run(
            metadata,
            "create a file named GOLDEN_PATH.txt containing hello",
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        response.status,
        ExecutionStatus::AwaitingApproval,
        "{response:?}"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn trusted_read_only_apply_patch_does_not_create_file() {
    let _environment_lock = environment_lock();
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
    assert_eq!(
        response.status,
        ExecutionStatus::AwaitingApproval,
        "{response:?}"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn cancel_after_awaiting_approval_rejects_later_approve() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let awaiting = client
        .run(
            trusted_metadata_in(&root),
            "create a file named GOLDEN_PATH.txt containing hello",
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        awaiting.status,
        ExecutionStatus::AwaitingApproval,
        "{awaiting:?}"
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(awaiting.output["approval"].clone()).unwrap();
    assert!(!root.join("GOLDEN_PATH.txt").exists());

    let cancelled = client
        .cancel_run(trusted_metadata_in(&root), None, "user")
        .await
        .unwrap();
    assert_eq!(
        cancelled.status,
        ExecutionStatus::Cancelled,
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

    let resumed = client
        .approval_decision_with_proof(
            trusted_metadata_in(&root),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
        )
        .await
        .unwrap();
    assert_eq!(resumed.status, ExecutionStatus::Blocked, "{resumed:?}");
    assert!(
        resumed
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("approval_already_consumed"),
        "{resumed:?}"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn wire_approval_proof_retry_resumes_original_run() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let awaiting = client
        .run(
            trusted_metadata_in(&root),
            "create a file named GOLDEN_PATH.txt containing hello",
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        awaiting.status,
        ExecutionStatus::AwaitingApproval,
        "{awaiting:?}"
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(awaiting.output["approval"].clone()).unwrap();

    let rejected = client
        .approval_decision_with_proof(
            trusted_metadata_in(&root),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some("sha256:wrong".to_owned()),
            Some(challenge.nonce.clone()),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status, ExecutionStatus::Blocked, "{rejected:?}");
    assert!(rejected
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("approval_request_hash_mismatch"));
    assert!(!root.join("GOLDEN_PATH.txt").exists());

    let resumed = client
        .approval_decision_with_proof(
            trusted_metadata_in(&root),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
        )
        .await
        .unwrap();
    assert_eq!(resumed.status, ExecutionStatus::Completed, "{resumed:?}");
    assert_eq!(resumed.output["sandbox"], "read-only");
    assert!(
        !root.join("GOLDEN_PATH.txt").exists(),
        "read-only approval continuation must not write: {resumed:?}"
    );
}

#[tokio::test]
async fn empty_prompt_is_blocked_before_harness_start() {
    let _environment_lock = environment_lock();
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
                ..ModelOutput::default()
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
    let _environment_lock = environment_lock();
    let root = temp_project();
    fs::write(root.join("marker.txt"), "ok").unwrap();
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(Arc::new(RecordingModel {
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

#[tokio::test]
async fn shell_secret_sentinels_do_not_reach_events_receipt_or_model_context() {
    let _environment_lock = environment_lock();
    let _secret = EnvGuard::set("KIANA_SHELL_SECRET", "env-sentinel");
    let root = temp_project();
    let events = root.join("sessions").join("events.jsonl");
    let model = CapturingModel::from_json(json!([
        {
            "text": "checking argv redaction",
            "tool_calls": [{
                "id": "c-argv",
                "name": "shell",
                "arguments": {
                    "command": ["/bin/echo", "api_key=argv-sentinel"]
                }
            }]
        },
        {
            "text": "checking output and environment redaction",
            "tool_calls": [{
                "id": "c-output",
                "name": "shell",
                "arguments": {
                    "command": r#"printf '%s\n' 'Authorization: Bearer stdout-sentinel'; printf '%s\n' 'X-Api-Key: stderr-sentinel' >&2; if [ -z "${KIANA_SHELL_SECRET+x}" ]; then printf '%s\n' env-filtered; else printenv KIANA_SHELL_SECRET; fi"#
                }
            }]
        },
        {"text": "shell complete"}
    ]));
    let host = Arc::new(
        trusted_harness_host_on_disk(KianaHarness::new(model.clone()), &events)
            .expect("disk daemon"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(trusted_metadata_in(&root), "inspect shell isolation", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");

    let run_id = response.output["run_id"]
        .as_str()
        .expect("completed run id")
        .to_owned();
    let receipt = client
        .receipt(trusted_metadata_in(&root), RunId::parse_str(&run_id))
        .await
        .unwrap();
    assert_eq!(receipt.status, ExecutionStatus::Completed, "{receipt:?}");

    let event_log = fs::read_to_string(&events).expect("durable event log");
    let model_context = model
        .seen
        .lock()
        .expect("captured model requests")
        .iter()
        .flat_map(|request| request.messages.iter())
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let response_text = serde_json::to_string(&response).expect("serialized run response");
    let receipt_text = serde_json::to_string(&receipt).expect("serialized receipt response");
    for sentinel in [
        "argv-sentinel",
        "stdout-sentinel",
        "stderr-sentinel",
        "env-sentinel",
    ] {
        assert!(
            !event_log.contains(sentinel),
            "event log leaked {sentinel}: {event_log}"
        );
        assert!(
            !model_context.contains(sentinel),
            "model context leaked {sentinel}: {model_context}"
        );
        assert!(
            !response_text.contains(sentinel),
            "run response leaked {sentinel}: {response_text}"
        );
        assert!(
            !receipt_text.contains(sentinel),
            "receipt leaked {sentinel}: {receipt_text}"
        );
    }
    assert!(event_log.contains("[REDACTED]"), "{event_log}");
    assert!(model_context.contains("[REDACTED]"), "{model_context}");
    assert!(model_context.contains("env-filtered"), "{model_context}");
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
                ..ModelOutput::default()
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
                if parsed.get("stop_confirmed") != Some(&json!(true)) {
                    return Err(format!(
                        "tool_result_missing_stop_confirmation:{}",
                        tool.text
                    ));
                }
                Ok(ModelOutput::text("command timed out"))
            }
            other => Err(format!("unexpected_model_step:{other}")),
        }
    }
}

#[tokio::test]
async fn shell_timeout_ms_is_brokered_as_a_timed_out_tool_result() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(Arc::new(TimeoutModel {
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
    let _environment_lock = environment_lock();
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
    let _environment_lock = environment_lock();
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
            ..ModelOutput::default()
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_stops_in_flight_shell_before_it_writes() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(Arc::new(CancelProbeModel)))
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
    assert_eq!(
        cancelled.status,
        ExecutionStatus::Cancelled,
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
    let _environment_lock = environment_lock();
    let root = temp_project();
    let events = root.join("sessions").join("events.jsonl");
    let first_run_id;
    {
        let host = Arc::new(
            trusted_harness_host_on_disk(
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
            trusted_harness_host_on_disk(
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
        trusted_harness_host_on_disk(
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
    let foreign = client
        .receipt(
            RequestMetadata::local("foreign-session", root.to_string_lossy()),
            RunId::parse_str(&first_run_id),
        )
        .await
        .unwrap();
    assert_eq!(foreign.status, ExecutionStatus::Blocked, "{foreign:?}");
    assert_eq!(foreign.error.as_deref(), Some("run_owner_mismatch"));
    assert!(
        foreign.output.is_null(),
        "foreign receipt leaked: {foreign:?}"
    );
    let log = fs::read_to_string(&events).unwrap();
    assert!(log.contains(&first_run_id));
    let completed_count = log
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event["kind"] == "run.completed")
        .count();
    assert_eq!(completed_count, 2);
}

#[tokio::test]
async fn spawn_builder_from_packet_does_not_copy_planner_transcript() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let mut outputs = apply_patch_cassette().as_array().cloned().unwrap();
    outputs.insert(0, json!({"text": "planned"}));
    let model = CapturingModel::from_json(json!(outputs));
    let host =
        Arc::new(trusted_harness_host(KianaHarness::new(model.clone())).expect("recording daemon"));
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
    let _environment_lock = environment_lock();
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

struct RoutingModel;

#[async_trait]
impl ModelClient for RoutingModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        let blob: String = request
            .messages
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let file = if blob.contains("ALPHA.txt") {
            "ALPHA.txt"
        } else if blob.contains("BRAVO.txt") {
            "BRAVO.txt"
        } else {
            return Err(format!("unexpected_prompt:{blob}"));
        };
        let has_tool = request
            .messages
            .iter()
            .any(|message| message.role == ModelRole::Tool);
        if has_tool {
            Ok(ModelOutput::text(format!("created {file}")))
        } else {
            Ok(ModelOutput::with_tool(
                format!("writing {file}"),
                "apply_patch",
                json!({
                    "patch": format!("*** Begin Patch\n*** Add File: {file}\n+hello\n*** End Patch\n")
                }),
            ))
        }
    }
}

struct HoldFirstModel {
    inner: ScriptedModel,
    entered: Notify,
    release: Notify,
    held: Mutex<bool>,
}

#[async_trait]
impl ModelClient for HoldFirstModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        let should_hold = {
            let mut held = self.held.lock().unwrap();
            if !*held {
                *held = true;
                true
            } else {
                false
            }
        };
        if should_hold {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.inner.complete(request).await
    }
}

#[tokio::test]
async fn disjoint_packet_builders_write_in_parallel() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(Arc::new(RoutingModel))).expect("routing daemon"),
    );
    let client = KianaClient::new(InProcessTransport { host });

    let mut alpha = trusted_write_metadata_in(&root);
    alpha.session_id = SessionId::new("builder-a");
    let mut bravo = trusted_write_metadata_in(&root);
    bravo.session_id = SessionId::new("builder-b");
    let (alpha_done, bravo_done) = tokio::join!(
        client.spawn(
            alpha,
            WorkPacket::builder_task("wp-a", "create ALPHA.txt containing hello")
                .with_path_allow(["ALPHA.txt"]),
            Some("workspace-write".to_owned()),
        ),
        client.spawn(
            bravo,
            WorkPacket::builder_task("wp-b", "create BRAVO.txt containing hello")
                .with_path_allow(["BRAVO.txt"]),
            Some("workspace-write".to_owned()),
        ),
    );
    let alpha_done = alpha_done.unwrap();
    let bravo_done = bravo_done.unwrap();
    assert_eq!(
        alpha_done.status,
        ExecutionStatus::Completed,
        "{alpha_done:?}"
    );
    assert_eq!(
        bravo_done.status,
        ExecutionStatus::Completed,
        "{bravo_done:?}"
    );
    assert_eq!(alpha_done.output["session_id"], "builder-a");
    assert_eq!(bravo_done.output["session_id"], "builder-b");
    assert_eq!(alpha_done.output["work_packet_id"], "wp-a");
    assert_eq!(bravo_done.output["work_packet_id"], "wp-b");
    assert_eq!(
        fs::read_to_string(root.join("ALPHA.txt")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("BRAVO.txt")).unwrap(),
        "hello\n"
    );
}

#[tokio::test]
async fn overlapping_live_packet_spawns_fail_closed() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let model = Arc::new(HoldFirstModel {
        inner: ScriptedModel::from_json(&apply_patch_cassette_for("ALPHA.txt")).unwrap(),
        entered: Notify::new(),
        release: Notify::new(),
        held: Mutex::new(false),
    });
    let host =
        Arc::new(trusted_harness_host(KianaHarness::new(model.clone())).expect("holding daemon"));
    let first_client = KianaClient::new(InProcessTransport { host: host.clone() });
    let client = KianaClient::new(InProcessTransport { host });

    let mut first = trusted_write_metadata_in(&root);
    first.session_id = SessionId::new("builder-a");
    let entered_wait = model.entered.notified();
    let first_task = tokio::spawn(async move {
        first_client
            .spawn(
                first,
                WorkPacket::builder_task("wp-a", "create ALPHA.txt containing hello")
                    .with_path_allow(["ALPHA.txt"]),
                Some("workspace-write".to_owned()),
            )
            .await
    });
    entered_wait.await;

    let mut second = trusted_write_metadata_in(&root);
    second.session_id = SessionId::new("builder-b");
    let blocked = client
        .spawn(
            second,
            WorkPacket::builder_task("wp-b", "also ALPHA.txt").with_path_allow(["ALPHA.txt"]),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status, ExecutionStatus::Blocked, "{blocked:?}");
    assert_eq!(blocked.error.as_deref(), Some("path_lock_conflict"));

    model.release.notify_one();
    let first_done = first_task.await.unwrap().unwrap();
    assert_eq!(
        first_done.status,
        ExecutionStatus::Completed,
        "{first_done:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("ALPHA.txt")).unwrap(),
        "hello\n"
    );
}

#[tokio::test]
async fn packet_path_allow_blocks_writes_outside_the_packet() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette_for("GOLDEN_PATH.txt"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut builder = trusted_write_metadata_in(&root);
    builder.session_id = SessionId::new("builder-a");
    let denied = client
        .spawn(
            builder,
            WorkPacket::builder_task("wp-a", "create GOLDEN_PATH.txt containing hello")
                .with_path_allow(["ALPHA.txt"]),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(denied.status, ExecutionStatus::Failed, "{denied:?}");
    assert_eq!(denied.error.as_deref(), Some("packet_path_denied"));
    assert!(!root.join("GOLDEN_PATH.txt").exists());
    assert!(!root.join("ALPHA.txt").exists());
}

#[tokio::test]
async fn anti_meeting_writes_decision_without_model_then_spawn() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let model = CapturingModel::from_json(apply_patch_cassette());
    let host =
        Arc::new(trusted_harness_host(KianaHarness::new(model.clone())).expect("recording daemon"));
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
    let _environment_lock = environment_lock();
    let root = temp_project();
    let mut outputs = vec![
        json!({"text": "choose the vertical slice"}),
        json!({"text": "agree with one slice"}),
    ];
    outputs.extend(apply_patch_cassette().as_array().cloned().unwrap());
    let model = CapturingModel::from_json(json!(outputs));
    let host =
        Arc::new(trusted_harness_host(KianaHarness::new(model.clone())).expect("recording daemon"));
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
async fn each_department_can_convene_on_daemon_host() {
    let _environment_lock = environment_lock();
    let cases = [
        (
            RoleSpec::sponsor(),
            "initiating",
            "charter/DECISION.json",
            false,
        ),
        (RoleSpec::pm(), "planning", "plan/DECISION.json", true),
        (
            RoleSpec::builder(),
            "executing",
            "receipt/DECISION.json",
            false,
        ),
        (
            RoleSpec::reviewer(),
            "monitoring",
            "gate/DECISION.json",
            false,
        ),
        (
            RoleSpec::closer(),
            "closing",
            "lessons/DECISION.json",
            false,
        ),
    ];
    for (role, department, decision_path, emits_packet) in cases {
        let root = temp_project();
        let model = CapturingModel::from_json(json!([]));
        let host = Arc::new(
            trusted_harness_host(KianaHarness::new(model.clone())).expect("recording daemon"),
        );
        let client = KianaClient::new(InProcessTransport { host });
        let mut chair = trusted_write_metadata_in(&root);
        chair.session_id = SessionId::new(format!("{department}-chair"));
        chair.assign_role(&role);
        let convened = client
            .convene(
                chair,
                "decide the next slice",
                true,
                4,
                Some("workspace-write".to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(
            convened.status,
            ExecutionStatus::Completed,
            "{department} {convened:?}"
        );
        assert_eq!(convened.output["department_id"], department);
        assert_eq!(
            convened.output["builder_present"],
            department == "executing"
        );
        assert!(root.join(decision_path).exists(), "{department}");
        assert_eq!(root.join("packet/TASK.json").exists(), emits_packet);
        if department != "planning" {
            assert!(!root.join("plan/DECISION.json").exists(), "{department}");
        }
        assert!(model.seen.lock().unwrap().is_empty());
    }

    let root = temp_project();
    let host = scripted_host(json!([{"text": "should not run"}]));
    let client = KianaClient::new(InProcessTransport { host });
    let mut chair = trusted_write_metadata_in(&root);
    chair.assign_role(&RoleSpec::architect());
    let convened = client
        .convene(
            chair,
            "decide the next slice",
            true,
            4,
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Blocked, "{convened:?}");
    assert_eq!(
        convened.error.as_deref(),
        Some("symposium_chair_must_be_pm")
    );
    assert!(!root.join("plan/DECISION.json").exists());
}

#[tokio::test]
async fn review_after_builder_uses_new_session_without_model() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let model = CapturingModel::from_json(apply_patch_cassette());
    let host =
        Arc::new(trusted_harness_host(KianaHarness::new(model.clone())).expect("recording daemon"));
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
async fn closing_closer_writes_receipt_after_independent_review() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });

    let mut builder = trusted_write_metadata_in(&root);
    builder.session_id = SessionId::new("builder-close-1");
    let built = client
        .run(
            builder,
            "create GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(built.status, ExecutionStatus::Completed, "{built:?}");
    let author_run_id = RunId::parse_str(built.output["run_id"].as_str().unwrap());

    let mut reviewer = trusted_write_metadata_in(&root);
    reviewer.session_id = SessionId::new("reviewer-close-1");
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = client
        .review(reviewer, "builder-close-1", author_run_id)
        .await
        .unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Completed, "{reviewed:?}");

    let mut closer = trusted_write_metadata_in(&root);
    closer.session_id = SessionId::new("closer-1");
    closer.assign_role(&RoleSpec::closer());
    let closed = client
        .close(closer, "builder-close-1", author_run_id)
        .await
        .unwrap();
    assert_eq!(closed.status, ExecutionStatus::Completed, "{closed:?}");
    assert_eq!(closed.output["schema"], "kiana.closing-receipt.v1");
    assert_eq!(closed.output["author_session_id"], "builder-close-1");
    assert_eq!(closed.output["reviewer_session_id"], "reviewer-close-1");
    assert_eq!(closed.output["closer_session_id"], "closer-1");
    assert_eq!(closed.output["accepted"], true);
    assert_eq!(
        fs::read_to_string(root.join("lessons").join("LEARNED.md"))
            .unwrap()
            .contains("GOLDEN_PATH.txt"),
        true
    );
    assert!(root.join("lessons").join("CLOSING.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn closing_rejects_symlinked_review_and_merge_artifacts() {
    let _environment_lock = environment_lock();
    for (name, expected_error) in [
        ("REVIEW.json", "close_review_not_found"),
        ("MERGE.json", "close_merge_receipt_not_found"),
    ] {
        let root = temp_project();
        let host = scripted_host(apply_patch_cassette());
        let client = KianaClient::new(InProcessTransport { host });

        let mut builder = trusted_write_metadata_in(&root);
        builder.session_id = SessionId::new(format!("builder-symlink-{name}"));
        let author_session_id = builder.session_id.to_string();
        let built = client
            .run(
                builder,
                "create GOLDEN_PATH.txt containing hello",
                Some("workspace-write".to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(built.status, ExecutionStatus::Completed, "{built:?}");
        let author_run_id = RunId::parse_str(built.output["run_id"].as_str().unwrap());

        let mut reviewer = trusted_write_metadata_in(&root);
        reviewer.session_id = SessionId::new(format!("reviewer-symlink-{name}"));
        reviewer.assign_role(&RoleSpec::reviewer());
        let reviewed = client
            .review(reviewer, &author_session_id, author_run_id)
            .await
            .unwrap();
        assert_eq!(reviewed.status, ExecutionStatus::Completed, "{reviewed:?}");

        let gate_artifact = root.join("gate").join(name);
        let outside = temp_project().join(format!("outside-{name}"));
        fs::rename(&gate_artifact, &outside).unwrap();
        let outside_before = fs::read_to_string(&outside).unwrap();
        symlink(&outside, &gate_artifact).unwrap();

        let mut closer = trusted_write_metadata_in(&root);
        closer.session_id = SessionId::new(format!("closer-symlink-{name}"));
        closer.assign_role(&RoleSpec::closer());
        let closed = client
            .close(closer, &author_session_id, author_run_id)
            .await
            .unwrap();

        assert_eq!(closed.status, ExecutionStatus::Blocked, "{name} {closed:?}");
        assert_eq!(closed.error.as_deref(), Some(expected_error));
        assert_eq!(fs::read_to_string(&outside).unwrap(), outside_before);
        assert!(!root.join("lessons").join("CLOSING.json").exists());
        assert!(!root.join("lessons").join("LEARNED.md").exists());
    }
}

#[tokio::test]
async fn closing_without_a_passing_review_fails_closed() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let mut builder = trusted_write_metadata_in(&root);
    builder.session_id = SessionId::new("builder-close-2");
    let built = client
        .run(
            builder,
            "create GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    let mut closer = trusted_write_metadata_in(&root);
    closer.session_id = SessionId::new("closer-2");
    closer.assign_role(&RoleSpec::closer());
    let closed = client
        .close(
            closer,
            "builder-close-2",
            RunId::parse_str(built.output["run_id"].as_str().unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(closed.status, ExecutionStatus::Blocked, "{closed:?}");
    assert_eq!(closed.error.as_deref(), Some("close_review_not_found"));
    assert!(!root.join("lessons").join("CLOSING.json").exists());
}

#[tokio::test]
async fn review_same_session_as_author_fails_closed() {
    let _environment_lock = environment_lock();
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

fn mcp_unknown_tool_cassette() -> serde_json::Value {
    json!([
        {
            "text": "calling unknown mcp tool",
            "tool_calls": [{
                "id": "c-mcp-unknown",
                "name": "mcp",
                "arguments": {
                    "server": "mock",
                    "tool": "not-advertised",
                    "arguments": { "message": "must not execute" }
                }
            }]
        },
        {"text": "unknown tool rejected"}
    ])
}

static ENVIRONMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn environment_lock() -> MutexGuard<'static, ()> {
    ENVIRONMENT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
import os
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
        marker = os.environ.get("MCP_CALL_MARKER")
        if marker:
            with open(marker, "w", encoding="utf-8") as marker_file:
                marker_file.write("called")
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
async fn wire_actor_metadata_is_server_stamped() {
    let _environment_lock = environment_lock();
    let host = scripted_host(json!([{"text": "ok"}]));
    let root = temp_project();
    let mut metadata = trusted_metadata_in(&root);
    metadata.actor_id = Some("caller-controlled".to_owned());
    let response = host
        .handle(RequestEnvelope::run(
            metadata,
            "hello",
            Some("read-only".to_owned()),
        ))
        .await;
    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.output["actor_id"], "local-user");
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn wire_explicit_run_id_cannot_cross_session_owner() {
    let _environment_lock = environment_lock();
    let host = scripted_host(json!([{"text": "ok"}]));
    let root = temp_project();
    let first = host
        .handle(RequestEnvelope::run(
            trusted_metadata_in(&root),
            "hello",
            Some("read-only".to_owned()),
        ))
        .await;
    let run_id: RunId = serde_json::from_value(first.output["run_id"].clone()).unwrap();
    let mut other = trusted_metadata_in(&root);
    other.session_id = SessionId::new("other-session");
    let response = host
        .handle(RequestEnvelope::continue_run(
            other,
            "continue",
            Some("read-only".to_owned()),
            Some(run_id),
        ))
        .await;
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("session_not_found"));
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn trusted_builder_stdio_mcp_echoes_through_daemon() {
    let _env_lock = environment_lock();
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
        trusted_harness_host(KianaHarness::new(model.clone())).expect("daemon with kiana harness"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let root = temp_project();
    let awaiting = client
        .run(
            trusted_write_metadata_in(&root),
            "echo hello via mcp",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(
        awaiting.status,
        ExecutionStatus::AwaitingApproval,
        "{awaiting:?}"
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(awaiting.output["approval"].clone()).unwrap();
    let response = client
        .approval_decision_with_proof(
            trusted_write_metadata_in(&root),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
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
async fn unknown_stdio_mcp_tool_is_rejected_before_tools_call() {
    let _env_lock = environment_lock();
    if !python3_available() {
        eprintln!("skipping MCP stdio test because python3 is unavailable");
        return;
    }
    let script = write_mock_mcp_server();
    let marker = script.with_extension("called");
    let config = json!([{
        "name": "mock",
        "transport": "stdio",
        "command": "python3",
        "args": ["-u", script.display().to_string()],
        "env": { "MCP_CALL_MARKER": marker.display().to_string() }
    }]);
    let _guard = EnvGuard::set("KIANA_MCP_SERVERS_JSON", config.to_string());
    let model = CapturingModel::from_json(mcp_unknown_tool_cassette());
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(model.clone())).expect("daemon with kiana harness"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let root = temp_project();
    let awaiting = client
        .run(
            trusted_write_metadata_in(&root),
            "call an unknown mcp tool",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(
        awaiting.status,
        ExecutionStatus::AwaitingApproval,
        "{awaiting:?}"
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(awaiting.output["approval"].clone()).unwrap();
    let response = client
        .approval_decision_with_proof(
            trusted_write_metadata_in(&root),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let seen = model.seen.lock().unwrap();
    let tool_text = tool_result_text(&seen);
    assert!(tool_text.contains("mcp_tool_unknown"), "{tool_text}");
    assert!(!marker.exists(), "unknown tool reached tools/call");
}

#[tokio::test]
async fn untrusted_mcp_does_not_spawn_a_server() {
    let _environment_lock = environment_lock();
    let model = CapturingModel::from_json(mcp_echo_cassette());
    let host = Arc::new(
        untrusted_harness_host(KianaHarness::new(model.clone()))
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
    let _environment_lock = environment_lock();
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
    let _env_lock = environment_lock();
    let config = json!([{
        "name": "mock",
        "transport": "http",
        "url": "https://example.invalid/mcp"
    }]);
    let _guard = EnvGuard::set("KIANA_MCP_SERVERS_JSON", config.to_string());
    let model = CapturingModel::from_json(mcp_echo_cassette());
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(model.clone())).expect("daemon with kiana harness"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let root = temp_project();
    let awaiting = client
        .run(
            trusted_write_metadata_in(&root),
            "echo hello via mcp",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(
        awaiting.status,
        ExecutionStatus::AwaitingApproval,
        "{awaiting:?}"
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(awaiting.output["approval"].clone()).unwrap();
    let response = client
        .approval_decision_with_proof(
            trusted_write_metadata_in(&root),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
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

fn write_kiana_project_skill(root: &Path, name: &str) {
    let dir = root.join(".kiana").join("skills").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\ndescription: {name} fixture\n---\n{name} body\n"),
    )
    .unwrap();
}

fn text_only_cassette() -> serde_json::Value {
    json!([{ "text": "architecture mapped" }])
}

fn first_system_text(seen: &[ModelRequest]) -> &str {
    seen.first()
        .and_then(|request| request.messages.first())
        .filter(|message| message.role == ModelRole::System)
        .map(|message| message.text.as_str())
        .unwrap_or("")
}

#[tokio::test]
async fn trusted_project_skill_appears_in_harness_system_message() {
    let _environment_lock = environment_lock();
    kiana_skills::clear_caches();
    let root = temp_project();
    write_kiana_project_skill(&root, "code03-harness-demo");
    let model = CapturingModel::from_json(text_only_cassette());
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(model.clone())).expect("daemon with kiana harness"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(trusted_metadata_in(&root), "map the architecture", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let seen = model.seen.lock().unwrap();
    let system_text = first_system_text(&seen);
    assert!(system_text.contains("code03-harness-demo"), "{system_text}");
}

#[tokio::test]
async fn untrusted_project_skill_is_withheld_from_harness_system_message() {
    let _environment_lock = environment_lock();
    kiana_skills::clear_caches();
    let root = temp_project();
    write_kiana_project_skill(&root, "code03-harness-demo");
    let model = CapturingModel::from_json(text_only_cassette());
    let host = Arc::new(
        untrusted_harness_host(KianaHarness::new(model.clone()))
            .expect("daemon with kiana harness"),
    );
    let client = KianaClient::new(InProcessTransport { host });
    let metadata = RequestMetadata::local("session-1", root.to_string_lossy());
    let response = client
        .run(metadata, "map the architecture", None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let seen = model.seen.lock().unwrap();
    let blob: String = seen
        .iter()
        .flat_map(|request| request.messages.iter())
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !blob.contains("code03-harness-demo"),
        "untrusted project skill leaked: {blob}"
    );
}

#[tokio::test]
async fn pre_tool_use_hook_blocks_apply_patch_before_broker_execute() {
    let _environment_lock = environment_lock();
    let _guard = EnvGuard::set(
        "KIANA_PRE_TOOL_USE_HOOKS",
        r#"printf '%s' '{"decision":"block","reason":"policy failed"}'"#,
    );
    let root = temp_project();
    let model = CapturingModel::from_json(apply_patch_cassette());
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(model.clone())).expect("daemon with kiana harness"),
    );
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
    assert!(
        !root.join("GOLDEN_PATH.txt").exists(),
        "hook must prevent apply_patch side effect"
    );
    let seen = model.seen.lock().unwrap();
    let tool_text = tool_result_text(&seen);
    assert!(tool_text.contains("hook_blocked"), "{tool_text}");
}

#[tokio::test]
async fn pre_tool_use_hook_update_input_fails_closed_before_broker_execute() {
    let _environment_lock = environment_lock();
    let _guard = EnvGuard::set(
        "KIANA_PRE_TOOL_USE_HOOKS",
        r#"printf '%s' '{"decision":"allow","updated_input":{"path":"safe.txt"}}'"#,
    );
    let root = temp_project();
    let model = CapturingModel::from_json(apply_patch_cassette());
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(model.clone())).expect("daemon with kiana harness"),
    );
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
    assert!(!root.join("GOLDEN_PATH.txt").exists());
    let seen = model.seen.lock().unwrap();
    assert!(tool_result_text(&seen).contains("hook_update_input_unapplied"));
}

#[tokio::test]
async fn pre_tool_use_hook_ask_fails_closed_before_broker_execute() {
    let _environment_lock = environment_lock();
    let _guard = EnvGuard::set(
        "KIANA_PRE_TOOL_USE_HOOKS",
        r#"printf '%s' '{"decision":"ask","reason":"confirm"}'"#,
    );
    let root = temp_project();
    let model = CapturingModel::from_json(apply_patch_cassette());
    let host = Arc::new(
        trusted_harness_host(KianaHarness::new(model.clone())).expect("daemon with kiana harness"),
    );
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
    assert!(!root.join("GOLDEN_PATH.txt").exists());
    let seen = model.seen.lock().unwrap();
    assert!(tool_result_text(&seen).contains("hook_ask_unattended:confirm"));
}

#[tokio::test]
async fn initiating_sponsor_can_write_charter_but_not_source() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette_for("charter/GOAL.md"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_write_metadata_in(&root);
    metadata.assign_role(&RoleSpec::sponsor());
    let response = client
        .run(
            metadata,
            "write charter/GOAL.md",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["role_id"], "sponsor");
    assert_eq!(response.output["department_id"], "initiating");
    assert_eq!(response.output["files_changed"][0], "charter/GOAL.md");
    assert_eq!(
        fs::read_to_string(root.join("charter").join("GOAL.md")).unwrap(),
        "hello\n"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());

    let host = scripted_host(apply_patch_cassette_for("GOLDEN_PATH.txt"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_write_metadata_in(&root);
    metadata.assign_role(&RoleSpec::sponsor());
    let denied = client
        .run(
            metadata,
            "create a file named GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(denied.status, ExecutionStatus::Failed, "{denied:?}");
    assert_eq!(denied.error.as_deref(), Some("role_path_denied"));
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn closing_closer_can_write_lessons_but_not_source() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(apply_patch_cassette_for("lessons/LEARNED.md"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_write_metadata_in(&root);
    metadata.assign_role(&RoleSpec::closer());
    let response = client
        .run(
            metadata,
            "write lessons/LEARNED.md",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["role_id"], "closer");
    assert_eq!(response.output["department_id"], "closing");
    assert_eq!(response.output["files_changed"][0], "lessons/LEARNED.md");
    assert_eq!(
        fs::read_to_string(root.join("lessons").join("LEARNED.md")).unwrap(),
        "hello\n"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());

    let host = scripted_host(apply_patch_cassette_for("GOLDEN_PATH.txt"));
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = trusted_write_metadata_in(&root);
    metadata.assign_role(&RoleSpec::closer());
    let denied = client
        .run(
            metadata,
            "create a file named GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(denied.status, ExecutionStatus::Failed, "{denied:?}");
    assert_eq!(denied.error.as_deref(), Some("role_path_denied"));
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

fn memory_search_cassette(collection: &str, query: &str) -> serde_json::Value {
    json!([
        {
            "text": "searching",
            "tool_calls": [{
                "id": "c-mem",
                "name": "memory.search",
                "arguments": { "query": query, "collection": collection }
            }]
        },
        { "text": "searched" }
    ])
}

fn memory_write_cassette(collection: &str, text: &str, source: &str) -> serde_json::Value {
    json!([
        {
            "text": "writing memory",
            "tool_calls": [{
                "id": "c-mem-w",
                "name": "memory.write",
                "arguments": {
                    "collection": collection,
                    "text": text,
                    "source": source
                }
            }]
        },
        { "text": "wrote" }
    ])
}

fn seed_memory_record(path: &Path, collection: &str, layer: &str, text: &str, source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let record = json!({
        "schema": "kiana.memory-record.v1",
        "id": "seed-1",
        "layer": layer,
        "collection": collection,
        "text": text,
        "source": source,
        "role_id": "builder",
        "department_id": "executing",
        "session_id": "seed",
        "created_at_ms": 1
    });
    fs::write(path, format!("{record}\n")).unwrap();
}

fn project_memory_path(root: &Path) -> PathBuf {
    root.join(".kiana")
        .join("memory")
        .join("project")
        .join("project.jsonl")
}

#[tokio::test]
async fn builder_project_search_hits_land_on_receipt() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    seed_memory_record(
        &project_memory_path(&root),
        "project",
        "project",
        "acceptance is GOLDEN_PATH.txt contains hello",
        "docs/acceptance.md",
    );
    let host = scripted_host(memory_search_cassette("project", "acceptance"));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "search project memory for acceptance",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["role_id"], "builder");
    assert_eq!(response.output["department_id"], "executing");
    let hits = response.output["memory_hits"].as_array().cloned().unwrap();
    assert_eq!(hits.len(), 1, "{response:?}");
    assert_eq!(hits[0]["layer"], "project");
    assert_eq!(hits[0]["collection"], "project");
    assert_eq!(hits[0]["source"], "docs/acceptance.md");
    assert_eq!(hits[0]["verified"], true);
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

fn spoofed_pm_memory_search_cassette() -> serde_json::Value {
    json!([
        {
            "text": "searching",
            "tool_calls": [{
                "id": "c-mem-spoof",
                "name": "memory.search",
                "arguments": {
                    "query": "secret plan",
                    "collection": "planning:unreleased-debate",
                    "role_id": "pm",
                    "department_id": "planning"
                }
            }]
        },
        { "text": "searched" }
    ])
}

#[tokio::test]
async fn model_role_arguments_cannot_widen_memory_grants() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(spoofed_pm_memory_search_cassette());
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "search unpublished planning debate",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("role_knowledge_denied"));
}

#[tokio::test]
async fn builder_cannot_search_user_private_memory() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(memory_search_cassette("user-private", "secret"));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "search private user memory",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("role_knowledge_denied"));
}

#[tokio::test]
async fn builder_cannot_search_unreleased_planning_debate() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(memory_search_cassette(
        "planning:unreleased-debate",
        "secret plan",
    ));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "search unpublished planning debate",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("role_knowledge_denied"));
}

#[tokio::test]
async fn builder_scratch_write_does_not_promote_to_project() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(memory_write_cassette(
        "instance-scratch",
        "secret draft stays scratch",
        "explicit:test",
    ));
    let client = KianaClient::new(InProcessTransport { host });
    let written = client
        .run(
            trusted_write_metadata_in(&root),
            "remember this draft",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(written.status, ExecutionStatus::Completed, "{written:?}");
    assert_eq!(written.output["role_id"], "builder");
    let scratch = root
        .join(".kiana")
        .join("memory")
        .join("instance")
        .join("session-1.jsonl");
    assert!(scratch.exists(), "scratch file missing");
    assert!(!project_memory_path(&root).exists());

    let host = scripted_host(memory_search_cassette("project", "secret draft"));
    let client = KianaClient::new(InProcessTransport { host });
    let searched = client
        .run(
            trusted_write_metadata_in(&root),
            "search project memory for the draft",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(searched.status, ExecutionStatus::Completed, "{searched:?}");
    let hits = searched.output["memory_hits"].as_array().cloned().unwrap();
    assert!(hits.is_empty(), "{searched:?}");
}

#[tokio::test]
async fn builder_cannot_write_project_memory() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = scripted_host(memory_write_cassette(
        "project",
        "should not land",
        "explicit:test",
    ));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "write project memory",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("role_memory_write_denied"));
    assert!(!project_memory_path(&root).exists());
}

#[tokio::test]
async fn unsourced_memory_hit_is_not_verified() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    seed_memory_record(
        &project_memory_path(&root),
        "project",
        "project",
        "acceptance without a source",
        "",
    );
    let host = scripted_host(memory_search_cassette("project", "acceptance"));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "search unsourced memory",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let hits = response.output["memory_hits"].as_array().cloned().unwrap();
    assert_eq!(hits.len(), 1, "{response:?}");
    assert_eq!(hits[0]["verified"], false);
    assert_eq!(hits[0]["source"], "");
}

#[tokio::test]
async fn fake_text_only_provider_fails_closed_with_unsupported_tools() {
    let _environment_lock = environment_lock();
    let _provider = EnvGuard::set("KIANA_PROVIDER", "fake");
    let _model = EnvGuard::set("KIANA_FAKE_MODEL", "fake-text-only");
    let _script = EnvGuard::set("KIANA_HARNESS_SCRIPT", "");
    let root = temp_project();
    let host = Arc::new(trusted_env_harness_host().expect("env harness daemon"));
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(
            trusted_write_metadata_in(&root),
            "create a file named GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed, "{response:?}");
    let error = response.error.as_deref().unwrap_or("");
    assert!(
        error.contains("unsupported_tools"),
        "expected unsupported_tools, got {response:?}"
    );
    assert!(
        !root.join("GOLDEN_PATH.txt").exists(),
        "text-only provider must not write GOLDEN_PATH.txt"
    );
}

#[tokio::test]
async fn over_budget_run_records_compact_on_receipt() {
    let _environment_lock = environment_lock();
    let host = compacting_host(json!([{"text": "compacted first turn"}]), 200, 40);
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .run(trusted_metadata(), "x".repeat(2000), None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let compact = &response.output["compact"];
    assert_eq!(compact["applied"], true, "{response:?}");
    assert!(compact["count"].as_u64().unwrap_or(0) >= 1, "{compact}");
    assert_eq!(compact["summary_present"], true, "{compact}");
    let before = compact["tokens_before"].as_u64().unwrap_or(0);
    let after = compact["tokens_after"].as_u64().unwrap_or(0);
    assert!(after < before, "before={before} after={after} {compact}");
}

#[tokio::test]
async fn under_budget_run_does_not_claim_compact() {
    let _environment_lock = environment_lock();
    let host = compacting_host(json!([{"text": "short turn"}]), 100_000, 40);
    let client = KianaClient::new(InProcessTransport { host });
    let response = client.run(trusted_metadata(), "hello", None).await.unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let compact = &response.output["compact"];
    assert_eq!(compact["applied"], false, "{response:?}");
    assert_eq!(compact["count"], 0, "{compact}");
}

#[tokio::test]
async fn continue_after_compact_still_writes_golden_path() {
    let _environment_lock = environment_lock();
    let root = temp_project();
    let host = compacting_host(
        json!([
            {"text": "compacted first turn"},
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
        ]),
        200,
        40,
    );
    let client = KianaClient::new(InProcessTransport { host });
    let started = client
        .run(
            trusted_write_metadata_in(&root),
            "x".repeat(2000),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(started.status, ExecutionStatus::Completed, "{started:?}");
    assert_eq!(started.output["compact"]["applied"], true, "{started:?}");
    assert!(
        !root.join("GOLDEN_PATH.txt").exists(),
        "first compacted turn must not write yet"
    );

    let continued = client
        .continue_run(
            trusted_write_metadata_in(&root),
            "create GOLDEN_PATH.txt",
            Some("workspace-write".to_owned()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        continued.status,
        ExecutionStatus::Completed,
        "{continued:?}"
    );
    assert_eq!(
        continued.output["compact"]["applied"], true,
        "{continued:?}"
    );
    let written = root.join("GOLDEN_PATH.txt");
    assert!(
        written.exists(),
        "continue after compact must write GOLDEN_PATH.txt: {continued:?}"
    );
    assert_eq!(fs::read_to_string(written).unwrap(), "hello\n");
}
