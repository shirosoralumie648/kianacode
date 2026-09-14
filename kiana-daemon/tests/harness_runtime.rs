use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest, CapabilityResult, PromptBundle,
    RoleSpec, RuntimeEvent,
};
use kiana_ports::{CapabilityBrokerPort, PortError};
use kiana_protocol::{ExecutionStatus, PermissionProfile, RequestEnvelope, RequestMetadata, RunId};
use kiana_runner::{KianaHarness, ModelClient, ModelOutput, ModelRequest, ScriptedModel};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};

struct InProcessTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for InProcessTransport {
    async fn send(
        &self,
        request: RequestEnvelope,
    ) -> Result<kiana_protocol::ResponseEnvelope, ClientError> {
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

struct CountingModel {
    inner: ScriptedModel,
    calls: Arc<AtomicUsize>,
}

#[allow(dead_code)]
#[derive(Default)]
struct CountingBroker {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl CapabilityBrokerPort for CountingBroker {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CapabilityResult::success(
            request.request.request_id,
            json!({"fixture": "counting_broker"}),
        ))
    }
}

impl CountingModel {
    fn from_json(cassette: serde_json::Value) -> (Arc<Self>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = Arc::new(Self {
            inner: ScriptedModel::from_json(&cassette).unwrap(),
            calls: calls.clone(),
        });
        (model, calls)
    }
}

#[async_trait]
impl ModelClient for CountingModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.complete(request).await
    }
}

fn temp_project() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-h01-runtime-{stamp}"));
    std::fs::create_dir_all(&root).expect("temporary project");
    root
}

fn metadata(root: &Path, trusted: bool) -> RequestMetadata {
    let mut metadata = RequestMetadata::local("h01-session", root.to_string_lossy());
    metadata.project_trusted = trusted;
    metadata.permission_profile = PermissionProfile::Balanced;
    metadata
}

fn host(model: Arc<dyn ModelClient>, trusted: bool) -> Arc<DaemonHost> {
    let harness = KianaHarness::new(model);
    Arc::new(
        DaemonHost::with_harness_and_project_authority(
            harness,
            Arc::new(FixedProjectTrustAuthority { trusted }),
        )
        .expect("h01 daemon host"),
    )
}

#[tokio::test]
async fn untrusted_run_never_reaches_model_or_broker() {
    let root = temp_project();
    let (model, calls) = CountingModel::from_json(json!([
        {
            "text": "must not be called",
            "tool_calls": [{
                "id": "h01-untrusted",
                "name": "shell",
                "arguments": {"command": "printf forbidden"}
            }]
        }
    ]));
    let host = host(model, false);
    let client = KianaClient::new(InProcessTransport { host: host.clone() });
    let response = client
        .run(
            metadata(&root, true),
            "attempt a workspace write",
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("workspace_write_requires_trusted_non_safe_profile")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(host
        .persisted_events()
        .await
        .unwrap()
        .unwrap()
        .iter()
        .all(|event| event.kind != "run.capability_requested"));
}

#[tokio::test]
async fn host_model_tool_result_next_step_receipt_roundtrip() {
    let root = temp_project();
    let (model, calls) = CountingModel::from_json(json!([
        {
            "text": "reading the project",
            "tool_calls": [{
                "id": "h01-shell",
                "name": "shell",
                "arguments": {"command": "printf h01"}
            }]
        },
        {"text": "roundtrip complete"}
    ]));
    let host = host(model, true);
    let client = KianaClient::new(InProcessTransport { host: host.clone() });
    let request_metadata = metadata(&root, true);
    let response = client
        .run(
            request_metadata.clone(),
            "read the project and summarize it",
            None,
        )
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["output"]["text"], "roundtrip complete");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let run_id = RunId::parse_str(response.output["run_id"].as_str().unwrap())
        .expect("run id in completed response");
    let receipt = client
        .receipt(request_metadata, Some(run_id))
        .await
        .unwrap();
    assert_eq!(receipt.status, ExecutionStatus::Completed, "{receipt:?}");
    assert_eq!(receipt.output["output"]["text"], "roundtrip complete");

    let events: Vec<RuntimeEvent> = host.persisted_events().await.unwrap().unwrap();
    assert!(events.iter().any(|event| {
        event.kind == "run.capability_requested" && event.data["run_id"] == json!(run_id)
    }));
    assert!(events
        .iter()
        .any(|event| { event.kind == "run.tool_result" && event.data["run_id"] == json!(run_id) }));
    assert!(events
        .iter()
        .any(|event| { event.kind == "run.completed" && event.data["run_id"] == json!(run_id) }));
    assert!(events.iter().any(|event| {
        event.kind == "run.tool_result" && event.data.to_string().contains("h01")
    }));
}

#[test]
fn prompt_bundle_fixture_keeps_role_context_explicit() {
    let encoded = PromptBundle::for_role(&RoleSpec::builder())
        .encode()
        .expect("prompt fixture encoding");
    let bundle = PromptBundle::decode(&encoded).expect("prompt fixture");
    assert_eq!(bundle.role_id, "builder");
    assert_eq!(bundle.model_profile, RoleSpec::builder().model_profile);
}

#[tokio::test]
async fn counting_broker_fixture_counts_authorized_calls() {
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = CountingBroker {
        calls: calls.clone(),
    };
    let request = CapabilityRequest::new(
        kiana_domain::RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({"command": "printf fixture"}),
    );
    let authorized = AuthorizedCapabilityRequest::new("h01-fixture", request).unwrap();
    let result = broker.execute(authorized).await.unwrap();
    assert!(result.success);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
