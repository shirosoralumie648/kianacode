use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{ControlPlane, CoreError};
use kiana_daemon::LocalArtifactStore;
use kiana_domain::{
    ApprovalChallenge, ApprovalId, ArtifactId, CommandIntent, CompanyBudgetPolicy, CompanyCommand,
    CompanyCommandRequest, CompanyEvent, ExecutionStatus, MetricDirection, Objective,
    ObjectiveStatus, PendingApproval, PermissionProfile, Project, ProjectBudget, ProjectId,
    ProjectStatus, Quota, RequestContext, RoleSpec, RuntimeBudget, COMPANY_COMMAND,
    COMPANY_COMMAND_SCHEMA,
};
use kiana_eventlog::JsonlEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{
    ApprovalStorePort, ArtifactContentPort, ArtifactStorePort, EventStorePort, PortError,
};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::json;
use std::fs;
use std::io::Write as _;
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

    fn context(&self, session_id: &str, role: &RoleSpec) -> RequestContext {
        let mut context =
            RequestContext::local(session_id, self.root.to_string_lossy().into_owned());
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        context.assign_role(role);
        context
    }

    async fn command(
        &self,
        core: &ControlPlane,
        session_id: &str,
        role: RoleSpec,
        expected_revision: u64,
        idempotency_key: &str,
        command: CompanyCommand,
    ) -> Result<kiana_domain::CoreResponse, CoreError> {
        let request = CompanyCommandRequest {
            schema: COMPANY_COMMAND_SCHEMA.to_owned(),
            expected_revision,
            idempotency_key: idempotency_key.to_owned(),
            command,
        };
        let context = self.context(session_id, &role);
        let configuration_revision = kiana_domain::json_digest(&json!({
            "fixture": "co06-company-artifact-history.v1"
        }));
        trace_ci_phase("authority synchronization begin");
        core.synchronize_authority(&context, &configuration_revision)
            .await
            .expect("server authority snapshot");
        trace_ci_phase("authority synchronization complete");
        trace_ci_future_size(future_type_size(|| {
            core.handle_command(
                context.clone(),
                CommandIntent::new(COMPANY_COMMAND, json!(request)),
            )
        }));
        trace_ci_phase("Company command begin");
        let response = core
            .handle_command(context, CommandIntent::new(COMPANY_COMMAND, json!(request)))
            .await;
        trace_ci_phase("Company command complete");
        response
    }

    async fn command_without_authority_sync(
        &self,
        core: &ControlPlane,
        session_id: &str,
        role: RoleSpec,
        expected_revision: u64,
        idempotency_key: &str,
        command: CompanyCommand,
    ) -> Result<kiana_domain::CoreResponse, CoreError> {
        let request = CompanyCommandRequest {
            schema: COMPANY_COMMAND_SCHEMA.to_owned(),
            expected_revision,
            idempotency_key: idempotency_key.to_owned(),
            command,
        };
        let context = self.context(session_id, &role);
        core.handle_command(context, CommandIntent::new(COMPANY_COMMAND, json!(request)))
            .await
    }

    async fn register_artifact(
        &self,
        core: &ControlPlane,
        expected_revision: u64,
        idempotency_key: &str,
        artifact_id: &str,
        relative_path: &str,
    ) -> kiana_domain::CoreResponse {
        self.command(
            core,
            "builder-session",
            RoleSpec::builder(),
            expected_revision,
            idempotency_key,
            CompanyCommand::RegisterArtifact {
                artifact_id: artifact_id.to_owned(),
                relative_path: relative_path.to_owned(),
            },
        )
        .await
        .expect("ControlPlane command")
    }

    fn artifact_blob_path(&self) -> PathBuf {
        let scope = fs::read_dir(&self.artifact_store_path)
            .expect("artifact store root")
            .map(|entry| entry.expect("artifact scope").path())
            .next()
            .expect("artifact scope directory");
        let artifact = fs::read_dir(scope)
            .expect("artifact scope")
            .map(|entry| entry.expect("artifact directory").path())
            .next()
            .expect("artifact directory");
        fs::read_dir(artifact)
            .expect("artifact files")
            .map(|entry| entry.expect("artifact file").path())
            .find(|path| path.extension().is_some_and(|value| value == "blob"))
            .expect("artifact blob")
    }
}

