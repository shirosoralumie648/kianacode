//! ER-27 behavioural fixtures: four entry points share one read-only receipt/recovery route.
//!
//! CLI, Workbench, Web and Desktop are labelled by [`EntryPointKind`] on the request metadata, but
//! they must all reach the same `DaemonHost → ControlPlane → EventLog` path. These fixtures drive a
//! real [`DaemonHost`] with a zero-call runner and a durable-shaped memory store, then prove:
//!
//! * a receipt query is read-only: it never sends a runner command, never restores a runner and
//!   never appends an event;
//! * the four labels observe the same committed receipt digest, status and recovery outcome, and
//!   the same denial code for the same malformed request;
//! * a UI-supplied owner or scope cannot widen the server-owned principal;
//! * `resume`/`cancel` remain ControlPlane commands whose terminal answers are consistent across
//!   every entry point and produce no execution side effect.

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, EntryPointKind, PendingApproval,
    RequestContext, RequestId, RunId, RuntimeEvent,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_protocol::{ExecutionStatus, RequestEnvelope, RequestMetadata};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const SESSION: &str = "er27-session";
const ROLE_ID: &str = "builder";
const DEPARTMENT_ID: &str = "executing";

/// Trust authority fixed by the fixture: the daemon still re-resolves trust server-side.
#[derive(Clone, Copy)]
struct FixedProjectTrustAuthority {
    trusted: bool,
}

impl ProjectTrustAuthority for FixedProjectTrustAuthority {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(self.trusted)
    }
}

/// Approval store is deliberately empty but *listable*: a read-only pending query must succeed
/// with an empty projection instead of fabricating authority or failing open.
struct NoApprovals;

#[async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _: &RequestContext,
        _: CapabilityRequest,
        _: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }

    async fn activate(&self, _: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }

    async fn consume(
        &self,
        _: &RequestContext,
        _: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }

    async fn list_pending(&self, _: &RequestContext) -> Result<Vec<PendingApproval>, PortError> {
        Ok(Vec::new())
    }
}

/// Runner that records whether a read-only request illegally advanced execution.
struct ZeroCallRunner {
    sends: Arc<AtomicUsize>,
    restores: Arc<AtomicUsize>,
}

#[async_trait]
impl RunnerPort for ZeroCallRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("read_only_must_not_send".to_owned()))
    }

    async fn restore(&self, _run_id: RunId, _checkpoint: Value) -> Result<(), PortError> {
        self.restores.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("read_only_must_not_restore".to_owned()))
    }
}

fn zero_runner() -> (Arc<ZeroCallRunner>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let sends = Arc::new(AtomicUsize::new(0));
    let restores = Arc::new(AtomicUsize::new(0));
    let runner = Arc::new(ZeroCallRunner {
        sends: sends.clone(),
        restores: restores.clone(),
    });
    (runner, sends, restores)
}

fn host(store: Arc<MemoryEventLog>, runner: Arc<dyn RunnerPort>) -> DaemonHost {
    let core = Arc::new(ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        store,
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        runner,
    ));
    let authority = Arc::new(FixedProjectTrustAuthority { trusted: true });
    DaemonHost::new_with_project_authority(core, authority)
}

struct ProjectRoot(PathBuf);

