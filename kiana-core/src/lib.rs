//! The single command and capability control plane for Kiana.

mod approval_binding;
mod approvals;
mod artifacts;
mod audit;
mod audit_export;
mod audit_projection;
mod authority;
mod authority_read_model;
mod billing_allocation;
mod billing_projection;
mod capabilities;
mod capability_scheduler;
mod cell_registry;
mod clarification;
mod collaboration;
mod commands;
mod communication;
mod company;
mod company_governance;
mod company_evidence;
mod company_review;
mod company_process;
mod connector_quota;
mod connector_reservation;
mod connectors;
mod context_query;
mod cost_correction;
mod credential_recovery;
mod data_governance;
mod deletion;
mod dispatch;
mod effect_usage_projection;
mod eval;
mod events;
mod fallback_admission;
mod fault_injection;
pub use dispatch::{project_root_identity, JournalPermitVerifier};
mod billing_settlement_fold;
mod health;
mod history;
mod hook_reauthorization;
mod incident_projection;
mod invocation_projection;
mod lifecycle;
mod memory_distillation;
mod model_budget;
pub use model_budget::JournalModelBudget;
mod capability_attempt_projection;
mod entrypoint_parity;
mod memory_proposals;
mod metrics;
mod model_attempt_lifecycle;
mod model_attempt_projection;
mod notification_action;
mod notification_delivery;
mod notification_external;
mod notification_faults;
mod notification_materializer;
mod notification_parity;
mod notification_policy;
mod notification_priority;
mod notification_projector;
mod notification_recovery;
mod notification_resolver;
mod notification_store;
mod operator_evidence;
mod parity;
mod performance;
mod persistence_read_model;
mod platform;
mod projection;
mod projection_checkpoint;
mod provider_diagnostics;
mod quality_gate;
mod receipts;
mod recovery;
mod redaction;
mod replay_diagnostics;
mod resource_leases;
mod resource_projection;
mod retention;
mod security_authority;
mod security_context;
mod security_fence;
mod sessions;
mod span_projection;
mod trace_export;
mod turn_outcome;
mod ui_actions;
mod versioning;
mod workflow_queue;
mod workspace_checkpoints;

