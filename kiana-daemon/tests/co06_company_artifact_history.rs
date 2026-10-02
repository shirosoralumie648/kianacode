use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_daemon::LocalArtifactStore;
use kiana_domain::{
    ApprovalChallenge, ApprovalId, ArtifactId, CommandIntent, CompanyCommand,
    CompanyCommandRequest, CompanyEvent, ExecutionStatus, PendingApproval, PermissionProfile,
    RequestContext, RoleSpec, COMPANY_COMMAND, COMPANY_COMMAND_SCHEMA,
};
use kiana_eventlog::JsonlEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, ArtifactContentPort, EventStorePort, PortError};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

struct UnusedApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnusedApprovalStore {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: kiana_domain::CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable(
            "approval_not_used_by_fixture".to_owned(),
        ))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "approval_not_used_by_fixture".to_owned(),
        ))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "approval_not_used_by_fixture".to_owned(),
        ))
    }
}

struct Fixture {
    root: PathBuf,
    event_log_path: PathBuf,
    artifact_store_path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("kiana-co06-company-{}", ArtifactId::new()));
        fs::create_dir_all(root.join(".git")).expect("project root");
        fs::create_dir_all(root.join(".kiana")).expect("project state root");
        let event_log_path = root.join(".kiana/company-events.jsonl");
        let artifact_store_path = root.join(".kiana/artifacts");
        Self {
            root,
            event_log_path,
            artifact_store_path,
        }
    }

    fn context(&self) -> RequestContext {
        let mut context = RequestContext::local(
            "co06-artifact-session",
            self.root.to_string_lossy().into_owned(),
        );
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        context.assign_role(&RoleSpec::builder());
        context
    }

    async fn register_artifact(
        &self,
        core: &ControlPlane,
        expected_revision: u64,
        idempotency_key: &str,
        artifact_id: &str,
        relative_path: &str,
    ) -> kiana_domain::CoreResponse {
        let request = CompanyCommandRequest {
            schema: COMPANY_COMMAND_SCHEMA.to_owned(),
            expected_revision,
            idempotency_key: idempotency_key.to_owned(),
            command: CompanyCommand::RegisterArtifact {
                artifact_id: artifact_id.to_owned(),
                relative_path: relative_path.to_owned(),
            },
        };
        core.handle_command(
            self.context(),
            CommandIntent::new(COMPANY_COMMAND, json!(request)),
        )
        .await
        .expect("ControlPlane command")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn core(events: Arc<JsonlEventLog>, store: Arc<LocalArtifactStore>) -> ControlPlane {
    let runner = KianaHarness::new(Arc::new(
        ScriptedModel::from_json(&json!([{"text":"unused fixture response"}]))
            .expect("scripted model"),
    ));
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnusedApprovalStore),
        Arc::new(runner),
    )
    .with_artifact_store(store)
}

async fn registered_company_event(events: &dyn EventStorePort) -> CompanyEvent {
    let fact = events
        .read_all()
        .await
        .expect("canonical EventLog read")
        .into_iter()
        .find(|event| event.kind == "company.ArtifactRegistered")
        .expect("Company artifact fact");
    serde_json::from_value(fact.data).expect("Company event payload")
}

#[tokio::test]
async fn artifact_version_remains_reviewable_after_workspace_file_changes() {
    let fixture = Fixture::new();
    let event_log = Arc::new(JsonlEventLog::open(&fixture.event_log_path).expect("EventLog"));
    let artifact_store = Arc::new(
        LocalArtifactStore::open(fixture.artifact_store_path.clone())
            .await
            .expect("artifact store"),
    );
    let core = core(event_log.clone(), artifact_store.clone());

    let missing = fixture
        .register_artifact(
            &core,
            0,
            "missing-source",
            &ArtifactId::new().to_string(),
            "missing.txt",
        )
        .await;
    assert_eq!(missing.status, ExecutionStatus::Blocked, "{missing:?}");
    assert_eq!(
        missing.error.as_deref(),
        Some("company_artifact_read_failed"),
        "{missing:?}"
    );
    assert_eq!(
        fs::read_dir(&fixture.artifact_store_path)
            .expect("artifact store root")
            .count(),
        0,
        "denied source must not publish a blob"
    );
    assert!(!event_log
        .read_all()
        .await
        .expect("EventLog after denial")
        .iter()
        .any(|event| event.kind == "company.ArtifactRegistered"));

    let artifact_id = ArtifactId::new();
    let artifact_path = fixture.root.join("evidence.txt");
    let original = b"approved historical evidence\n";
    fs::write(&artifact_path, original).expect("historical source");
    let registered = fixture
        .register_artifact(
            &core,
            0,
            "register-evidence",
            &artifact_id.to_string(),
            "evidence.txt",
        )
        .await;
    assert_eq!(
        registered.status,
        ExecutionStatus::Completed,
        "{registered:?}"
    );

    let original_fact = registered_company_event(event_log.as_ref()).await;
    let original_version = original_fact
        .proof
        .artifact_version
        .clone()
        .expect("typed historical version in Company fact");
    assert_eq!(
        original_fact
            .proof
            .artifact
            .as_ref()
            .map(|artifact| artifact.text.as_bytes()),
        Some(original.as_slice())
    );

    let current = b"current workspace evidence\n";
    fs::write(&artifact_path, current).expect("workspace edit");
    drop(core);
    drop(event_log);
    drop(artifact_store);

    let reopened_events = JsonlEventLog::open(&fixture.event_log_path).expect("reopened EventLog");
    let reopened_fact = registered_company_event(&reopened_events).await;
    assert_eq!(
        reopened_fact.proof.artifact_version,
        Some(original_version.clone())
    );
    let reopened_store = LocalArtifactStore::open(fixture.artifact_store_path.clone())
        .await
        .expect("reopened artifact store");
    let historical_reference = original_version.as_ref();
    let historical = ArtifactContentPort::read_artifact(&reopened_store, &historical_reference)
        .await
        .expect("historical bytes remain reviewable");

    assert_eq!(historical.as_slice(), original.as_slice());
    assert_eq!(
        fs::read(&artifact_path)
            .expect("current workspace file")
            .as_slice(),
        current.as_slice()
    );
}
