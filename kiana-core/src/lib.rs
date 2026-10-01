//! The single command and capability control plane for Kiana.

mod approval_binding;
mod approvals;
mod artifacts;
mod audit;
mod audit_export;
mod audit_projection;
mod audit_projection_commit;
mod authority;
mod authority_read_model;
mod automation_release_evidence;
mod automation_snapshot;
mod automation_surface_uat;
mod billing_allocation;
mod billing_invoice;
mod billing_projection;
mod billing_recovery;
mod bq26_fault_harness;
mod capabilities;
mod capability_scheduler;
mod capacity_fault_envelope;
mod cell_registry;
mod change_contract;
mod ci12_product_gate;
mod clarification;
mod closing_receipt;
mod collaboration;
mod commands;
mod communication;
mod company;
mod company_evidence;
mod company_governance;
mod company_inbox;
mod company_integration;
mod company_knowledge;
mod company_parallel;
mod company_portfolio;
mod company_process;
mod company_read_model;
mod company_reconciliation;
mod company_recovery;
mod company_review;
mod company_template_registry;
mod connector_quota;
mod connector_reservation;
mod connectors;
mod context_query;
mod control_plane_authority;
mod control_plane_product_flow;
mod cost_correction;
mod credential_recovery;
mod data_class;
mod data_governance;
mod deletion;
mod delivery_authorization;
mod delivery_manifest;
mod deployment_admission;
mod deployment_capacity;
mod deployment_compatibility;
mod deployment_config;
mod deployment_incident;
mod deployment_lease;
mod deployment_observability;
mod deployment_operation;
mod deployment_reconcile;
mod deployment_release;
mod deployment_shutdown;
mod deployment_startup;
mod deployment_supervisor;
mod dispatch;
mod effect_reconciliation;
mod effect_usage_projection;
mod er31_fault_matrix;
mod er32_adapter_conformance;
mod er33_capacity_migration;
mod er34_durable_gate;
mod eval;
mod events;
mod evidence_manifest;
mod fallback_admission;
mod fault_injection;
mod milestone_acceptance;
mod ops_diagnostics;
mod outcome_measurement;
mod packet_acceptance;
mod project_acceptance;
mod project_control;
mod promotion_gate;
mod rework_contract;
mod storage_diagnostics;
mod storage_preflight;
pub use dispatch::{project_root_identity, JournalPermitVerifier};
mod billing_settlement_fold;
mod connector_conformance;
mod connector_mapping;
mod connector_notifications;
mod connector_pilot;
mod connector_propagation;
mod connector_recovery;
mod connector_surfaces;
mod connector_write_pilot;
mod golden_trace_chain;
mod health;
mod health_aggregation;
mod history;
mod hook_reauthorization;
mod incident_projection;
mod invocation_projection;
mod lifecycle;
mod memory_distillation;
mod model_budget;
mod restore_verification;
mod swarm_handoff;
mod swarm_merge_reducer;
mod swarm_merge_review;
mod swarm_projection;
mod swarm_release_gate;
mod swarm_retirement;
pub use model_budget::JournalModelBudget;
mod automation_boot_recovery;
mod automation_cancellation;
mod automation_compensation;
mod automation_dispatch;
mod automation_effect_reservation;
mod automation_fanout;
mod automation_retry;
mod automation_signal;
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
mod quality_drift;
mod quality_evidence_archive;
mod quality_feedback;
mod quality_gate;
mod quality_report;
mod receipts;
mod recovery;
mod recovery_rehearsal;
mod redaction;
mod replay_diagnostics;
mod resource_leases;
mod resource_projection;
mod restore_activation;
mod retention;
mod retention_archive_bounds;
mod retention_deletion;
mod revocation_propagation;
mod security_authority;
mod security_context;
mod security_fence;
mod security_incident;
mod sessions;
mod span_projection;
mod telemetry_separation;
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
pub use audit_projection_commit::{
    AuditProjectionClaim, AuditProjectionCommitReport, AuditProjectionCommitStatus,
    AuditProjectionHead, AuditProjectionRebuildProof, AuditProjectionRebuildReport,
    AuditProjectionRebuildStatus, AUDIT_PROJECTION_CLAIM_SCHEMA,
    AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA, AUDIT_PROJECTION_COMMIT_VERSION,
    AUDIT_PROJECTION_HEAD_SCHEMA, AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA,
    AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA,
};
pub use authority_read_model::{
    project_authority_read_model, AuthorityProjectionError, AuthorityReadModel,
    BudgetAuthorityProjection, CellAuthorityProjection, GrantAuthorityProjection,
    LeaseAuthorityProjection, AUTHORITY_READ_MODEL_SCHEMA,
};
pub use automation_boot_recovery::validate_boot_recovery;
pub use automation_cancellation::validate_automation_cancel;
pub use automation_compensation::validate_compensation_plan;
pub use automation_dispatch::observe_automation_dispatch;
pub use automation_effect_reservation::reserve_automation_effect;
pub use automation_fanout::validate_fanout_plan;
pub use automation_release_evidence::validate_automation_release_gate;
pub use automation_retry::classify_retry;
pub use automation_signal::validate_automation_signal_fact;
pub use automation_snapshot::validate_automation_snapshot;
pub use automation_surface_uat::validate_automation_surface_uat;
pub use billing_allocation::{CostAllocationAdmission, CostAllocationAdmissionError};
pub use billing_invoice::{
    reject_duplicate_provider_invoices, validate_invoice_comparison, validate_provider_invoice,
};
pub use billing_projection::{
    BillingProjectionFence, BillingProjectionFenceError, BILLING_PROJECTION_FENCE_NO_FACT_WRITES,
};
pub use billing_recovery::validate_billing_recovery;
pub use billing_settlement_fold::{
    project_settlement_fold, project_settlement_folds, SettlementFoldProjection,
    SettlementFoldProjectionError,
};
pub use bq26_fault_harness::{
    bq26_fixture_clock, bq26_fixture_deadline, bq26_fixture_digest, bq26_fixture_fold_seed,
    bq26_fixture_group, bq26_fixture_now, bq26_fixture_reservation, bq26_fixture_retry_policy,
    bq26_fixture_window, bq26_inject_library_case, Bq26FaultCase, Bq26FaultClass,
    Bq26FaultHarnessRun, Bq26FaultObservation, Bq26FaultRefusal, Bq26FaultSeam,
    BQ26_FAKE_DEADLINE_UNIX_MS, BQ26_FAKE_MONOTONIC_MS, BQ26_FAKE_NOW_UNIX_MS,
    BQ26_FAULT_CASES_SCHEMA, BQ26_FAULT_HARNESS_SCHEMA, BQ26_FAULT_HARNESS_VERSION,
    BQ26_FAULT_MAX_CASES,
};
pub use capabilities::derive_swarm_child_grant;
pub use capability_attempt_projection::{
    project_capability_attempts, project_effect_attempts, CapabilityAttemptProjectionError,
};
pub use capacity_fault_envelope::*;
pub use ci12_product_gate::validate_ci12_product_gate;
pub use clarification::{
    clarification_human_inbox_item, commit_clarification_answer, ClarificationCommit,
    CLARIFICATION_CORE_SCHEMA,
};
pub use company::validate_company_assignment;
pub use company_governance::{project_company_governance, CompanyGovernanceProjectionError};
pub use connector_conformance::validate_connector_conformance_report;
pub use connector_mapping::validate_connector_object_mapping;
pub use connector_notifications::validate_connector_notification;
pub use connector_pilot::validate_connector_pilot_gate;
pub use connector_propagation::validate_connector_propagation_fact;
pub use connector_quota::ControlPlaneConnectorQuota;
pub use connector_recovery::validate_connector_recovery_fact;
pub use connector_reservation::ControlPlaneConnectorReservation;
pub use connector_surfaces::{validate_connector_query, validate_connector_response};
pub use connector_write_pilot::validate_connector_write_pilot_gate;
pub use control_plane_authority::validate_control_plane_authority_scenario;
pub use control_plane_product_flow::validate_control_plane_product_bundle;
pub use cost_correction::{CostCorrectionAdmission, CostCorrectionAdmissionError};
pub use credential_recovery::{
    explicit_re_admit_credential_recovery, project_credential_recovery,
    CredentialRecoveryProjectionError, CredentialRecoveryReplayRequest,
    CREDENTIAL_RECOVERY_PROJECTION_VERSION,
};
pub use data_class::{
    admit_field, derive_sink_class, evaluate_sink_admission, metric_label_budget,
    metric_series_budget, runtime_metric_refusal, ClassBasis, ClassClaimOrigin, DerivedClass,
    SinkAdmission, SinkAdmissionRefusal, SinkAdmissionReport, SinkField, TelemetryGuarantees,
    TelemetryRuntimeRefusal, DATA_CLASS_ADMISSION_VERSION, DATA_CLASS_DERIVATION_SCHEMA,
    MAX_SINK_ADMISSION_FIELDS, MAX_SINK_FIELD_KEY, MAX_SINK_FIELD_VALUE, OBSERVATION_SINKS,
    SINK_ADMISSION_REPORT_SCHEMA,
};
pub use data_governance::{
    plan_data_propagation, project_data_governance, project_data_governance_snapshot,
    receipt_data_binding_from_events, seal_governance_events,
};
pub use deletion::{
    plan_deletion, plan_deletion_propagation, receipt_redaction_is_not_authorization,
};
pub use deployment_admission::{evaluate_admission, validate_admission_decision};
pub use deployment_capacity::{evaluate_capacity, validate_capacity_report};
pub use deployment_compatibility::validate_deployment_compatibility;
pub use deployment_config::validate_deployment_config_snapshot;
pub use deployment_incident::{evaluate_incident, validate_incident_report};
pub use deployment_lease::validate_operation_lease_cas;
pub use deployment_observability::validate_deployment_observability;
pub use deployment_operation::{replay_operation_journal, validate_operation_journal};
pub use deployment_reconcile::{evaluate_reconcile, validate_reconcile_report};
pub use deployment_release::validate_deployment_release;
pub use deployment_shutdown::{evaluate_shutdown, validate_shutdown_report};
pub use deployment_startup::{evaluate_startup, validate_startup_report};
pub use deployment_supervisor::validate_supervisor_observation;
pub use effect_reconciliation::*;
pub use effect_usage_projection::{
    project_effect_usage, EffectUsageProjectionError, EFFECT_USAGE_PROJECTION_SCHEMA,
};
pub use entrypoint_parity::{
    EntrypointCommand, EntrypointDecision, EntrypointParityMatrix, ENTRYPOINT_COMMAND_SCHEMA,
    ENTRYPOINT_PARITY_MATRIX_SCHEMA, ENTRYPOINT_PARITY_VERSION, ENTRYPOINT_ROUTE,
};
pub use er31_fault_matrix::validate_er31_fault_matrix;
pub use er32_adapter_conformance::validate_er32_conformance_report;
pub use er33_capacity_migration::validate_er33_capacity_migration_drill;
pub use er34_durable_gate::validate_er34_durable_gate_evidence;
pub use eval::{evaluate_provider_independent, evaluate_suite, EvalError};
pub use evidence_manifest::*;
pub use fallback_admission::ControlPlaneFallbackAdmission;
pub use fault_injection::{
    fault_matrix, fault_matrix_from_events, replay_fault_matrix, FaultInjectionError,
};
pub use golden_trace_chain::*;
pub use health::{project_health_snapshot, HealthProjectionError};
pub use health_aggregation::{aggregate_health, validate_health_aggregation};
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
pub use ops_diagnostics::{evaluate_ops_diagnostics, evaluate_ops_mode, validate_ops_diagnostics};
pub use parity::{project_entrypoint_parity, ParityProjectionError};
pub use performance::{
    build_performance_baseline, percentile_micros, summarize_benchmark, PerformanceError,
};
pub use persistence_read_model::{
    project_persistence_read_model, PersistenceReadModel, ProjectedRunState,
    PERSISTENCE_READ_MODEL_SCHEMA,
};
pub use platform::classify_failure_summary;
pub use projection::{project_run_state, RunOutcome, RunPhase, RunProjectionError, RunState};
pub use projection_checkpoint::{ProjectionDriver, ProjectionDriverStatus, ReplayProjection};
pub use promotion_gate::*;
pub use provider_diagnostics::{
    project_provider_diagnostics, replay_provider_terminal, ProviderDiagnosticsProjectionError,
};
pub use quality_drift::{evaluate_quality_drift, DRIFT_ALERT_COMMAND};
pub use quality_evidence_archive::validate_quality_evidence_archive;
pub use quality_feedback::{derive_quality_feedback, QUALITY_FEEDBACK_COMMAND};
pub use quality_report::{validate_quality_report, QUALITY_REPORT_COMMAND};
pub use receipts::aggregate_receipt_facts;
pub use recovery_rehearsal::{
    evaluate_post_recovery_lease, evaluate_recovery_rehearsal, RecoveryLeaseObservation,
    RecoveryRehearsalOutcome, RECOVERY_REHEARSAL_CORE_VERSION, RECOVERY_REHEARSAL_OUTCOME_SCHEMA,
};
pub use replay_diagnostics::{diagnose_replay, ReplayDiagnosticsError, ReplayExpectation};
pub use resource_projection::project_recovery_resources;
pub use restore_activation::{
    admit_audit_read_after_activation, admit_command_after_activation, ActivationLedger,
    ActivationStage, ActivationState, RestoreActivationRecord, RestoreActivationReport,
    RestoreActivationRequest, RestoreActivationStatus, RootWriteMode, SupersededRootWriter,
    RESTORE_ACTIVATION_LEDGER_SCHEMA, RESTORE_ACTIVATION_RECORD_SCHEMA,
    RESTORE_ACTIVATION_REPORT_SCHEMA, RESTORE_ACTIVATION_REQUEST_SCHEMA,
    RESTORE_ACTIVATION_STATE_SCHEMA, RESTORE_ACTIVATION_VERSION,
};
pub use restore_verification::validate_restore_verification_fact;
pub use retention::scan_retention;
pub use retention_archive_bounds::*;
pub use retention_deletion::*;
pub use revocation_propagation::{
    admit_derived_read_after_recovery, admit_derived_write, merge_layer_observation,
    propagation_scope, RevocationKind, RevocationLayer, RevocationLayerObservation,
    RevocationLayerState, RevocationPropagationReport, RevocationPropagationRequest,
    RevocationPropagationStatus, REVOCATION_OBSERVATION_SCHEMA, REVOCATION_PROPAGATION_VERSION,
    REVOCATION_REPORT_SCHEMA, REVOCATION_REQUEST_SCHEMA,
};
pub use security_authority::{
    SecurityAuthoritySnapshot, SECURITY_AUTHORITY_SNAPSHOT_SCHEMA,
    SECURITY_AUTHORITY_SNAPSHOT_VERSION,
};
pub use security_context::{SecurityContext, SECURITY_CONTEXT_SCHEMA, SECURITY_CONTEXT_VERSION};
pub use security_incident::*;
pub use span_projection::{project_span_lifecycle, project_spans, SpanProjectionError};
pub use storage_diagnostics::{
    evaluate_storage_diagnostics, storage_diagnostic_ui_view, validate_storage_diagnostic_report,
    validate_storage_diagnostic_ui_view, StorageCheckpointState, StorageDiagnosticInput,
    StorageDiagnosticNote, StorageDiagnosticReport, StorageDiagnosticSignal,
    StorageDiagnosticUiView, StorageMaintenanceCounters, StorageMaintenanceObservation,
    StorageMaintenanceSubject, StorageMetricUnit, StorageProjectionLag, StorageSignalChannel,
    STORAGE_DIAGNOSTIC_DISPLAY_ONLY, STORAGE_DIAGNOSTIC_FACT_RESERVED_PREFIXES,
    STORAGE_DIAGNOSTIC_INPUT_SCHEMA, STORAGE_DIAGNOSTIC_LAG_SCHEMA,
    STORAGE_DIAGNOSTIC_MAINTENANCE_SCHEMA, STORAGE_DIAGNOSTIC_METRIC_CATALOG,
    STORAGE_DIAGNOSTIC_NOTE_SCHEMA, STORAGE_DIAGNOSTIC_REPORT_SCHEMA,
    STORAGE_DIAGNOSTIC_SIGNAL_SCHEMA, STORAGE_DIAGNOSTIC_VERSION, STORAGE_DIAGNOSTIC_VIEW_SCHEMA,
};
pub use storage_preflight::{evaluate_storage_preflight, validate_storage_preflight_report};
pub use swarm_admission::admit_swarm_resources;
pub use swarm_cancellation::validate_child_cancellation;
pub use swarm_child::materialize_child;
pub use swarm_child_result::validate_child_result;
pub use swarm_handoff::validate_swarm_handoff_record;
pub use swarm_merge_reducer::validate_swarm_merge;
pub use swarm_merge_review::{validate_swarm_receipt, validate_swarm_review};
pub use swarm_progress::record_swarm_progress;
pub use swarm_projection::validate_swarm_projection_event_fact;
pub use swarm_queue::{claim_swarm_entry, complete_swarm_entry, enqueue_swarm_entry};
pub use swarm_recovery::validate_child_recovery;
pub use swarm_release_gate::validate_swarm_release_evidence;
pub use swarm_retirement::validate_swarm_retirement_fact;
pub use swarm_routing::validate_child_route;
pub use telemetry_separation::{
    admission_cell, is_actionable_refusal, observation_label_budget, separate, ObservationOutcome,
    SeparationReport, SeparationRequest, SinkObservation, SinkReachability, SubstitutionRefusal,
    MAX_SEPARATION_FIELDS, MAX_SEPARATION_SINKS, TELEMETRY_SEPARATION_CORE_VERSION,
    TELEMETRY_SEPARATION_REPORT_SCHEMA,
};
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
    ArtifactStorePort, CapabilityOutcome, EventStorePort, PortError, PreToolHookDecision,
    PreToolHookPort, RunnerPort, SpawnReservationRequest,
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
    /// Optional immutable artifact persistence. The default remains fail-closed for typed
    /// artifact publication; composition roots opt in with `with_artifact_store`.
    artifact_store: Option<Arc<dyn ArtifactStorePort>>,
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
mod swarm_admission;
mod swarm_cancellation;
mod swarm_child;
mod swarm_child_result;
mod swarm_progress;
mod swarm_queue;
mod swarm_recovery;
mod swarm_routing;

mod company_business;
mod company_scope;