pub use approval_binding::{
    ApprovalBinding, HumanInboxItem, HumanInboxStatus, APPROVAL_BINDING_SCHEMA,
    APPROVAL_BINDING_VERSION, HUMAN_INBOX_ITEM_SCHEMA,
};
pub use audit::{append_committed_audit_records, reduce_committed_audit_records};
pub use audit_export::{AuditExportError, AuditExportInput};
pub use audit_projection::{
    rebuild_audit_projection, AuditProjection, AuditProjectionError, AuditQueryInput,
    AUDIT_PROJECTION_VERSION,
};
pub use authority_read_model::{
    project_authority_read_model, AuthorityProjectionError, AuthorityReadModel,
    BudgetAuthorityProjection, CellAuthorityProjection, GrantAuthorityProjection,
    LeaseAuthorityProjection, AUTHORITY_READ_MODEL_SCHEMA,
};
pub use billing_allocation::{CostAllocationAdmission, CostAllocationAdmissionError};
pub use billing_projection::{
    BillingProjectionFence, BillingProjectionFenceError, BILLING_PROJECTION_FENCE_NO_FACT_WRITES,
};
pub use billing_settlement_fold::{
    project_settlement_fold, project_settlement_folds, SettlementFoldProjection,
    SettlementFoldProjectionError,
};
pub use capabilities::derive_swarm_child_grant;
pub use capability_attempt_projection::{
    project_capability_attempts, project_effect_attempts, CapabilityAttemptProjectionError,
};
pub use clarification::{
    clarification_human_inbox_item, commit_clarification_answer, ClarificationCommit,
    CLARIFICATION_CORE_SCHEMA,
};
pub use company::validate_company_assignment;
pub use company_governance::{project_company_governance, CompanyGovernanceProjectionError};
pub use connector_quota::ControlPlaneConnectorQuota;
pub use connector_reservation::ControlPlaneConnectorReservation;
pub use cost_correction::{CostCorrectionAdmission, CostCorrectionAdmissionError};
pub use credential_recovery::{
    explicit_re_admit_credential_recovery, project_credential_recovery,
    CredentialRecoveryProjectionError, CredentialRecoveryReplayRequest,
    CREDENTIAL_RECOVERY_PROJECTION_VERSION,
};
pub use data_governance::{
    plan_data_propagation, project_data_governance, project_data_governance_snapshot,
    receipt_data_binding_from_events, seal_governance_events,
};
pub use deletion::{
    plan_deletion, plan_deletion_propagation, receipt_redaction_is_not_authorization,
};
pub use effect_usage_projection::{
    project_effect_usage, EffectUsageProjectionError, EFFECT_USAGE_PROJECTION_SCHEMA,
};
pub use entrypoint_parity::{
    EntrypointCommand, EntrypointDecision, EntrypointParityMatrix, ENTRYPOINT_COMMAND_SCHEMA,
    ENTRYPOINT_PARITY_MATRIX_SCHEMA, ENTRYPOINT_PARITY_VERSION, ENTRYPOINT_ROUTE,
};
pub use eval::{evaluate_provider_independent, evaluate_suite, EvalError};
pub use fallback_admission::ControlPlaneFallbackAdmission;
pub use fault_injection::{
    fault_matrix, fault_matrix_from_events, replay_fault_matrix, FaultInjectionError,
};
pub use health::{project_health_snapshot, HealthProjectionError};
pub use hook_reauthorization::*;
pub use incident_projection::{
    project_incidents, project_observability_incidents, IncidentProjectionError,
};
pub use invocation_projection::{project_invocations, InvocationProjection};
pub use metrics::{
    project_metrics, project_operational_metrics, project_run_metrics, MetricCardinalityError,
    MetricCardinalityGuard, MetricReducer, MetricReducerError, MetricsProjectionError,
};
pub use model_attempt_lifecycle::{
    project_model_attempt_lifecycle, validate_model_attempt_dispatch,
    ModelAttemptLifecycleProjection, ModelAttemptLifecycleProjectionError,
};
pub use model_attempt_projection::{
    project_model_attempts, project_provider_attempts, ModelAttemptProjectionError,
};
pub use notification_action::{
    NotificationActionAdmission, NotificationActionGate, NotificationActionGateError,
    NOTIFICATION_ACTION_GATE_SCHEMA,
};
pub use notification_delivery::{
    NotificationDeliveryPlan, NotificationDeliveryWorker, NOTIFICATION_DELIVERY_WORKER_SCHEMA,
};
pub use notification_external::{
    admit_external_notification, observe_external_notification_receipt,
    ExternalNotificationAdmission, ExternalNotificationDisposition,
    ExternalNotificationReceiptObservation,
};
pub use notification_faults::{
    notification_fault_matrix, NotificationFaultCase, NotificationFaultDisposition,
    NotificationFaultMatrix, NotificationFaultScenario, NOTIFICATION_FAULT_CASE_SCHEMA,
    NOTIFICATION_FAULT_MATRIX_SCHEMA,
};
pub use notification_materializer::NotificationMaterializer;
pub use notification_parity::{
    compare_notification_entrypoints, NotificationEntrypoint, NotificationEntrypointParity,
    NotificationEntrypointSnapshot, NotificationParityDisposition,
    NOTIFICATION_ENTRYPOINT_PARITY_SCHEMA, NOTIFICATION_ENTRYPOINT_SNAPSHOT_SCHEMA,
};
pub use notification_policy::{
    plan_notification_policy, NotificationPolicyConfig, NotificationPolicyError,
    NOTIFICATION_POLICY_PLANNER_SCHEMA,
};
pub use notification_priority::{
    classify_notification, compare_notification_priority, NotificationPriority,
    NotificationPriorityError, NotificationUrgency, NOTIFICATION_PRIORITY_SCHEMA,
};
pub use notification_projector::NotificationProjection;
pub use notification_projector::{
    NotificationProjector, NotificationProjectorError, NotificationVisibility,
    NOTIFICATION_PROJECTOR_SCHEMA,
};
pub use notification_recovery::{
    plan_notification_recovery, NotificationRecoveryDisposition, NotificationRecoveryInput,
    NotificationRecoveryPlan, NotificationRecoveryState, NOTIFICATION_RECOVERY_SCHEMA,
};
pub use notification_resolver::resolve_notification_subscriptions;
pub use notification_store::{
    NotificationListRequest, NotificationPage, NotificationPageItem,
    NotificationProjectionMutation, NotificationStore, NotificationStoreError,
    NOTIFICATION_MUTATION_SCHEMA, NOTIFICATION_PAGE_SCHEMA, NOTIFICATION_STORE_MAX_PAGE_SIZE,
    NOTIFICATION_STORE_SCHEMA,
};
pub use operator_evidence::{project_operator_evidence, OperatorEvidenceError};
pub use parity::{project_entrypoint_parity, ParityProjectionError};
pub use performance::{
    build_performance_baseline, percentile_micros, summarize_benchmark, PerformanceError,
};
pub use persistence_read_model::{
    project_persistence_read_model, PersistenceReadModel, ProjectedRunState,
    PERSISTENCE_READ_MODEL_SCHEMA,
};
pub use projection::{project_run_state, RunOutcome, RunPhase, RunProjectionError, RunState};
pub use projection_checkpoint::{ProjectionDriver, ProjectionDriverStatus, ReplayProjection};
pub use provider_diagnostics::{
    project_provider_diagnostics, replay_provider_terminal, ProviderDiagnosticsProjectionError,
};
pub use receipts::aggregate_receipt_facts;
pub use replay_diagnostics::{diagnose_replay, ReplayDiagnosticsError, ReplayExpectation};
pub use resource_projection::project_recovery_resources;
pub use retention::scan_retention;
pub use security_authority::{
    SecurityAuthoritySnapshot, SECURITY_AUTHORITY_SNAPSHOT_SCHEMA,
    SECURITY_AUTHORITY_SNAPSHOT_VERSION,
};
pub use security_context::{SecurityContext, SECURITY_CONTEXT_SCHEMA, SECURITY_CONTEXT_VERSION};
pub use span_projection::{project_span_lifecycle, project_spans, SpanProjectionError};
pub use trace_export::{
    exportable_status, foreign_parent_link, LocalTraceExporter, NoopTraceExporter,
    TraceExportConfig, TraceExportDisposition, TraceExportError, TraceExportReceipt,
};
pub use turn_outcome::{annotate_output, propose_turn_outcome, TURN_OUTCOME_CORE_SCHEMA};
pub use ui_actions::UiActionAuthoritySnapshot;
pub use workflow_queue::{
    project_workflow_queue_ready, validate_workflow_queue_dispatch,
    validate_workflow_queue_transaction, workflow_queue_requires_recovery, WorkflowQueueReadyView,
};