impl ProjectRoot {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-er27-{label}-{stamp}"));
        std::fs::create_dir_all(&root).expect("project root");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ProjectRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_event(run_id: RunId, sequence: u64, kind: &str, data: Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data)
        .expect("fixture event")
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

fn authorized_data(run_id: RunId, root: &Path) -> Value {
    json!({
        "run_id": run_id,
        "session_id": SESSION,
        "actor_id": "local-user",
        "project_root": root.to_string_lossy(),
        "role_id": ROLE_ID,
        "department_id": DEPARTMENT_ID,
    })
}

/// Append a completed run owned by the local principal over one durable-shaped stream.
async fn committed_run(store: &Arc<MemoryEventLog>, run_id: RunId, root: &Path) {
    let events: Arc<dyn EventStorePort> = store.clone();
    let authorized = run_event(run_id, 1, "run.authorized", authorized_data(run_id, root));
    events
        .append(authorized)
        .await
        .expect("append run.authorized");
    let completed_data = json!({"run_id": run_id, "result": "ok"});
    let completed = run_event(run_id, 2, "run.completed", completed_data);
    events
        .append(completed)
        .await
        .expect("append run.completed");
}

fn metadata(root: &Path, entrypoint: EntryPointKind) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(SESSION, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata.entrypoint = Some(entrypoint);
    metadata
}

fn four_entrypoints() -> [EntryPointKind; 4] {
    [
        EntryPointKind::Cli,
        EntryPointKind::Web,
        EntryPointKind::Workbench,
        EntryPointKind::Desktop,
    ]
}

async fn kinds(store: &Arc<MemoryEventLog>) -> Vec<String> {
    store
        .read_all()
        .await
        .expect("read committed facts")
        .into_iter()
        .map(|event| event.kind.as_str().to_owned())
        .collect()
}

/// 【拒绝】Receipt 查询是只读的：四入口同一事实、零 Runner 调用、零新增事件、收据摘要一致。
#[tokio::test]
async fn er27_receipt_query_is_read_only_and_agrees_across_four_entrypoints() {
    let root = ProjectRoot::new("receipt");
    let project = root.path();
    let run_id = RunId::new();
    let store = Arc::new(MemoryEventLog::new());
    committed_run(&store, run_id, project).await;

    let (runner, sends, restores) = zero_runner();
    let host = host(store.clone(), runner);
    let before = kinds(&store).await;

    let mut receipts = Vec::new();
    for entrypoint in four_entrypoints() {
        let envelope = RequestEnvelope::receipt(metadata(project, entrypoint), Some(run_id));
        let response = host.handle(envelope).await;
        assert_eq!(
            response.status,
            ExecutionStatus::Completed,
            "stable response error: {:?}",
            response.error
        );
        let digest = response.output["run_receipt"]["receipt_digest"]
            .as_str()
            .expect("run receipt digest")
            .to_owned();
        receipts.push(digest);
    }

    // A receipt query is a projection: identical digest, no execution, no new fact.
    assert!(
        receipts.windows(2).all(|pair| pair[0] == pair[1]),
        "entrypoint receipts diverged: {receipts:?}"
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(restores.load(Ordering::SeqCst), 0);
    assert_eq!(kinds(&store).await, before);
}

/// 【拒绝】UI 提供的 owner/scope 不能扩大服务端主体：actor、department、role 任一被改写都拒绝。
#[tokio::test]
async fn er27_ui_supplied_owner_or_scope_is_denied() {
    let root = ProjectRoot::new("owner");
    let project = root.path();
    let run_id = RunId::new();
    let store = Arc::new(MemoryEventLog::new());
    committed_run(&store, run_id, project).await;
    let (runner, sends, restores) = zero_runner();
    let host = host(store.clone(), runner);
    let before = kinds(&store).await;

    let mut forged_actor = metadata(project, EntryPointKind::Web);
    forged_actor.actor_id = Some("attacker".to_owned());
    let envelope = RequestEnvelope::receipt(forged_actor, Some(run_id));
    let response = host.handle(envelope).await;
    assert_eq!(response.status, ExecutionStatus::Blocked);
    let owner_denial = response.error.as_deref().unwrap_or_default();
    assert!(
        owner_denial.contains("AUTH_CALLER_UNTRUSTED"),
        "unexpected owner denial: {:?}",
        response.error
    );

    let mut forged_department = metadata(project, EntryPointKind::Web);
    forged_department.department_id = "finance".to_owned();
    let envelope = RequestEnvelope::receipt(forged_department, Some(run_id));
    let response = host.handle(envelope).await;
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("role_department_mismatch"));

    let mut forged_role = metadata(project, EntryPointKind::Web);
    forged_role.role_id = "not-a-role".to_owned();
    let envelope = RequestEnvelope::receipt(forged_role, Some(run_id));
    let response = host.handle(envelope).await;
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("role_unknown"));

    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(restores.load(Ordering::SeqCst), 0);
    assert_eq!(kinds(&store).await, before);
}

