//! Composition root for Kiana control-plane adapters.

mod apply_patch;
#[cfg(test)]
mod approval_store;
mod authn;
mod connectors;
pub mod container_environment;
mod context_query;
mod data_governance;
pub mod eval_runtime;
mod execution_control;
mod execution_output;
mod execution_workspace;
mod extensions;
mod harness_capabilities;
mod harness_mcp;
mod harness_memory;
mod harness_sandbox;
mod harness_skills;
mod instance;
mod journal_approvals;
mod local_packages;
mod mcp_http;
mod mcp_stdio;
mod memory_retrieval;
mod model_client;
mod pre_tool_hooks;
mod process_supervisor;
mod run_stream;
mod shell_plan;
mod storage;
mod workflow_ingress;
mod workflow_service;
mod workspace_checkpoints;

pub use authn::LocalAuthnAdapter;
pub use instance::{
    discover as discover_instance, validate_peer as validate_instance_peer, InstanceLease,
};
use journal_approvals::JournalApprovalStore;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{ControlPlane, ControlPlaneRuntimeConfig};
pub use kiana_domain::StreamingRedactor;
use kiana_domain::{
    AuthenticatedPrincipalRef, CommandIntent, ComponentHealth, ComponentHealthState,
    CredentialRecoveryProjection, DepartmentSpec, HealthProbeKind, HealthSnapshot,
    IdentityMigration, OperatorEvidenceSnapshot, OrganizationId, PermissionProfile, ProjectIdentity,
    ProjectTrustSnapshot, project_ui_snapshot, RequestContext, ResolvedAssignment, RoleSpec, RunId,
    RuntimeEvent, UiActionCommand, UiActionRecord, UiSnapshotPage, UiSnapshotQuery,
};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{
    AssignmentDirectoryPort, InMemoryAssignmentDirectory, ObservabilityQueue,
    ObservabilityQueueClass, ObservabilityQueueError, ObservabilityQueueStats, PortError,
    QueuedObservabilityItem, RunnerPort, WorkflowQueueStore,
};
use kiana_protocol::{
    RequestBody, RequestEnvelope, RequestMetadata, ResponseEnvelope, UiAction, UiCursor,
    UiFeedCursorV1, UiFeedFrameV1, UiSnapshot, PROTOCOL_SCHEMA,
};
use kiana_runner::{HarnessBudgetConfig, HarnessBudgetSource, KianaHarness, RuntimeConfig};
use run_stream::RunStreamBus;
pub use run_stream::{
    RunStreamFeedError, RunStreamFeedSubscription, RunStreamSubscription, UiFeedBackpressureMetrics,
    UI_FEED_QUEUE_CAPACITY,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
pub use storage::{resolve_storage_root, StorageLease};
pub use workflow_ingress::WorkflowEventVerifier;
pub use workflow_service::{
    WorkflowQueueService, WorkflowQueueShutdownReport, WORKFLOW_SERVICE_CHANNEL_CAPACITY,
};

const ENV_HARNESS_MAX_STEPS: &str = "KIANA_HARNESS_MAX_STEPS";
const ENV_HARNESS_WALL_TIME_MS: &str = "KIANA_HARNESS_WALL_TIME_MS";
const ENV_HARNESS_MAX_ATTEMPTS: &str = "KIANA_HARNESS_MAX_ATTEMPTS";
const ENV_HARNESS_MAX_TOOL_CALLS: &str = "KIANA_HARNESS_MAX_TOOL_CALLS";
const ENV_HARNESS_MAX_REPAIRS: &str = "KIANA_HARNESS_MAX_REPAIRS";
const ENV_HARNESS_MAX_COMPACTIONS: &str = "KIANA_HARNESS_MAX_COMPACTIONS";
const ENV_HARNESS_MAX_TOKENS: &str = "KIANA_HARNESS_MAX_TOKENS";
const ENV_HARNESS_TASK_WALL_TIME_MS: &str = "KIANA_HARNESS_TASK_WALL_TIME_MS";
const OBSERVABILITY_QUEUE_CAPACITY: usize = 1_024;

/// Validate optional protected-transport metadata before any request reaches the ControlPlane.
///
/// Legacy clients may omit these fields. When present, instance/origin/host/credential metadata
/// is only a narrow ingress assertion: it never creates a Principal or grants a role.
pub fn validate_protected_ingress(metadata: &RequestMetadata) -> Result<(), PortError> {
    if metadata
        .instance_id
        .as_deref()
        .is_some_and(|id| id.trim().is_empty() || id.len() > 256 || id.contains('\0'))
    {
        return Err(PortError::Failed("ingress_instance_invalid".to_owned()));
    }
    for (value, field) in [(&metadata.origin, "origin"), (&metadata.host, "host")] {
        if let Some(value) = value.as_deref() {
            if value.trim().is_empty() || !loopback_authority(value) {
                return Err(PortError::Failed(format!("ingress_{field}_not_loopback")));
            }
        }
    }
    if let Some(reference) = &metadata.credential_ref {
        reference.validate().map_err(|error| {
            PortError::Failed(format!("ingress_credential_ref_invalid:{error}"))
        })?;
    }
    if let Some(mode) = metadata.identity_mode.as_deref() {
        match mode {
            "legacy_local_user" => {}
            "protected_local" => {
                if metadata.instance_id.is_none() || metadata.credential_ref.is_none() {
                    return Err(PortError::Failed(
                        "ingress_protected_credentials_required".to_owned(),
                    ));
                }
            }
            _ => {
                return Err(PortError::Failed(
                    "ingress_identity_mode_invalid".to_owned(),
                ))
            }
        }
    }
    Ok(())
}

fn loopback_authority(value: &str) -> bool {
    let trimmed = value.trim();
    let authority = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed)
        .split('/')
        .next()
        .unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

pub struct DaemonHost {
    core: Arc<ControlPlane>,
    principal: AuthenticatedPrincipal,
    authn: LocalAuthnAdapter,
    assignment_directory: InMemoryAssignmentDirectory,
    project_authority: Arc<dyn ProjectTrustAuthority>,
    run_stream: Arc<RunStreamBus>,
    observability_queue: Arc<ObservabilityQueue>,
    workflow_service: WorkflowQueueService,
    /// Optional registry handle used only to build read-only visibility projections. Mutation
    /// and execution remain owned by ControlPlane/Broker; hosts constructed around an injected
    /// core simply report the projection as unavailable.
    extensions: Option<Arc<extensions::ExtensionRegistry>>,
}

pub trait ProjectTrustAuthority: Send + Sync {
    fn project_trusted(&self, project_root: &Path) -> Result<bool, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StoredProjectTrustAuthority;

impl ProjectTrustAuthority for StoredProjectTrustAuthority {
    fn project_trusted(&self, project_root: &Path) -> Result<bool, String> {
        kiana_types::read_project_trust(project_root)
            .map(|trust| trust.is_some_and(kiana_types::ProjectTrust::as_bool))
    }
}

#[derive(Clone, Debug)]
struct AuthenticatedPrincipal {
    identity: AuthenticatedPrincipalRef,
    actor_id: String,
    allowed_roles: Vec<String>,
}

impl AuthenticatedPrincipal {
    fn local() -> Self {
        let identity = AuthenticatedPrincipalRef::local();
        Self {
            actor_id: identity.principal_id.clone(),
            identity,
            allowed_roles: std::env::var("KIANA_LOCAL_ALLOWED_ROLES")
                .map(|value| {
                    value
                        .split(',')
                        .map(|role| role.trim().to_owned())
                        .filter(|role| !role.is_empty())
                        .collect()
                })
                .unwrap_or_else(|_| {
                    RoleSpec::catalog()
                        .iter()
                        .map(|role| role.role_id.clone())
                        .collect()
                }),
        }
    }
}

impl DaemonHost {
    pub fn new(core: Arc<ControlPlane>) -> Self {
        Self::new_with_project_authority(core, Arc::new(StoredProjectTrustAuthority))
    }

    pub fn new_with_project_authority(
        core: Arc<ControlPlane>,
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Self {
        Self::with_run_stream(core, project_authority, Arc::new(RunStreamBus::default()))
    }

    fn with_run_stream(
        core: Arc<ControlPlane>,
        project_authority: Arc<dyn ProjectTrustAuthority>,
        run_stream: Arc<RunStreamBus>,
    ) -> Self {
        let principal = AuthenticatedPrincipal::local();
        let authn = LocalAuthnAdapter::new(principal.identity.clone())
            .expect("local authenticated principal must validate");
        Self {
            core,
            principal,
            authn,
            assignment_directory: InMemoryAssignmentDirectory::new(),
            project_authority,
            run_stream,
            observability_queue: Arc::new(
                ObservabilityQueue::new(OBSERVABILITY_QUEUE_CAPACITY)
                    .expect("static observability queue capacity is non-zero"),
            ),
            workflow_service: WorkflowQueueService::new(),
            extensions: None,
        }
    }

    /// Return the local compatibility authn adapter. Session state is process-local and does not
    /// itself grant a role or capability; each request still enters SecurityContext/ControlPlane.
    pub fn authn_adapter(&self) -> LocalAuthnAdapter {
        self.authn.clone()
    }

    /// 只读解析一个角色的 harness runtime 配置。
    ///
    /// 显式环境开关 > 角色快照 > harness 默认值。
    pub fn harness_runtime_config(
        &self,
        role: Option<RoleSpec>,
    ) -> Result<RuntimeConfig, PortError> {
        harness_runtime_config_from_env().map(|config| config.into_runtime_config(role))
    }

    /// Return the daemon-resolved principal without exposing a credential value.
    pub fn authenticated_principal(&self) -> AuthenticatedPrincipalRef {
        self.principal.identity.clone()
    }

    /// Build the explicit compatibility fact used when migrating the historical local-user
    /// principal. The migration object is metadata only; callers still need a protected identity
    /// resolver before it can be used for authorization.
    pub fn legacy_local_user_migration(
        &self,
        migration_id: impl Into<String>,
        principal_id: impl Into<String>,
        reason: impl Into<String>,
        migrated_at_unix_ms: u64,
    ) -> Result<IdentityMigration, PortError> {
        IdentityMigration::new(
            migration_id,
            self.principal.identity.principal_id.clone(),
            principal_id,
            reason,
            migrated_at_unix_ms,
        )
        .map_err(PortError::Failed)
    }

    /// Return the server-owned assignment port. The returned adapter is a shared snapshot handle;
    /// registration and revocation still pass through its validated directory methods.
    pub fn assignment_directory(&self) -> InMemoryAssignmentDirectory {
        self.assignment_directory.clone()
    }

    /// Resolve an assignment for a filesystem project using only daemon-owned identity material.
    /// The organization and requested role are lookup inputs; they do not grant authority when the
    /// directory has no matching server assignment.
    pub async fn resolve_assignment_for_project(
        &self,
        project_root: &str,
        organization_id: OrganizationId,
        role_id: &str,
        now_unix_ms: u64,
    ) -> Result<ResolvedAssignment, PortError> {
        let project = self.project_identity(project_root)?;
        self.assignment_directory
            .resolve_assignment(
                &self.principal.identity,
                organization_id,
                project.project_id,
                role_id,
                now_unix_ms,
            )
            .await
    }

    /// Build a Company request context from a resolved server assignment. A caller-supplied actor
    /// or role that conflicts with the assignment is rejected before ControlPlane admission.
    pub fn context_from_assignment(
        &self,
        mut context: RequestContext,
        assignment: &ResolvedAssignment,
        requested_role_id: Option<&str>,
        now_unix_ms: u64,
        write: bool,
    ) -> Result<RequestContext, PortError> {
        assignment
            .validate()
            .map_err(|error| PortError::Failed(format!("assignment_invalid:{error}")))?;
        if assignment.principal != self.principal.identity {
            return Err(PortError::Failed(
                "assignment_principal_mismatch".to_owned(),
            ));
        }
        if requested_role_id.is_some_and(|role| role != assignment.role_id) {
            return Err(PortError::Failed("assignment_role_mismatch".to_owned()));
        }
        if context
            .actor_id
            .as_deref()
            .is_some_and(|actor| actor != assignment.principal.principal_id)
        {
            return Err(PortError::Failed("assignment_actor_mismatch".to_owned()));
        }
        context.actor_id = Some(assignment.principal.principal_id.clone());
        context.role_id = assignment.role_id.clone();
        context.department_id = assignment.department_id.clone();
        let project_trust = self.project_trust_snapshot(&context.project_root, 1)?;
        let department = DepartmentSpec::lookup(&assignment.department_id)
            .ok_or_else(|| PortError::Failed("department_unknown".to_owned()))?;
        let department = kiana_domain::DepartmentSnapshot::from_spec(
            &department,
            assignment.assignment_revision,
            assignment.authority_epoch,
        )
        .map_err(PortError::Failed)?;
        let authority = kiana_core::SecurityAuthoritySnapshot::from_parts(
            kiana_domain::SecurityContextId::new(),
            self.principal.identity.clone(),
            project_trust,
            assignment.clone(),
            department,
            assignment.authority_epoch,
        )
        .map_err(PortError::Failed)?;
        authority
            .validate_request(&context)
            .map_err(PortError::Failed)?;
        kiana_core::validate_company_assignment(&context, assignment, now_unix_ms, write)
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        Ok(context)
    }

    /// Resolve a stable project identity from the daemon's filesystem authority.
    ///
    /// This is a scope snapshot only. It does not trust a wire `project_trusted` bit and does not
    /// grant a role or capability; the request still goes through ControlPlane admission.
    pub fn project_identity(&self, project_root: &str) -> Result<ProjectIdentity, PortError> {
        let canonical = std::fs::canonicalize(project_root)
            .map_err(|_| PortError::Failed("project_identity_unavailable".to_owned()))?;
        let metadata = std::fs::metadata(&canonical)
            .map_err(|_| PortError::Failed("project_identity_unavailable".to_owned()))?;
        if !metadata.is_dir() {
            return Err(PortError::Failed(
                "project_identity_not_directory".to_owned(),
            ));
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        #[cfg(unix)]
        let device = Some(metadata.dev());
        #[cfg(not(unix))]
        let device = None;
        #[cfg(unix)]
        let inode = Some(metadata.ino());
        #[cfg(not(unix))]
        let inode = None;
        let trusted = self
            .project_authority
            .project_trusted(Path::new(project_root))
            .map_err(|_| PortError::Failed("project_trust_unavailable".to_owned()))?;
        let trust_revision = kiana_domain::json_digest(&serde_json::json!({"trusted":trusted}));
        ProjectIdentity::new(
            project_root.to_owned(),
            canonical.to_string_lossy().into_owned(),
            device,
            inode,
            trust_revision,
        )
        .map_err(PortError::Failed)
    }

    /// Build a server-owned project trust snapshot without accepting the wire trust bit.
    pub fn project_trust_snapshot(
        &self,
        project_root: &str,
        revision: u64,
    ) -> Result<ProjectTrustSnapshot, PortError> {
        let project = self.project_identity(project_root)?;
        let trusted = self
            .project_authority
            .project_trusted(Path::new(project_root))
            .map_err(|_| PortError::Failed("project_trust_unavailable".to_owned()))?;
        ProjectTrustSnapshot::from_project(&project, trusted, "daemon.project_authority", revision)
            .map_err(PortError::Failed)
    }

    /// Acquire the one local instance lease for a workspace and write a ready record. The lease
    /// is discovery metadata only; every command still returns through ControlPlane.
    pub fn acquire_instance(
        &self,
        workspace: impl AsRef<Path>,
        transport: kiana_protocol::UiTransportKind,
        endpoint: &str,
    ) -> Result<InstanceLease, PortError> {
        instance::InstanceLease::acquire(workspace, transport, endpoint)
    }

    /// Resolve the one user-level storage root for every daemon-backed surface. The root is
    /// derived outside the project tree and bound to the authenticated daemon owner/instance;
    /// this helper only resolves metadata and does not authorize a command.
    pub fn storage_root(
        &self,
        project_root: impl AsRef<Path>,
        instance_id: impl Into<String>,
        authority_epoch: u64,
    ) -> Result<kiana_domain::StorageRoot, PortError> {
        resolve_storage_root(
            project_root.as_ref(),
            self.principal.actor_id.clone(),
            instance_id,
            authority_epoch,
        )
    }

    /// Open the daemon's single-writer storage lease after root/owner/instance validation.
    pub fn acquire_storage(
        &self,
        project_root: impl AsRef<Path>,
        instance_id: impl Into<String>,
        authority_epoch: u64,
    ) -> Result<StorageLease, PortError> {
        let root = self.storage_root(project_root, instance_id, authority_epoch)?;
        StorageLease::acquire(root)
    }

    /// Subscribe to additive run-stream events for one run.
    ///
    /// The subscription must be created before the run starts to observe deltas. It remains
    /// usable until the terminal event or until the receiver is dropped.
    pub fn subscribe_run(&self, run_id: RunId) -> RunStreamSubscription {
        self.run_stream.subscribe(run_id)
    }

    /// Resume a display subscription; omitted deltas require snapshot reconciliation.
    pub fn subscribe_run_after(
        &self,
        run_id: RunId,
        cursor: Option<&UiCursor>,
    ) -> RunStreamSubscription {
        self.run_stream.subscribe_after(run_id, cursor)
    }

    /// Subscribe to the versioned feed projection.  Replay is bounded by the daemon-owned
    /// window; stale/foreign cursors produce an explicit gap frame requiring a fresh snapshot.
    pub fn subscribe_run_feed_after(
        &self,
        run_id: RunId,
        cursor: Option<&UiFeedCursorV1>,
    ) -> RunStreamFeedSubscription {
        self.run_stream.subscribe_feed_after(run_id, cursor)
    }

    pub fn feed_instance_id(&self) -> String {
        self.run_stream.instance_id().to_owned()
    }

    pub fn feed_backpressure_metrics(&self) -> UiFeedBackpressureMetrics {
        self.run_stream.feed_metrics()
    }

    pub fn feed_heartbeat(&self, run_id: RunId) -> Result<UiFeedFrameV1, PortError> {
        self.run_stream
            .heartbeat(run_id)
            .map_err(PortError::Failed)
    }

    pub fn ui_cursor(&self) -> UiCursor {
        self.run_stream.ui_cursor()
    }

    pub fn run_stream_cursor(&self, run_id: RunId) -> UiCursor {
        self.run_stream.run_cursor(run_id)
    }

    /// A UI precondition only: the command still passes through ControlPlane authorization.
    pub fn claim_ui_action(&self, action: &UiAction) -> Result<UiCursor, PortError> {
        self.run_stream.claim_ui_action(action)
    }

    /// Persist a UI action admission through the ControlPlane journal.  The legacy cursor claim
    /// above remains a cheap display precondition; it never replaces this durable authority.
    pub async fn admit_ui_action(
        &self,
        action: &UiActionCommand,
        authority: &kiana_core::UiActionAuthoritySnapshot,
        now_unix_ms: u64,
    ) -> Result<UiActionRecord, PortError> {
        self.core
            .admit_ui_action(action, authority, now_unix_ms)
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub async fn apply_ui_action(
        &self,
        action: &UiActionCommand,
        receipt_digest: &str,
        now_unix_ms: u64,
    ) -> Result<UiActionRecord, PortError> {
        self.core
            .apply_ui_action(action, receipt_digest, now_unix_ms)
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub async fn reject_ui_action(
        &self,
        action: &UiActionCommand,
        reason: &str,
    ) -> Result<UiActionRecord, PortError> {
        self.core
            .reject_ui_action(action, reason)
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub async fn mark_ui_action_unknown(
        &self,
        action: &UiActionCommand,
        reason: &str,
    ) -> Result<UiActionRecord, PortError> {
        self.core
            .mark_ui_action_unknown(action, reason)
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub async fn query_original_ui_action(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<UiActionRecord>, PortError> {
        self.core
            .query_original_ui_action(idempotency_key)
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    /// Project one owner-scoped snapshot from a single EventStore read.  The domain projector
    /// rejects projection lag, retention-protected pending work, stale page cursors and cross-
    /// owner sessions; an unsupported or failed source read is never turned into an empty page.
    pub async fn ui_snapshot_page(
        &self,
        query: &UiSnapshotQuery,
    ) -> Result<UiSnapshotPage, PortError> {
        query.validate().map_err(PortError::Failed)?;
        let events = self
            .persisted_events()
            .await?
            .ok_or_else(|| PortError::Failed("ui_snapshot_projection_unknown".to_owned()))?;
        project_ui_snapshot(&events, query).map_err(PortError::Failed)
    }

    /// Build a disposable UI snapshot exclusively from this principal's event facts.
    pub async fn ui_snapshot(&self, session_id: &str) -> Result<UiSnapshot, PortError> {
        let cursor = self.ui_cursor();
        let events = self
            .ui_events()
            .await?
            .ok_or_else(|| PortError::Failed("ui_snapshot_unsupported".to_owned()))?;
        let run_id = events.iter().rev().find_map(|event| {
            (event.kind == "run.authorized"
                && event.data["session_id"].as_str() == Some(session_id)
                && event.data["actor_id"].as_str() == Some(self.principal.actor_id.as_str()))
            .then(|| event.data["run_id"].as_str().and_then(RunId::parse_str))
            .flatten()
        });
        let status = if let Some(run_id) = run_id {
            let projection = kiana_core::project_run_state(run_id, &events)
                .map_err(|_| PortError::Failed("run_terminal_conflict".to_owned()))?;
            Some(match projection.outcome {
                Some(kiana_core::RunOutcome::Completed) => kiana_domain::ExecutionStatus::Completed,
                Some(kiana_core::RunOutcome::Failed) => kiana_domain::ExecutionStatus::Failed,
                Some(kiana_core::RunOutcome::Cancelled) => kiana_domain::ExecutionStatus::Cancelled,
                Some(kiana_core::RunOutcome::ResultUnknown) => {
                    kiana_domain::ExecutionStatus::ResultUnknown
                }
                None if projection.phase == kiana_core::RunPhase::AwaitingApproval => {
                    kiana_domain::ExecutionStatus::AwaitingApproval
                }
                None => kiana_domain::ExecutionStatus::Running,
            })
        } else {
            None
        };
        let mut pending = std::collections::BTreeMap::new();
        if let Some(run_id) = run_id {
            for event in &events {
                let Some(id) = event.data["approval_id"].as_str() else {
                    continue;
                };
                if event.kind == "approval.requested"
                    && event.data["run_id"].as_str() == Some(run_id.to_string().as_str())
                {
                    pending.insert(id.to_owned(), event.data.clone());
                } else if matches!(
                    event.kind.as_str(),
                    "approval.approved"
                        | "approval.denied"
                        | "approval.cancelled"
                        | "approval.expired"
                ) {
                    pending.remove(id);
                }
            }
        }
        Ok(UiSnapshot {
            schema: PROTOCOL_SCHEMA.to_owned(),
            cursor,
            session_id: session_id.to_owned(),
            run_id,
            stream_cursor: run_id.map(|run_id| self.run_stream_cursor(run_id)),
            status,
            pending_actions: pending.into_values().take(128).collect(),
        })
    }

    /// Return the one server-owned extension visibility snapshot used by CLI, Workbench, Web and
    /// Desktop adapters. The projection contains redacted metadata only; callers must submit any
    /// activation/revoke intent back through `extension.manage`, where the current registry,
    /// approval and Broker admission are checked again.
    pub async fn extension_visibility_snapshot(
        &self,
        project_root: &str,
        role_id: &str,
        query: &str,
        max_results: usize,
    ) -> Result<kiana_protocol::ExtensionVisibilitySnapshot, PortError> {
        let role = RoleSpec::lookup(role_id)
            .ok_or_else(|| PortError::Failed("role_unknown".to_owned()))?;
        if !self.principal.allowed_roles.contains(&role.role_id) {
            return Err(PortError::Failed(
                "principal_role_not_authorized".to_owned(),
            ));
        }
        if project_root.trim().is_empty() {
            return Err(PortError::Failed("project_root_required".to_owned()));
        }
        if query.len() > kiana_domain::MAX_EXTENSION_VISIBILITY_QUERY_BYTES
            || query.contains('\0')
            || max_results == 0
            || max_results > kiana_domain::MAX_EXTENSION_VISIBILITY_ENTRIES
        {
            return Err(PortError::Failed(
                "extension_visibility_query_invalid".to_owned(),
            ));
        }
        let Some(extensions) = &self.extensions else {
            return Err(PortError::Unavailable(
                "extension_visibility_unsupported".to_owned(),
            ));
        };
        extensions
            .visibility_snapshot(project_root, &role.role_id, query, max_results)
            .await
    }

    /// Limit display history to runs bound to this authenticated local principal.
    pub async fn ui_events(&self) -> Result<Option<Vec<RuntimeEvent>>, PortError> {
        let Some(events) = self.persisted_events().await? else {
            return Ok(None);
        };
        let owned: std::collections::HashSet<_> = events
            .iter()
            .filter(|event| {
                event.kind == "run.authorized"
                    && event.data["actor_id"].as_str() == Some(self.principal.actor_id.as_str())
            })
            .filter_map(|event| event.data["run_id"].as_str().map(str::to_owned))
            .collect();
        let requests: std::collections::HashSet<_> = events
            .iter()
            .filter(|event| {
                event.data["run_id"]
                    .as_str()
                    .is_some_and(|run_id| owned.contains(run_id))
            })
            .map(|event| event.request_id)
            .collect();
        Ok(Some(
            events
                .into_iter()
                .filter(|event| {
                    if let Some(run_id) = event.data["run_id"].as_str() {
                        return owned.contains(run_id);
                    }
                    requests.contains(&event.request_id)
                })
                .collect(),
        ))
    }

    /// 只读地读取本实例账本里的全部事件，供展示层做只读投影。
    ///
    /// 展示层不得自己解析账本文件：路径推导、torn-tail 容忍和 symlink 拒绝都由
    /// `EventStorePort` 的实现负责。`Ok(None)` 表示该存储不支持全量读取；读取失败
    /// 照原样返回，调用方不得把它当成空账本。
    pub async fn persisted_events(&self) -> Result<Option<Vec<RuntimeEvent>>, PortError> {
        self.core
            .persisted_events()
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    /// Rebuild the credential/config recovery projection from the EventLog.  This is a read-only
    /// bridge: it never refreshes credentials, consumes a lease or resumes a run.  Callers must
    /// still commit an explicit re-admission fact before any continuation can be authorized.
    pub async fn credential_recovery_projection(
        &self,
        request: &kiana_core::CredentialRecoveryReplayRequest,
    ) -> Result<Option<CredentialRecoveryProjection>, PortError> {
        let Some(events) = self.persisted_events().await? else {
            return Err(PortError::Unavailable(
                "credential_recovery_projection_unsupported".to_owned(),
            ));
        };
        kiana_core::project_credential_recovery(&events, request)
            .map(Some)
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub async fn flush_event_store(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.core
            .flush_event_store()
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub async fn event_store_health(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.core
            .event_store_health()
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub async fn last_durable_cursor(&self) -> Result<kiana_domain::EventCursor, PortError> {
        self.core
            .last_durable_cursor()
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub async fn close_event_store(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.core
            .close_event_store()
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    /// Graceful shutdown boundary: flush committed facts before draining the best-effort
    /// observability queue, then close the EventStore. A close/flush error is returned as
    /// `result_unknown` by callers; dropping the host is never treated as a terminal ack.
    pub async fn shutdown(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        let _ = self.workflow_service.shutdown().await?;
        self.flush_event_store().await?;
        let _ = self.flush_observability().await;
        self.shutdown_observability();
        self.close_event_store().await
    }

    /// Enqueue a redacted observation without blocking the ControlPlane or EventStore commit.
    pub fn try_enqueue_observability(
        &self,
        item: QueuedObservabilityItem,
    ) -> Result<(), ObservabilityQueueError> {
        self.observability_queue.try_enqueue(item)
    }

    pub fn observability_queue(&self) -> Arc<ObservabilityQueue> {
        self.observability_queue.clone()
    }

    pub async fn flush_observability(&self) -> u64 {
        self.observability_queue.flush().await
    }

    pub fn shutdown_observability(&self) -> ObservabilityQueueStats {
        self.observability_queue.shutdown()
    }

    /// Start the one bounded workflow queue service owned by this DaemonHost. The service only
    /// claims/heartbeats/fences queue leases; command admission and capability execution remain
    /// on the existing ControlPlane/Broker spine.
    pub async fn start_workflow_queue_service(
        &self,
        store: Arc<dyn WorkflowQueueStore>,
    ) -> Result<(), PortError> {
        self.workflow_service.start(store).await
    }

    pub fn workflow_queue_service(&self) -> WorkflowQueueService {
        self.workflow_service.clone()
    }

    pub fn reopen_observability(&self) -> ObservabilityQueueStats {
        self.observability_queue.reopen()
    }

    pub fn enqueue_terminal_observation(
        &self,
        source_cursor: u64,
    ) -> Result<(), ObservabilityQueueError> {
        self.try_enqueue_observability(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Terminal,
            source_cursor,
        ))
    }

    /// Read-only startup/readiness/liveness projection assembled by the ControlPlane.
    ///
    /// The daemon component is marked healthy only because this host successfully served the
    /// projection; readiness still follows the snapshot's stricter admission status.
    pub async fn health_snapshot(
        &self,
        probe: HealthProbeKind,
    ) -> Result<HealthSnapshot, PortError> {
        let mut snapshot = self
            .core
            .health_snapshot(probe)
            .await
            .map_err(|error| PortError::Failed(error.to_string()))?;
        let daemon_state = if matches!(snapshot.status, kiana_domain::SignalStatus::Ok) {
            ComponentHealthState::Healthy
        } else {
            ComponentHealthState::Degraded
        };
        let daemon_limitation = (daemon_state != ComponentHealthState::Healthy)
            .then_some("health_status_degraded".to_owned());
        let daemon = ComponentHealth::new(
            "daemon",
            "daemon.v1",
            daemon_state,
            Some(snapshot.source_cursor),
            daemon_limitation,
        )
        .map_err(|error| PortError::Failed(error.to_owned()))?;
        snapshot.components.insert("daemon".to_owned(), daemon);
        let queue_stats = self.observability_queue.stats();
        if queue_stats.dropped_best_effort_total > 0 || queue_stats.critical_rejected_total > 0 {
            snapshot.status = kiana_domain::SignalStatus::Degraded;
            if snapshot.limitations.len() < kiana_domain::MAX_HEALTH_LIMITATIONS {
                snapshot
                    .limitations
                    .push("observability_queue_drop_or_reject".to_owned());
            }
            let telemetry = ComponentHealth::new(
                "telemetry",
                "telemetry.v1",
                ComponentHealthState::Degraded,
                Some(snapshot.source_cursor),
                Some("observability_queue_drop_or_reject".to_owned()),
            )
            .map_err(|error| PortError::Failed(error.to_owned()))?;
            snapshot
                .components
                .insert("telemetry".to_owned(), telemetry);
        }
        snapshot.snapshot_digest = snapshot.digest();
        snapshot
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        Ok(snapshot)
    }

    pub async fn readiness(&self) -> Result<HealthSnapshot, PortError> {
        self.health_snapshot(HealthProbeKind::Readiness).await
    }

    pub async fn liveness(&self) -> Result<HealthSnapshot, PortError> {
        self.health_snapshot(HealthProbeKind::Liveness).await
    }

    pub async fn startup_health(&self) -> Result<HealthSnapshot, PortError> {
        self.health_snapshot(HealthProbeKind::Startup).await
    }

    /// Read-only operator evidence bound to the same EventLog projection as health and metrics.
    /// Queue depth is an observation only; it cannot authorize work or claim an effect succeeded.
    pub async fn operator_evidence(
        &self,
        probe: HealthProbeKind,
    ) -> Result<OperatorEvidenceSnapshot, PortError> {
        let queue_depth = self.observability_queue.stats().depth as u64;
        self.core
            .operator_evidence_with_queue(probe, Some(queue_depth))
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub fn local() -> Result<Self, PortError> {
        Self::with_runner_and_events(
            Arc::new(configured_env_harness()?),
            Arc::new(JsonlEventLog::open_default()?),
        )
    }

    pub fn local_with_model_config(config: LocalModelConfig) -> Result<Self, PortError> {
        let runtime_config = runtime_config_from_env()?;
        Self::with_runner_and_events(
            Arc::new(KianaHarness::with_config(
                model_client::from_config(config),
                runtime_config,
            )),
            Arc::new(JsonlEventLog::open_default()?),
        )
    }

    pub fn local_with_project_authority(
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_and_authority(
            Arc::new(configured_env_harness()?),
            Arc::new(JsonlEventLog::open_default()?),
            project_authority,
        )
    }

    pub fn with_harness(harness: KianaHarness) -> Result<Self, PortError> {
        Self::with_runner(Arc::new(harness))
    }

    pub fn with_harness_and_project_authority(
        harness: KianaHarness,
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_and_authority(
            Arc::new(harness),
            Arc::new(MemoryEventLog::new()),
            project_authority,
        )
    }

    pub fn with_env_harness() -> Result<Self, PortError> {
        Self::with_runner(Arc::new(configured_env_harness()?))
    }

    pub fn with_env_harness_and_project_authority(
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_and_authority(
            Arc::new(configured_env_harness()?),
            Arc::new(MemoryEventLog::new()),
            project_authority,
        )
    }

    pub fn with_harness_on_disk(
        harness: KianaHarness,
        events_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, PortError> {
        Self::with_runner_and_events(
            Arc::new(harness),
            Arc::new(JsonlEventLog::open(events_path)?),
        )
    }

    pub fn with_harness_on_disk_and_project_authority(
        harness: KianaHarness,
        events_path: impl AsRef<std::path::Path>,
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_and_authority(
            Arc::new(harness),
            Arc::new(JsonlEventLog::open(events_path)?),
            project_authority,
        )
    }

    pub fn with_runner(runner: Arc<dyn RunnerPort>) -> Result<Self, PortError> {
        Self::with_runner_and_events(runner, Arc::new(MemoryEventLog::new()))
    }

    pub fn with_runner_and_events(
        runner: Arc<dyn RunnerPort>,
        events: Arc<dyn kiana_ports::EventStorePort>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_and_authority(
            runner,
            events,
            Arc::new(StoredProjectTrustAuthority),
        )
    }

    fn with_runner_events_and_authority(
        runner: Arc<dyn RunnerPort>,
        events: Arc<dyn kiana_ports::EventStorePort>,
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        kiana_domain::validate_tool_authority().map_err(PortError::Failed)?;
        let run_stream = Arc::new(RunStreamBus::default());
        let events = run_stream::StreamEventStore::wrap(events, run_stream.clone());
        let approvals = Arc::new(JournalApprovalStore::new(events.clone())?);
        let runtime_config = harness_runtime_config_from_env()?;
        runner.install_model_budget(Arc::new(kiana_core::JournalModelBudget::new(
            events.clone(),
        )))?;
        let extensions = extensions::ExtensionRegistry::from_env(events.clone())?;
        let runner =
            harness_skills::SkillAwareRunner::wrap_with_extensions(runner, extensions.clone());
        let runner = run_stream::RunStreamRunner::wrap(runner, run_stream.clone());
        let mut capabilities = CapabilityBroker::new();
        capabilities.set_permit_verifier(Arc::new(kiana_core::JournalPermitVerifier::new(
            events.clone(),
        )));
        context_query::register(&mut capabilities)?;
        harness_capabilities::register(&mut capabilities)?;
        execution_control::ExecutionControl::register(&mut capabilities, events.clone())?;
        let mcp_registry = harness_mcp::McpRegistry::new(events.clone())?;
        harness_mcp::register(&mut capabilities, mcp_registry.clone())?;
        harness_memory::register(&mut capabilities, events.clone())?;
        workspace_checkpoints::register(&mut capabilities)?;
        data_governance::register(&mut capabilities)?;
        connectors::register(&mut capabilities, events.clone())?;
        extensions.register(&mut capabilities)?;
        capabilities.validate_catalog_bindings()?;
        let hooks = Arc::new(pre_tool_hooks::QueryPreToolHooks::new(
            mcp_registry,
            events.clone(),
        )?);
        let core = ControlPlane::with_pre_tool_hooks_and_runtime_config(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events,
            Arc::new(capabilities),
            approvals,
            runner,
            hooks,
            ControlPlaneRuntimeConfig {
                max_steps_per_turn: runtime_config.into_runtime_config(None).max_steps_per_turn,
            },
        )
        .with_role_step_limits(runtime_config.max_steps_override)
        .with_workspace_checkpoints(Arc::new(workspace_checkpoints::LocalWorkspaceCheckpoints));
        let mut host = Self::with_run_stream(
            Arc::new(core),
            project_authority,
            run_stream,
        );
        host.extensions = Some(extensions);
        Ok(host)
    }

    pub async fn handle(&self, request: RequestEnvelope) -> ResponseEnvelope {
        let request_id = request.metadata.request_id;
        if let Err(error) = validate_protected_ingress(&request.metadata) {
            return ResponseEnvelope::rejected(request_id, error.to_string());
        }
        let protected_identity =
            request.metadata.identity_mode.as_deref() == Some("protected_local");
        if let Err(error) = self.authn.validate_if_present(
            request.metadata.session_id.as_str(),
            authn::LocalAuthnAdapter::now_unix_ms(),
            protected_identity,
        ) {
            return ResponseEnvelope::rejected(request_id, error.to_string());
        }
        let publish_run_response = matches!(
            &request.body,
            RequestBody::Run(_)
                | RequestBody::Continue(_)
                | RequestBody::Cancel(_)
                | RequestBody::ApprovalDecision(_)
        );
        if request.schema != PROTOCOL_SCHEMA
            || kiana_domain::check_schema_compatibility(
                &request.schema,
                &kiana_domain::SchemaVersion::new(1, 0),
            )
            .is_err()
        {
            return ResponseEnvelope::rejected(request_id, "protocol_schema_unsupported");
        }
        if let RequestBody::AuditQuery(query) = &request.body {
            if query.validate().is_err() {
                return ResponseEnvelope::rejected(request_id, "audit_query_invalid");
            }
            if request
                .metadata
                .actor_id
                .as_deref()
                .is_none_or(|actor| actor != self.principal.actor_id)
            {
                return ResponseEnvelope::rejected(request_id, "audit_query_unauthenticated");
            }
        }
        if let RequestBody::AuditExport(export) = &request.body {
            if export.validate().is_err() {
                return ResponseEnvelope::rejected(request_id, "audit_export_invalid");
            }
            if request
                .metadata
                .actor_id
                .as_deref()
                .is_none_or(|actor| actor != self.principal.actor_id)
            {
                return ResponseEnvelope::rejected(request_id, "audit_export_unauthenticated");
            }
        }
        if let RequestBody::Parity(_) = &request.body {
            if request
                .metadata
                .actor_id
                .as_deref()
                .is_none_or(|actor| actor != self.principal.actor_id)
            {
                return ResponseEnvelope::rejected(request_id, "parity_unauthenticated");
            }
        }
        let metadata = request.metadata;
        let permission_profile =
            effective_permission_profile(&request.body, metadata.permission_profile);
        if let RequestBody::ApprovalDecision(decision) = &request.body {
            let proof_missing = decision
                .request_hash
                .as_deref()
                .is_none_or(|proof| proof.trim().is_empty())
                || decision
                    .nonce
                    .as_deref()
                    .is_none_or(|proof| proof.trim().is_empty());
            if proof_missing {
                return ResponseEnvelope::rejected(request_id, "approval_proof_required");
            }
        }
        if matches!(&request.body, RequestBody::ApprovalDecision(_))
            && metadata
                .actor_id
                .as_deref()
                .is_some_and(|actor| actor != self.principal.actor_id)
        {
            return ResponseEnvelope::rejected(request_id, "approval_context_mismatch");
        }
        if metadata.session_id.is_empty() {
            return ResponseEnvelope::rejected(request_id, "session_id_required");
        }
        if metadata.project_root.trim().is_empty() {
            return ResponseEnvelope::rejected(request_id, "project_root_required");
        }
        let project_trusted = match self
            .project_authority
            .project_trusted(Path::new(&metadata.project_root))
        {
            Ok(trusted) => trusted,
            Err(_) => {
                return ResponseEnvelope::rejected(request_id, "project_trust_unavailable");
            }
        };
        if metadata
            .actor_id
            .as_deref()
            .is_none_or(|actor| actor.trim().is_empty())
        {
            return ResponseEnvelope::rejected(request_id, "actor_identity_required");
        }

        let Some(role) = RoleSpec::lookup(&metadata.role_id) else {
            return ResponseEnvelope::rejected(request_id, "role_unknown");
        };
        if !self.principal.allowed_roles.contains(&role.role_id) {
            return ResponseEnvelope::rejected(request_id, "principal_role_not_authorized");
        }
        let department = metadata.department_id.trim();
        if !department.is_empty() && department != role.department_id {
            return ResponseEnvelope::rejected(request_id, "role_department_mismatch");
        }

        let project_identity = match self.project_identity(&metadata.project_root) {
            Ok(identity) => identity,
            Err(error) => return ResponseEnvelope::rejected(request_id, error.to_string()),
        };
        let requested_context = RequestContext {
            request_id,
            session_id: metadata.session_id,
            project_root: metadata.project_root,
            actor_id: metadata.actor_id,
            project_trusted: metadata.project_trusted,
            permission_profile,
            role_id: role.role_id.clone(),
            department_id: role.department_id.clone(),
            work_packet_id: None,
            cell_id: None,
            path_allow: Vec::new(),
        };
        let security_context = match self
            .core
            .resolve_security_context(
                &requested_context,
                self.principal.identity.clone(),
                project_identity.clone(),
                project_trusted,
                &role,
            )
            .await
        {
            Ok(context) => context,
            Err(error) => return ResponseEnvelope::rejected(request_id, error.to_string()),
        };
        if request_may_execute(&request.body) && !security_context.project_trusted {
            return ResponseEnvelope::rejected(request_id, "project_untrusted");
        }
        let context = match security_context.apply_to_request(requested_context) {
            Ok(context) => context,
            Err(reason) => return ResponseEnvelope::rejected(request_id, reason),
        };
        if request_may_execute(&request.body) {
            let configuration_revision = kiana_domain::json_digest(&serde_json::json!({
                "project_identity":project_identity,"role_catalog":kiana_domain::RoleCatalog::builtin(),"department_catalog":kiana_domain::DepartmentCatalog::builtin(),"local_roles":self.principal.allowed_roles,
                "model_profiles":std::env::var("KIANA_MODEL_PROFILES_JSON").unwrap_or_default(),
                "policy":"kiana.default-policy.content.v2","tool_catalog_digest":kiana_domain::tool_catalog_digest(),
                "action_catalog":kiana_domain::capability_action_catalog_digest(),
                "security_context_digest":security_context.context_digest,
            }));
            if let Err(error) = self
                .core
                .synchronize_authority(&context, &configuration_revision)
                .await
            {
                return ResponseEnvelope::rejected(request_id, error.to_string());
            }
            if let Err(error) = self.core.bind_session_assignment(&context).await {
                return ResponseEnvelope::rejected(request_id, error.to_string());
            }
        }
        // Trusted local opt-in: consume at most one queued job after a user run finishes.
        // This calls the same core run path and never recursively schedules internal runs.
        let auto_distill = matches!(
            &request.body,
            RequestBody::Run(_)
                | RequestBody::Continue(_)
                | RequestBody::Spawn(_)
                | RequestBody::Symposium(_)
                | RequestBody::ApprovalDecision(_)
        ) && !context
            .session_id
            .as_str()
            .starts_with(kiana_domain::MEMORY_DISTILL_SESSION_PREFIX)
            && matches!(
                std::env::var("KIANA_MEMORY_DISTILL_AUTO").as_deref(),
                Ok("1" | "true")
            );
        let mut distill_context = context.clone();
        distill_context.request_id = kiana_domain::RequestId::new();
        let response = match request.body {
            RequestBody::Command(command) => {
                self.core
                    .handle_command(context, CommandIntent::new(command.name, command.arguments))
                    .await
            }
            RequestBody::ApprovalDecision(decision) => {
                self.core
                    .decide_approval_with_proof_and_version(
                        &context,
                        decision.approval_id,
                        decision.decision,
                        decision.request_hash.as_deref(),
                        decision.nonce.as_deref(),
                        decision.expected_version,
                    )
                    .await
            }
            RequestBody::Run(run) => {
                self.core
                    .start_run_with_history(context, run.prompt, run.history, run.sandbox)
                    .await
            }
            RequestBody::Continue(run) => {
                self.core
                    .continue_run(context, run.prompt, run.sandbox, run.run_id)
                    .await
            }
            RequestBody::Steer(request) => {
                self.core
                    .steer_run(
                        context,
                        request.run_id,
                        request.expected_turn_id,
                        request.text,
                    )
                    .await
            }
            RequestBody::Inject(request) => {
                self.core
                    .inject_run(
                        context,
                        request.run_id,
                        request.target,
                        request.source,
                        request.text,
                        request.target_turn_id,
                    )
                    .await
            }
            RequestBody::Resume(run) => self.core.resume_run(context, run.run_id).await,
            RequestBody::ListApprovals(query) => {
                self.core
                    .list_pending_approvals(&context, query.run_id)
                    .await
            }
            RequestBody::Cancel(run) => self.core.cancel_run(context, run.run_id, run.reason).await,
            RequestBody::Receipt(receipt) => self.core.read_receipt(context, receipt.run_id).await,
            RequestBody::AuditQuery(query) => {
                self.core
                    .query_audit(
                        &context,
                        kiana_core::AuditQueryInput {
                            source_cursor: query.source_cursor,
                            after_cursor: query.after_cursor,
                            limit: query.limit,
                            action_kind: query.action_kind,
                            decision: query.decision,
                            target_kind: query.target_kind,
                            cursor: query.cursor,
                        },
                    )
                    .await
            }
            RequestBody::AuditExport(export) => {
                self.core
                    .export_audit(
                        &context,
                        kiana_core::AuditExportInput {
                            query: kiana_core::AuditQueryInput {
                                source_cursor: export.query.source_cursor,
                                after_cursor: export.query.after_cursor,
                                limit: export.query.limit,
                                action_kind: export.query.action_kind,
                                decision: export.query.decision,
                                target_kind: export.query.target_kind,
                                cursor: export.query.cursor,
                            },
                            format: export.format,
                            purpose: export.purpose,
                            recipient: export.recipient,
                            retention_class: export.retention_class,
                            deliver: export.deliver,
                        },
                    )
                    .await
            }
            RequestBody::Parity(parity) => {
                self.core
                    .entrypoint_parity(&context, parity.entrypoint, parity.run_id)
                    .await
            }
            RequestBody::Spawn(spawn) => {
                self.core
                    .spawn_from_packet(context, spawn.packet, spawn.sandbox)
                    .await
            }
            RequestBody::Symposium(symposium) => {
                self.core
                    .convene_symposium(
                        context,
                        symposium.goal,
                        symposium.anti_meeting,
                        Some(symposium.max_rounds),
                        symposium.sandbox,
                    )
                    .await
            }
            RequestBody::Review(review) => {
                self.core
                    .review_author_run(context, review.author_session_id, review.author_run_id)
                    .await
            }
            RequestBody::Close(close) => {
                self.core
                    .close_author_run(context, close.author_session_id, close.author_run_id)
                    .await
            }
        };
        let response = match response {
            Ok(response) => {
                let mut response = ResponseEnvelope::from_core(response);
                response.request_id = request_id;
                response
            }
            Err(error) => ResponseEnvelope {
                schema: PROTOCOL_SCHEMA.to_owned(),
                request_id,
                status: kiana_domain::ExecutionStatus::Failed,
                output: serde_json::Value::Null,
                error: Some(error.to_string()),
            },
        };
        if publish_run_response && response.status.is_terminal() {
            if let Some(run_id) = response
                .output
                .get("run_id")
                .and_then(|value| value.as_str())
                .and_then(RunId::parse_str)
            {
                self.run_stream.publish_terminal(run_id, response.clone());
            }
        }
        if auto_distill
            && matches!(
                response.status,
                kiana_domain::ExecutionStatus::Completed
                    | kiana_domain::ExecutionStatus::Failed
                    | kiana_domain::ExecutionStatus::Cancelled
                    | kiana_domain::ExecutionStatus::ResultUnknown
            )
        {
            let _ = self
                .core
                .handle_memory_distillation(
                    distill_context,
                    serde_json::json!({"action":"consume"}),
                )
                .await;
        }
        response
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalModelConfig {
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

/// Harness runtime inputs resolved when a daemon is composed.
///
/// A value is set only by the explicit environment override. A role supplies
/// its snapshot only when this override is absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HarnessRuntimeConfig {
    pub max_steps_override: Option<u32>,
    pub wall_time_override: Option<Duration>,
    pub budget: HarnessBudgetConfig,
}

impl HarnessRuntimeConfig {
    fn into_runtime_config(self, role: Option<RoleSpec>) -> RuntimeConfig {
        let mut config = RuntimeConfig::default();
        config.budget = self.budget;
        config.wall_time_budget = self.wall_time_override;
        if let Some(max_steps) = self
            .max_steps_override
            .or_else(|| role.map(|role| role.max_steps))
        {
            config.max_steps_per_turn = max_steps;
            if self.max_steps_override.is_none()
                && self.budget.source == HarnessBudgetSource::Default
            {
                config.budget.max_model_steps_per_turn =
                    config.budget.max_model_steps_per_turn.min(max_steps);
                config.budget.source = HarnessBudgetSource::Role;
            }
        }
        config
    }
}

fn configured_env_harness() -> Result<KianaHarness, PortError> {
    let config = runtime_config_from_env()?;
    Ok(KianaHarness::with_config(model_client::from_env(), config))
}

pub fn runtime_config_from_env() -> Result<RuntimeConfig, PortError> {
    runtime_config_from_lookup(|name| std::env::var(name))
}

fn harness_runtime_config_from_env() -> Result<HarnessRuntimeConfig, PortError> {
    harness_runtime_config_from_lookup(|name| std::env::var(name))
}

fn harness_runtime_config_from_lookup(
    mut lookup: impl FnMut(&str) -> Result<String, std::env::VarError>,
) -> Result<HarnessRuntimeConfig, PortError> {
    let mut config = HarnessRuntimeConfig::default();
    let mut environment_override = false;
    if let Some(max_steps) = parse_u32_override(&mut lookup, ENV_HARNESS_MAX_STEPS)? {
        config.max_steps_override = Some(max_steps);
        config.budget.max_model_steps_per_turn = max_steps;
        environment_override = true;
    }
    if let Some(wall_time_ms) = parse_u64_override(&mut lookup, ENV_HARNESS_WALL_TIME_MS)? {
        config.wall_time_override = Some(Duration::from_millis(wall_time_ms));
        environment_override = true;
    }
    if let Some(value) = parse_u32_override(&mut lookup, ENV_HARNESS_MAX_ATTEMPTS)? {
        config.budget.max_attempts_per_task = value;
        environment_override = true;
    }
    if let Some(value) = parse_u32_override(&mut lookup, ENV_HARNESS_MAX_TOOL_CALLS)? {
        config.budget.max_tool_calls_per_task = value;
        environment_override = true;
    }
    if let Some(value) = parse_u32_override(&mut lookup, ENV_HARNESS_MAX_REPAIRS)? {
        config.budget.max_repairs_per_task = value;
        environment_override = true;
    }
    if let Some(value) = parse_u32_override(&mut lookup, ENV_HARNESS_MAX_COMPACTIONS)? {
        config.budget.max_compactions_per_task = value;
        environment_override = true;
    }
    if let Some(value) = parse_u64_override(&mut lookup, ENV_HARNESS_MAX_TOKENS)? {
        config.budget.max_tokens_per_task = value;
        environment_override = true;
    }
    if let Some(value) = parse_u64_override(&mut lookup, ENV_HARNESS_TASK_WALL_TIME_MS)? {
        config.budget.max_wall_time_per_task = Some(Duration::from_millis(value));
        environment_override = true;
    }
    if environment_override {
        config.budget.source = HarnessBudgetSource::Environment;
    }
    config
        .budget
        .validate()
        .map_err(|error| PortError::Failed(format!("runtime_config_invalid:budget:{error}")))?;
    Ok(config)
}

fn runtime_config_from_lookup(
    mut lookup: impl FnMut(&str) -> Result<String, std::env::VarError>,
) -> Result<RuntimeConfig, PortError> {
    Ok(harness_runtime_config_from_lookup(&mut lookup)?.into_runtime_config(None))
}

fn parse_u32_override(
    lookup: &mut impl FnMut(&str) -> Result<String, std::env::VarError>,
    name: &str,
) -> Result<Option<u32>, PortError> {
    match lookup(name) {
        Ok(raw) => {
            let value = raw
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid_runtime_config(name))?;
            if value == 0 {
                return Err(invalid_runtime_config(name));
            }
            Ok(Some(value))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(invalid_runtime_config(name)),
    }
}

fn parse_u64_override(
    lookup: &mut impl FnMut(&str) -> Result<String, std::env::VarError>,
    name: &str,
) -> Result<Option<u64>, PortError> {
    match lookup(name) {
        Ok(raw) => {
            let value = raw
                .trim()
                .parse::<u64>()
                .map_err(|_| invalid_runtime_config(name))?;
            if value == 0 {
                return Err(invalid_runtime_config(name));
            }
            Ok(Some(value))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(invalid_runtime_config(name)),
    }
}

fn invalid_runtime_config(name: &str) -> PortError {
    PortError::Failed(format!(
        "runtime_config_invalid:{name}:expected_positive_integer"
    ))
}

fn request_may_execute(body: &RequestBody) -> bool {
    match body {
        RequestBody::Receipt(_)
        | RequestBody::ListApprovals(_)
        | RequestBody::AuditQuery(_)
        | RequestBody::Parity(_) => false,
        RequestBody::Command(command) => match command.name.as_str() {
            "company.snapshot.v1"
            | "company.next.v1"
            | "company.governance.v1"
            | "workflow.snapshot.v1"
            | "swarm.snapshot.v1"
            | "human.inbox"
            | "failure.incidents"
            | "feedback.list"
            | "version.drift"
            | "trace.replay"
            | "memory.proposals"
            | "workspace.checkpoint.list"
            | "workspace.checkpoint.preview" => false,
            "memory.distill" | "extension.manage" | "connector.manage" | "data.governance" => {
                !matches!(
                    command.arguments["action"].as_str(),
                    None | Some("list" | "search" | "show" | "status" | "inspect" | "preview")
                )
            }
            "extension.list" | "extension.inspect" => false,
            _ => true,
        },
        _ => true,
    }
}

fn effective_permission_profile(
    body: &RequestBody,
    declared: PermissionProfile,
) -> PermissionProfile {
    match body {
        RequestBody::Run(request) => permission_profile_for_sandbox(request.sandbox.as_deref()),
        RequestBody::Continue(request) => {
            permission_profile_for_sandbox(request.sandbox.as_deref())
        }
        RequestBody::Spawn(request) => permission_profile_for_sandbox(request.sandbox.as_deref()),
        RequestBody::Symposium(request) => {
            permission_profile_for_sandbox(request.sandbox.as_deref())
        }
        RequestBody::ApprovalDecision(_)
        | RequestBody::Resume(_)
        | RequestBody::ListApprovals(_) => declared,
        RequestBody::Steer(_) | RequestBody::Inject(_) => declared,
        RequestBody::Review(_) | RequestBody::Close(_) => PermissionProfile::Balanced,
        RequestBody::Command(_) => declared,
        RequestBody::Cancel(_) => declared,
        RequestBody::Receipt(_) => PermissionProfile::Safe,
        RequestBody::AuditQuery(_) => PermissionProfile::Safe,
        RequestBody::Parity(_) => PermissionProfile::Safe,
        RequestBody::AuditExport(_) => declared,
    }
}

fn permission_profile_for_sandbox(sandbox: Option<&str>) -> PermissionProfile {
    match sandbox.map(str::trim) {
        Some("workspace-write" | "workspace_write" | "workspace") => PermissionProfile::Balanced,
        _ => PermissionProfile::Safe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_runner::UnavailableModel;

    fn harness_with_config(config: RuntimeConfig) -> KianaHarness {
        KianaHarness::with_config(Arc::new(UnavailableModel::default()), config)
    }

    #[test]
    fn unset_harness_environment_uses_default_runtime_config() {
        let config = runtime_config_from_lookup(|_| Err(std::env::VarError::NotPresent)).unwrap();
        let runtime_config = harness_with_config(config).config();

        assert_eq!(runtime_config, RuntimeConfig::default());
        assert_eq!(runtime_config.max_steps_per_turn, 32);
        assert_eq!(runtime_config.wall_time_budget, None);
    }

    #[test]
    fn harness_max_steps_environment_override_reaches_runtime_config() {
        let config = runtime_config_from_lookup(|name| {
            if name == ENV_HARNESS_MAX_STEPS {
                Ok("5".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap();

        assert_eq!(
            harness_with_config(config).config(),
            RuntimeConfig {
                max_steps_per_turn: 5,
                ..RuntimeConfig::default()
            }
        );
    }

    #[test]
    fn harness_wall_time_environment_override_reaches_runtime_config() {
        let config = runtime_config_from_lookup(|name| {
            if name == ENV_HARNESS_WALL_TIME_MS {
                Ok("1000".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap();

        assert_eq!(
            harness_with_config(config).config(),
            RuntimeConfig {
                wall_time_budget: Some(Duration::from_secs(1)),
                ..RuntimeConfig::default()
            }
        );
    }

    #[test]
    fn invalid_role_harness_environment_value_fails_closed() {
        let error = harness_runtime_config_from_lookup(|name| {
            if name == ENV_HARNESS_MAX_STEPS {
                Ok("abc".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap_err();

        assert_eq!(
            error,
            PortError::Failed(format!(
                "runtime_config_invalid:{ENV_HARNESS_MAX_STEPS}:expected_positive_integer"
            ))
        );
    }

    #[test]
    fn non_numeric_harness_environment_value_fails_closed() {
        for name in [ENV_HARNESS_MAX_STEPS, ENV_HARNESS_WALL_TIME_MS] {
            let error = runtime_config_from_lookup(|candidate| {
                if candidate == name {
                    Ok("abc".to_owned())
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .unwrap_err();

            assert_eq!(
                error,
                PortError::Failed(format!(
                    "runtime_config_invalid:{name}:expected_positive_integer"
                ))
            );
        }
    }

    #[test]
    fn zero_harness_environment_value_fails_closed() {
        for name in [ENV_HARNESS_MAX_STEPS, ENV_HARNESS_WALL_TIME_MS] {
            let error = runtime_config_from_lookup(|candidate| {
                if candidate == name {
                    Ok("0".to_owned())
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .unwrap_err();

            assert_eq!(
                error,
                PortError::Failed(format!(
                    "runtime_config_invalid:{name}:expected_positive_integer"
                ))
            );
        }
    }
}