use capability_scheduler::CapabilityAdmissionScheduler;
use cell_registry::MemoryCellRegistry;
use kiana_domain::{
    path_locks_conflict, ApprovalDecision, ApprovalId, AuthorizedCapabilityRequest, BudgetLease,
    CapabilityGrant, CapabilityGrantId, CapabilityKind, CapabilityRequest, CapabilityResult,
    CellId, CellLifecycle, CellSpec, ClosingReceipt, CommandIntent, CoreResponse, DecisionRecord,
    ExecutionStatus, GateDecision, MemoryDistillationSource, MemoryDistillationSourceStatus,
    MemoryDistillationSourceType, MemoryOrigin, MemoryProposal, MergeReceipt, PendingInvocation,
    PermissionProfile, PolicyDecision, RequestContext, RequestId, ReviewPacket, RiskLevel,
    RoleSpec, RunId, RuntimeEvent, SpawnPlan, SpawnPlanId, SpawnPlanStatus, SupervisionLease,
    SupervisionLeaseId, Symposium, SymposiumClaim, WorkFingerprint, WorkPacket,
    CAPABILITY_GRANT_SCHEMA, CELL_SCHEMA, DEPARTMENT_EXECUTING, DEPARTMENT_PLANNING,
    MEMORY_SEARCH_SCHEMA, REVIEW_PACKET_PATH, REVIEW_RESULT_SCHEMA, ROLE_BUILDER, ROLE_CLOSER,
    ROLE_REVIEWER, SPAWN_PLAN_SCHEMA, SUPERVISION_LEASE_SCHEMA, SYMPOSIUM_RESULT_SCHEMA,
    WORK_PACKET_PATH,
};
use kiana_gates::GateEngine;
use kiana_policy::{capability_risk_violation, PolicyEngine};
use kiana_ports::{
    AllowAllPreToolHooks, ApprovalStorePort, CapabilityBrokerPort, CapabilityLease,
    CapabilityOutcome, EventStorePort, PortError, PreToolHookDecision, PreToolHookPort, RunnerPort,
    SpawnReservationRequest,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "linux")]
use std::ffi::{CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{watch, Mutex as AsyncMutex};

pub struct PathLockLease {
    _file: File,
}

/// The same OS locks are held by short Run scopes and explicitly supervised processes.
pub fn acquire_workspace_resources(
    project_root: &str,
    paths: &[String],
) -> Result<Vec<PathLockLease>, PortError> {
    sessions::acquire_durable_path_locks(project_root, paths)
        .map_err(|reason| PortError::Conflict(reason.to_owned()))
}

pub struct ControlPlaneRuntimeConfig {
    pub max_steps_per_turn: u32,
}

struct RunTerminalScope {
    recorded: AsyncMutex<bool>,
}

struct TerminalScopeGuard<'a> {
    control_plane: &'a ControlPlane,
    run_id: RunId,
}

impl Drop for TerminalScopeGuard<'_> {
    fn drop(&mut self) {
        self.control_plane.end_terminal_scope(self.run_id);
    }
}