/// 【拒绝】未知 run 的 receipt 查询返回 receipt_not_found，不合成空收据、不启动执行。
#[tokio::test]
async fn er27_receipt_query_never_starts_execution_for_unknown_run() {
    let root = ProjectRoot::new("unknown");
    let project = root.path();
    let store = Arc::new(MemoryEventLog::new());
    committed_run(&store, RunId::new(), project).await;
    let (runner, sends, restores) = zero_runner();
    let host = host(store.clone(), runner);
    let before = kinds(&store).await;

    let missing = RunId::new();
    for entrypoint in four_entrypoints() {
        let envelope = RequestEnvelope::receipt(metadata(project, entrypoint), Some(missing));
        let response = host.handle(envelope).await;
        assert_eq!(response.status, ExecutionStatus::Blocked);
        assert_eq!(response.error.as_deref(), Some("receipt_not_found"));
    }

    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(restores.load(Ordering::SeqCst), 0);
    assert_eq!(kinds(&store).await, before);
}

/// 【成功/回归】terminal run 的 resume/cancel/pending 在四入口得到同一只读答复，零执行副作用。
#[tokio::test]
async fn er27_recovery_commands_agree_and_stay_read_only() {
    let root = ProjectRoot::new("recovery");
    let project = root.path();
    let run_id = RunId::new();
    let store = Arc::new(MemoryEventLog::new());
    committed_run(&store, run_id, project).await;
    let (runner, sends, restores) = zero_runner();
    let host = host(store.clone(), runner);

    let mut reasons = Vec::new();
    for entrypoint in four_entrypoints() {
        // resume on a terminal run is a structured Unknown, never a silent restart.
        let envelope = RequestEnvelope::resume_run(metadata(project, entrypoint), Some(run_id));
        let resumed = host.handle(envelope).await;
        assert_eq!(resumed.status, ExecutionStatus::ResultUnknown);
        assert_eq!(resumed.output["reason"], json!("run_resume_terminal_completed"));
        reasons.push(resumed.error.clone());

        // cancel on a terminal run is an idempotent read of the known terminal state.
        let envelope =
            RequestEnvelope::cancel_run(metadata(project, entrypoint), Some(run_id), "er27");
        let cancelled = host.handle(envelope).await;
        assert_eq!(cancelled.status, ExecutionStatus::Completed);
        assert_eq!(cancelled.output["already_terminal"], json!(true));

        // pending approvals are a read-only projection, empty here, still Completed.
        let envelope =
            RequestEnvelope::pending_approvals(metadata(project, entrypoint), Some(run_id));
        let pending = host.handle(envelope).await;
        assert_eq!(pending.status, ExecutionStatus::Completed);
        assert_eq!(pending.output["approvals"], json!([]));
    }

    assert!(
        reasons.windows(2).all(|pair| pair[0] == pair[1]),
        "entrypoint resume denials diverged: {reasons:?}"
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(restores.load(Ordering::SeqCst), 0);
    // Recovery commands may (re)commit authority/session facts, but must never append an execution
    // or a new terminal fact for a run that is already terminal.
    for forbidden in [
        "execution.prepared",
        "invocation.dispatching",
        "run.resume_prepared",
        "run.cancelled",
    ] {
        assert!(
            !kinds(&store).await.iter().any(|kind| kind == forbidden),
            "recovery read path appended {forbidden}"
        );
    }
}

/// 【成功/回归】同一畸形请求在四入口得到同一错误码（入口不能各自翻译拒绝）。
#[tokio::test]
async fn er27_same_denial_error_code_across_four_entrypoints() {
    let root = ProjectRoot::new("denial");
    let project = root.path();
    let run_id = RunId::new();
    let store = Arc::new(MemoryEventLog::new());
    committed_run(&store, run_id, project).await;
    let (runner, sends, _restores) = zero_runner();
    let host = host(store.clone(), runner);
    let before = kinds(&store).await;

    let mut codes = Vec::new();
    for entrypoint in four_entrypoints() {
        let mut envelope = RequestEnvelope::receipt(metadata(project, entrypoint), Some(run_id));
        envelope.schema = "kiana.protocol.v0".to_owned();
        let response = host.handle(envelope).await;
        assert_eq!(response.status, ExecutionStatus::Blocked);
        codes.push(response.error);
    }
    assert_eq!(codes[0].as_deref(), Some("protocol_schema_unsupported"));
    assert!(
        codes.windows(2).all(|pair| pair[0] == pair[1]),
        "entrypoint denial codes diverged: {codes:?}"
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(kinds(&store).await, before);
}