fn trace_ci_phase(phase: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "co06_company_artifact_history phase: {phase}");
    let _ = stderr.flush();
}

// Infer the opaque future type without constructing or polling the future.
fn future_type_size<F: std::future::Future>(_factory: impl FnOnce() -> F) -> usize {
    std::mem::size_of::<F>()
}

fn trace_ci_future_size(bytes: usize) {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(
        stderr,
        "co06_company_artifact_history handle_command boxed future bytes: {bytes}"
    );
    let _ = stderr.flush();
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn company_control_plane(
    events: Arc<dyn EventStorePort>,
    store: Arc<dyn ArtifactStorePort>,
) -> ControlPlane {
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

async fn company_fact_count(events: &dyn EventStorePort) -> usize {
    events
        .read_all()
        .await
        .expect("canonical EventLog read")
        .iter()
        .filter(|event| {
            event.kind.starts_with("company.") && event.kind != "company.command_rejected"
        })
        .count()
}

async fn registered_event_id(events: &dyn EventStorePort) -> String {
    events
        .read_all()
        .await
        .expect("canonical EventLog read")
        .into_iter()
        .find(|event| event.kind == "company.ArtifactRegistered")
        .expect("Company artifact fact")
        .event_id
        .to_string()
}

fn project(project_id: &str, charter_ref: &str) -> Project {
    Project {
        project_id: project_id.to_owned(),
        organization_id: "org-co06".to_owned(),
        objective_refs: vec!["objective-co06".to_owned()],
        sponsor_id: "local-user".to_owned(),
        charter_ref: charter_ref.to_owned(),
        scope_baseline: "scope-v1".to_owned(),
        success_criteria: vec!["historical charter remains reviewable".to_owned()],
        non_goals: vec!["no external delivery".to_owned()],
        project_budget_ref: format!("budget-{project_id}"),
        risk_summary: "bounded fixture".to_owned(),
        decision_ref: None,
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: None,
        closing_receipt_id: None,
        status: ProjectStatus::Proposed,
        version: 1,
    }
}

fn objective() -> Objective {
    Objective {
        objective_id: "objective-co06".to_owned(),
        organization_id: "org-co06".to_owned(),
        title: "Keep approved evidence reviewable".to_owned(),
        problem: "A workspace edit must not rewrite historical evidence".to_owned(),
        metric: "evidence_reviewable".to_owned(),
        baseline: 0.0,
        target: 1.0,
        unit: "ratio".to_owned(),
        measurement_method: Some("CompanyProof fixture".to_owned()),
        direction: MetricDirection::AtLeast,
        period_start: 1,
        period_end: u64::MAX - 1,
        owner_principal_id: "local-user".to_owned(),
        status: ObjectiveStatus::Proposed,
        version: 1,
    }
}

fn budget(project_id: &str) -> CompanyBudgetPolicy {
    CompanyBudgetPolicy {
        project: ProjectBudget {
            project_id: ProjectId::new(),
            max_runs: 4,
            max_tokens: 100_000,
        },
        runtime: RuntimeBudget {
            max_model_calls: 8,
            max_tokens: 100_000,
            max_wall_time_ms: 60_000,
        },
        quota: Quota {
            scope: project_id.to_owned(),
            model_calls: 8,
            tokens: 100_000,
            concurrency: 1,
        },
    }
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
    let core = company_control_plane(event_log.clone(), artifact_store.clone());

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

    let decision_ref = format!("event:{}", registered_event_id(event_log.as_ref()).await);
    let sponsor_session = "sponsor-session";
    let charter_ref = format!("artifact:{artifact_id}");
    let objective_proposal = fixture
        .command(
            &core,
            sponsor_session,
            RoleSpec::sponsor(),
            1,
            "propose-objective",
            CompanyCommand::ProposeObjective {
                objective: objective(),
            },
        )
        .await
        .expect("objective proposal command");
    assert_eq!(
        objective_proposal.status,
        ExecutionStatus::Completed,
        "{objective_proposal:?}"
    );
    let objective_decision = fixture
        .command(
            &core,
            sponsor_session,
            RoleSpec::sponsor(),
            2,
            "activate-objective",
            CompanyCommand::DecideObjective {
                objective_id: "objective-co06".to_owned(),
                approve: true,
            },
        )
        .await
        .expect("objective decision command");
    assert_eq!(
        objective_decision.status,
        ExecutionStatus::Completed,
        "{objective_decision:?}"
    );
    let proposed = fixture
        .command(
            &core,
            sponsor_session,
            RoleSpec::sponsor(),
            3,
            "propose-project",
            CompanyCommand::ProposeProject {
                project: project("project-co06", &charter_ref),
            },
        )
        .await
        .expect("project proposal command");
    assert_eq!(proposed.status, ExecutionStatus::Completed, "{proposed:?}");
    let chartering = fixture
        .command(
            &core,
            sponsor_session,
            RoleSpec::sponsor(),
            4,
            "start-chartering",
            CompanyCommand::StartChartering {
                project_id: "project-co06".to_owned(),
            },
        )
        .await
        .expect("start chartering command");
    assert_eq!(
        chartering.status,
        ExecutionStatus::Completed,
        "{chartering:?}"
    );
    let configured = fixture
        .command(
            &core,
            sponsor_session,
            RoleSpec::sponsor(),
            5,
            "configure-budget",
            CompanyCommand::ConfigureBudget {
                project_id: "project-co06".to_owned(),
                policy: budget("project-co06"),
            },
        )
        .await
        .expect("configure budget command");
    assert_eq!(
        configured.status,
        ExecutionStatus::Completed,
        "{configured:?}"
    );

    let facts_before_foreign = company_fact_count(event_log.as_ref()).await;
    let foreign_reference = fixture
        .command(
            &core,
            "reviewer-session",
            RoleSpec::reviewer(),
            6,
            "review-foreign-artifact",
            CompanyCommand::ReviewPacket {
                packet_id: "missing-packet".to_owned(),
                review_id: "foreign-artifact-review".to_owned(),
                criterion_results: Default::default(),
                evidence_refs: vec!["artifact:foreign-artifact".to_owned()],
            },
        )
        .await
        .expect("foreign reference denial response");
    assert_eq!(foreign_reference.status, ExecutionStatus::Blocked);
    assert_eq!(
        foreign_reference.error.as_deref(),
        Some("company_artifact_not_registered")
    );
    assert_eq!(
        company_fact_count(event_log.as_ref()).await,
        facts_before_foreign,
        "foreign artifact references must not append a Company fact"
    );

    let current = b"current workspace evidence\n";
    fs::write(&artifact_path, current).expect("workspace edit");

    drop(core);
    drop(event_log);
    drop(artifact_store);
    let event_log =
        Arc::new(JsonlEventLog::open(&fixture.event_log_path).expect("reopened EventLog"));
    let artifact_store = Arc::new(
        LocalArtifactStore::open(fixture.artifact_store_path.clone())
            .await
            .expect("reopened artifact store"),
    );
    let core = company_control_plane(event_log.clone(), artifact_store.clone());
    let reopened_fact = registered_company_event(event_log.as_ref()).await;
    assert_eq!(
        reopened_fact.proof.artifact_version,
        Some(original_version.clone())
    );

    let artifact_blob = fixture.artifact_blob_path();
    let stored_bytes = fs::read(&artifact_blob).expect("stored historical artifact");
    assert_eq!(stored_bytes.as_slice(), original.as_slice());

    let facts_before_missing_blob = company_fact_count(event_log.as_ref()).await;
    fs::remove_file(&artifact_blob).expect("remove historical blob for deny fixture");
    let missing_blob = fixture
        .command(
            &core,
            sponsor_session,
            RoleSpec::sponsor(),
            6,
            "approve-with-missing-historical-blob",
            CompanyCommand::ApproveProject {
                project_id: "project-co06".to_owned(),
                decision_ref: decision_ref.clone(),
            },
        )
        .await
        .expect_err("missing historical blob must fail closed");
    assert!(
        missing_blob.to_string().contains("artifact_blob_missing"),
        "{missing_blob}"
    );
    assert_eq!(
        company_fact_count(event_log.as_ref()).await,
        facts_before_missing_blob,
        "missing historical blobs must not append a Company fact"
    );

    fs::write(&artifact_blob, &stored_bytes).expect("restore historical artifact");
    fs::write(&artifact_blob, b"tampered historical evidence\n")
        .expect("tamper historical artifact");
    let facts_before_hash_drift = company_fact_count(event_log.as_ref()).await;
    let hash_drift = fixture
        .command(
            &core,
            sponsor_session,
            RoleSpec::sponsor(),
            6,
            "approve-with-hash-drifted-historical-blob",
            CompanyCommand::ApproveProject {
                project_id: "project-co06".to_owned(),
                decision_ref: decision_ref.clone(),
            },
        )
        .await
        .expect("hash drift denial response");
    assert_eq!(
        hash_drift.status,
        ExecutionStatus::Blocked,
        "{hash_drift:?}"
    );
    assert_eq!(
        hash_drift.error.as_deref(),
        Some("artifact_content_hash_mismatch")
    );
    assert_eq!(
        company_fact_count(event_log.as_ref()).await,
        facts_before_hash_drift,
        "hash-drifted historical blobs must not append a Company fact"
    );

    fs::write(&artifact_blob, &stored_bytes).expect("restore verified historical artifact");
    let approved = fixture
        .command(
            &core,
            sponsor_session,
            RoleSpec::sponsor(),
            6,
            "approve-project-from-historical-charter",
            CompanyCommand::ApproveProject {
                project_id: "project-co06".to_owned(),
                decision_ref,
            },
        )
        .await
        .expect("historical CompanyProof command");
    assert_eq!(approved.status, ExecutionStatus::Completed, "{approved:?}");
    let approved_fact = event_log
        .read_all()
        .await
        .expect("EventLog after historical approval")
        .into_iter()
        .find(|event| event.kind == "company.ProjectApproved")
        .expect("historical approval fact");
    let approved_event: CompanyEvent =
        serde_json::from_value(approved_fact.data).expect("approved Company event");
    assert!(approved_event.proof.events.contains(&charter_ref));

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

/// EventStore 读写可用但不声明多聚合原子事务时，Company artifact 发布必须拒绝。
///
/// 这个 adapter 保留真实 JsonlEventLog 的读取和提交语义，只把事务能力声明为 false，
/// 因此测试命中的是 ControlPlane 的能力 gate，而不是一个假的写入错误。
struct NonAtomicEventStore {
    inner: Arc<JsonlEventLog>,
    atomic: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl EventStorePort for NonAtomicEventStore {
    fn supports_atomic_transitions(&self) -> bool {
        self.atomic.load(std::sync::atomic::Ordering::SeqCst)
    }

    async fn append(&self, event: kiana_domain::RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }

    async fn append_expected(
        &self,
        event: kiana_domain::RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        self.inner.append_expected(event, expected_version).await
    }

    async fn read_request(
        &self,
        id: &kiana_domain::RequestId,
    ) -> Result<Vec<kiana_domain::RuntimeEvent>, PortError> {
        self.inner.read_request(id).await
    }

    async fn read_all(&self) -> Result<Vec<kiana_domain::RuntimeEvent>, PortError> {
        self.inner.read_all().await
    }

    async fn read_stream(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<Vec<kiana_domain::RuntimeEvent>, PortError> {
        self.inner.read_stream(kind, id).await
    }

    async fn read_command(
        &self,
        id: &kiana_domain::RequestId,
    ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
        self.inner.read_command(id).await
    }

    async fn commit_transition(
        &self,
        batch: kiana_domain::TransitionBatch,
    ) -> Result<kiana_domain::CommitOutcome, PortError> {
        self.inner.commit_transition(batch).await
    }
}

#[tokio::test]
async fn artifact_publication_is_denied_when_event_store_lacks_atomic_transitions() {
    let fixture = Fixture::new();
    let event_log = Arc::new(JsonlEventLog::open(&fixture.event_log_path).expect("EventLog"));
    let artifact_store = Arc::new(fixture_artifact_store(&fixture).await);
    let atomic = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let events: Arc<dyn EventStorePort> = Arc::new(NonAtomicEventStore {
        inner: event_log.clone(),
        atomic: atomic.clone(),
    });
    let core = company_control_plane(events, artifact_store);
    fs::write(fixture.root.join("atomicity.txt"), b"atomicity gate\n").expect("source file");

    let authority_context = fixture.context("builder-session", &RoleSpec::builder());
    core.synchronize_authority(&authority_context, "co06-atomicity-deny.v1")
        .await
        .expect("authority synchronization");
    atomic.store(false, std::sync::atomic::Ordering::SeqCst);

    let response = fixture
        .command_without_authority_sync(
            &core,
            "builder-session",
            RoleSpec::builder(),
            0,
            "non-atomic-artifact",
            CompanyCommand::RegisterArtifact {
                artifact_id: ArtifactId::new().to_string(),
                relative_path: "atomicity.txt".to_owned(),
            },
        )
        .await
        .expect("artifact command");
    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("company_artifact_event_atomicity_unsupported"),
        "{response:?}"
    );
    assert_eq!(
        fs::read_dir(&fixture.artifact_store_path)
            .expect("artifact store root")
            .count(),
        0,
        "atomicity denial must not publish a blob or manifest"
    );
    let persisted = event_log.read_all().await.expect("EventLog after denial");
    assert!(!persisted
        .iter()
        .any(|event| event.kind == "company.ArtifactRegistered"));
    assert!(persisted
        .iter()
        .any(|event| event.kind == "company.command_rejected"));
}

/// Lose one Company fact acknowledgement, either before or after its actual append.
/// Authority synchronization and unrelated journal writes retain the real adapter semantics.
struct ArtifactFactFault {
    inner: Arc<JsonlEventLog>,
    armed: std::sync::atomic::AtomicBool,
    append_before_failure: bool,
}

#[async_trait]
impl EventStorePort for ArtifactFactFault {
    fn supports_atomic_transitions(&self) -> bool {
        true
    }

    async fn append(&self, event: kiana_domain::RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }

    async fn read_request(
        &self,
        id: &kiana_domain::RequestId,
    ) -> Result<Vec<kiana_domain::RuntimeEvent>, PortError> {
        self.inner.read_request(id).await
    }

    async fn read_all(&self) -> Result<Vec<kiana_domain::RuntimeEvent>, PortError> {
        self.inner.read_all().await
    }

    async fn read_stream(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<Vec<kiana_domain::RuntimeEvent>, PortError> {
        self.inner.read_stream(kind, id).await
    }

    async fn read_command(
        &self,
        id: &kiana_domain::RequestId,
    ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
        self.inner.read_command(id).await
    }

    async fn commit_transition(
        &self,
        batch: kiana_domain::TransitionBatch,
    ) -> Result<kiana_domain::CommitOutcome, PortError> {
        if batch
            .events
            .iter()
            .any(|event| event.kind == "company.ArtifactRegistered")
            && self.armed.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            if self.append_before_failure {
                self.inner.commit_transition(batch).await?;
            }
            return Err(PortError::Failed(
                "result_unknown:fixture_company_fact_ack_lost".to_owned(),
            ));
        }
        self.inner.commit_transition(batch).await
    }
}

#[tokio::test]
async fn artifact_fact_failure_retry_returns_the_persisted_version_and_replays_once() {
    for append_before_failure in [false, true] {
        let fixture = Fixture::new();
        let events = Arc::new(JsonlEventLog::open(&fixture.event_log_path).expect("EventLog"));
        let store = Arc::new(fixture_artifact_store(&fixture).await);
        let fault = Arc::new(ArtifactFactFault {
            inner: events.clone(),
            armed: std::sync::atomic::AtomicBool::new(true),
            append_before_failure,
        });
        let core = company_control_plane(fault, store.clone());
        let artifact_id = ArtifactId::new().to_string();
        fs::write(fixture.root.join("retry.txt"), b"first immutable bytes\n").unwrap();
        let command = CompanyCommand::RegisterArtifact {
            artifact_id: artifact_id.clone(),
            relative_path: "retry.txt".to_owned(),
        };
        let first = fixture
            .command(
                &core,
                "builder-session",
                RoleSpec::builder(),
                0,
                "retry-artifact-fact",
                command.clone(),
            )
            .await;
        assert!(
            first.is_err(),
            "unconfirmed fact must not return completed: {first:?}"
        );
        assert!(first
            .unwrap_err()
            .to_string()
            .contains("company_artifact_event_commit_unconfirmed"));
        assert_eq!(
            company_fact_count(events.as_ref()).await,
            usize::from(append_before_failure)
        );

        // Inspect the actual first-stage manifest, without inventing a timestamp or a blob.
        let blob = fixture.artifact_blob_path();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(blob.with_extension("manifest")).unwrap()).unwrap();
        let persisted: kiana_domain::ArtifactVersion =
            serde_json::from_value(manifest["version"].clone()).unwrap();
        assert_eq!(
            ArtifactContentPort::read_artifact(store.as_ref(), &persisted.as_ref())
                .await
                .unwrap(),
            b"first immutable bytes\n"
        );
        // The second proposal must have a later wall-clock timestamp than the first manifest.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let retried = fixture
            .command(
                &core,
                "builder-session",
                RoleSpec::builder(),
                0,
                "retry-artifact-fact",
                command.clone(),
            )
            .await
            .unwrap();
        assert_eq!(retried.status, ExecutionStatus::Completed, "{retried:?}");
        assert_eq!(company_fact_count(events.as_ref()).await, 1);
        let fact = registered_company_event(events.as_ref()).await;
        assert_eq!(fact.proof.artifact_version.as_ref(), Some(&persisted));
        assert_eq!(
            retried.output["state"]["artifacts"][&artifact_id]["typed_version"],
            json!(persisted),
            "response must use the stored manifest, not the later retry proposal"
        );
        let event_id = registered_event_id(events.as_ref()).await;
        drop(core);
        drop(store);
        drop(events);
        let reopened_events = Arc::new(JsonlEventLog::open(&fixture.event_log_path).unwrap());
        let reopened_store = Arc::new(fixture_artifact_store(&fixture).await);
        let reopened = company_control_plane(reopened_events.clone(), reopened_store);
        let replay = fixture
            .command(
                &reopened,
                "builder-session",
                RoleSpec::builder(),
                0,
                "retry-artifact-fact",
                command,
            )
            .await
            .unwrap();
        assert_eq!(replay.status, ExecutionStatus::Completed);
        assert_eq!(
            replay.output["state"]["artifacts"][&artifact_id]["typed_version"],
            json!(persisted)
        );
        assert_eq!(company_fact_count(reopened_events.as_ref()).await, 1);
        assert_eq!(
            registered_event_id(reopened_events.as_ref()).await,
            event_id
        );
    }
}

/// Publication-precondition fault: delegate to the real local adapter, but withhold one
/// precondition of the ControlPlane's committed-artifact fence once. Commit semantics and stored
/// bytes remain the real adapter's; only the returned manifest or the read-back result is faulty.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArtifactPublicationFault {
    StagedManifestMismatch,
    CommittedBlobUnreadable,
}

struct FaultyArtifactStore {
    inner: Arc<LocalArtifactStore>,
    fault: std::sync::Mutex<Option<ArtifactPublicationFault>>,
}

impl FaultyArtifactStore {
    fn take_fault(&self, expected: ArtifactPublicationFault) -> bool {
        let mut fault = self.fault.lock().expect("fixture fault lock");
        if *fault == Some(expected) {
            *fault = None;
            true
        } else {
            false
        }
    }
}

#[async_trait]
impl ArtifactStorePort for FaultyArtifactStore {
    async fn stage_artifact_version(
        &self,
        version: kiana_domain::ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<kiana_domain::ArtifactVersion, PortError> {
        let persisted = self.inner.stage_artifact_version(version, content).await?;
        if self.take_fault(ArtifactPublicationFault::StagedManifestMismatch) {
            let mut tampered = persisted;
            tampered.content_hash = kiana_domain::journal_sha256(b"tampered staged manifest");
            return Ok(tampered);
        }
        Ok(persisted)
    }

    async fn commit_artifact(
        &self,
        reference: kiana_domain::ArtifactRef,
        expected_revision: Option<u64>,
    ) -> Result<(), PortError> {
        self.inner
            .commit_artifact(reference, expected_revision)
            .await
    }

    async fn read_artifact(
        &self,
        reference: &kiana_domain::ArtifactRef,
    ) -> Result<Vec<u8>, PortError> {
        if self.take_fault(ArtifactPublicationFault::CommittedBlobUnreadable) {
            return Err(PortError::Unavailable(
                "fixture_committed_blob_unreadable".to_owned(),
            ));
        }
        ArtifactContentPort::read_artifact(self.inner.as_ref(), reference).await
    }
}

#[tokio::test]
async fn artifact_publication_denies_uncorrelated_manifest_and_unreadable_commit() {
    for (fault, expected_code) in [
        (
            ArtifactPublicationFault::StagedManifestMismatch,
            "company_artifact_staged_manifest_mismatch",
        ),
        (
            ArtifactPublicationFault::CommittedBlobUnreadable,
            "company_artifact_committed_blob_unverified",
        ),
    ] {
        let fixture = Fixture::new();
        let events = Arc::new(JsonlEventLog::open(&fixture.event_log_path).expect("EventLog"));
        let inner = Arc::new(fixture_artifact_store(&fixture).await);
        let store: Arc<dyn ArtifactStorePort> = Arc::new(FaultyArtifactStore {
            inner: inner.clone(),
            fault: std::sync::Mutex::new(Some(fault)),
        });
        let core = company_control_plane(events.clone(), store);
        let artifact_id = ArtifactId::new().to_string();
        fs::write(
            fixture.root.join("verified.txt"),
            b"verified immutable bytes\n",
        )
        .expect("verified source");
        let command = CompanyCommand::RegisterArtifact {
            artifact_id: artifact_id.clone(),
            relative_path: "verified.txt".to_owned(),
        };

        let denied = fixture
            .command(
                &core,
                "builder-session",
                RoleSpec::builder(),
                0,
                "verify-artifact-publication",
                command.clone(),
            )
            .await
            .expect("publication denial is a command response");
        assert_eq!(
            denied.status,
            ExecutionStatus::Blocked,
            "{fault:?}: {denied:?}"
        );
        assert_eq!(
            denied.error.as_deref(),
            Some(expected_code),
            "{fault:?}: {denied:?}"
        );
        assert_eq!(
            company_fact_count(events.as_ref()).await,
            0,
            "{fault:?}: a denied publication must not append a Company fact"
        );

        // The same idempotency key must complete against the first verified manifest once the
        // adapter no longer withholds the publication precondition.
        let retried = fixture
            .command(
                &core,
                "builder-session",
                RoleSpec::builder(),
                0,
                "verify-artifact-publication",
                command,
            )
            .await
            .expect("retry after publication denial");
        assert_eq!(
            retried.status,
            ExecutionStatus::Completed,
            "{fault:?}: {retried:?}"
        );
        assert_eq!(company_fact_count(events.as_ref()).await, 1, "{fault:?}");
        let fact = registered_company_event(events.as_ref()).await;
        let persisted = fact
            .proof
            .artifact_version
            .expect("persisted version in Company fact");
        assert_eq!(
            ArtifactContentPort::read_artifact(inner.as_ref(), &persisted.as_ref())
                .await
                .expect("committed historical bytes"),
            b"verified immutable bytes\n",
            "{fault:?}"
        );
        assert_eq!(
            retried.output["state"]["artifacts"][&artifact_id]["typed_version"],
            json!(persisted),
            "{fault:?}"
        );
    }
}

async fn fixture_artifact_store(fixture: &Fixture) -> LocalArtifactStore {
    LocalArtifactStore::open(fixture.artifact_store_path.clone())
        .await
        .expect("artifact store")
}