struct BuilderPathLockGuard<'a> {
    control_plane: &'a ControlPlane,
    project_root: String,
    session_id: String,
}

impl Drop for BuilderPathLockGuard<'_> {
    fn drop(&mut self) {
        self.control_plane
            .release_builder_path_locks(&self.project_root, &self.session_id);
    }
}

pub const LEGACY_EDGES_REMAINING: usize = 9;
pub const HARNESS_ID: &str = "kiana-harness";
pub const RUN_RESULT_SCHEMA: &str = "kiana.run-result.v1";
pub const COMPACT_SCHEMA: &str = "kiana.compact.v1";
const CONTEXT_QUERY_COMMAND: &str = "context.query.v1";
const CONTEXT_REPO_MAP_OPERATION: &str = "context.repo_map";
const CONTEXT_INDEX_OPERATION: &str = "context.index.read";
const CONTEXT_INDEX_CACHE_OPERATION: &str = "context.index.cache.write";
const CONTEXT_ARTIFACTS_OPERATION: &str = "context.artifacts.read";
const CONTEXT_ARTIFACTS_CACHE_OPERATION: &str = "context.artifacts.cache.write";
const CONTEXT_ARTIFACT_STORE_OPERATION: &str = "context.artifact_store.read";
const CONTEXT_ARTIFACT_STORE_CACHE_OPERATION: &str = "context.artifact_store.cache.write";
const CONTEXT_ARTIFACT_INGEST_OPERATION: &str = "context.artifact_ingest.write";
const CONTEXT_ARTIFACT_GRAPH_OPERATION: &str = "context.artifact_graph.read";
const CONTEXT_ARTIFACT_READINESS_OPERATION: &str = "context.artifact_readiness.read";
const CONTEXT_SEARCH_OPERATION: &str = "context.search";
const CONTEXT_VECTOR_SEARCH_OPERATION: &str = "context.vector_search";
const CONTEXT_PACK_OPERATION: &str = "context.pack";
const MAX_CONTEXT_LIMIT: u64 = 1_000;
const MAX_CONTEXT_BYTES_PER_FILE: u64 = 16 * 1024 * 1024;
const MAX_CONTEXT_SNIPPET_LINES: u64 = 1_000;

#[derive(Clone, Debug)]
struct SessionBinding {
    run_id: RunId,
    actor_id: Option<String>,
    project_root: String,
    role_id: String,
    department_id: String,
}

#[derive(Clone, Copy)]
struct PersistedApprovalCursor {
    run_id: RunId,
    event_request_id: RequestId,
    event_sequence: u64,
    continuation_recorded: bool,
}

pub struct ControlPlane {
    policy: Arc<dyn PolicyEngine>,
    gates: Arc<dyn GateEngine>,
    events: Arc<dyn EventStorePort>,
    capabilities: Arc<dyn CapabilityBrokerPort>,
    approvals: Arc<dyn ApprovalStorePort>,
    runner: Arc<dyn RunnerPort>,
    workspace_checkpoints: Option<Arc<dyn kiana_ports::WorkspaceCheckpointPort>>,
    max_steps_per_turn: Option<u32>,
    pre_tool_hooks: Arc<dyn PreToolHookPort>,
    cell_registry: Arc<dyn kiana_ports::CellRegistryPort>,
    sessions: Mutex<HashMap<String, SessionBinding>>,
    /// Event-derived invocation projections. The ledger remains authoritative; this map is
    /// only a write-through cache for repeated reads within one host process.
    invocation_projections: Mutex<HashMap<RunId, Vec<InvocationProjection>>>,
    /// Event IDs included in each cached fold. This lets a cache miss detector notice facts
    /// appended by the broker's permit verifier, which cannot call back into ControlPlane.
    invocation_projection_event_ids: Mutex<HashMap<RunId, HashSet<String>>>,
    pending_invocations: Mutex<HashMap<ApprovalId, PendingInvocation>>,
    cancellations: Mutex<HashMap<RunId, watch::Sender<bool>>>,
    capability_stops: Mutex<HashMap<RunId, watch::Sender<Option<bool>>>>,
    active_terminal_scopes: Mutex<HashMap<RunId, Arc<RunTerminalScope>>>,
    path_locks: Mutex<HashMap<String, String>>,
    durable_path_locks: Mutex<HashMap<String, Vec<PathLockLease>>>,
    /// Process-local admission only. It never executes a capability; the Broker remains the
    /// single effect boundary and CellRegistry remains the budget/path authority.
    pub(crate) admission_scheduler: Arc<CapabilityAdmissionScheduler>,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Domain(#[from] kiana_domain::DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
}

mod automation;

mod swarm;

mod company_business;
mod company_scope;
