//! Kiana client 与 daemon 之间的 versioned wire contract。
//!
//! `kiana.protocol.v1` 只定义可序列化的请求/响应外形。它是跨进程边界，不包含执行器实现
//! 或权限绕过能力；daemon 收到 envelope 后仍必须重建上下文、检查项目 trust、策略、
//! gate、审批和生命周期。新增字段优先使用 `serde(default)` 保持旧客户端可读取，但这
//! 只是兼容性策略，不代表缺失字段自动安全或自动允许。

pub use kiana_domain::json_digest;
pub use kiana_domain::{
    apply_settlement_fold_event, ConsumedUnits, ReservationUnits, SettlementFoldApplyOutcome,
    SettlementFoldEvent, SettlementFoldEventKind, SettlementFoldLedger, SettlementFoldRecord,
    SettlementFoldState, SettlementSourceRef, SettlementUsageClass, SETTLEMENT_FOLD_EVENT_SCHEMA,
    SETTLEMENT_FOLD_SCHEMA, SETTLEMENT_FOLD_VERSION,
};
use kiana_domain::{canonical_scopes, CoreResponse};
pub use kiana_domain::{
    normalize_role_path, project_redacted_error, redact_text, scan_secret_channels,
    scan_secret_sentinels, scan_secret_value, ActionRef, ActionRefId, AdapterCommitState,
    AdapterResult, AdapterResultKind, AgentTemplate, AggregationVerification, ApprovalChallenge,
    ApprovalConsumptionFact, ApprovalDecision, ApprovalDecisionFact, ApprovalExecutionMaterial,
    ApprovalId, ApprovalMaterialState, ApprovalPlanPreview, ArtifactId, ArtifactProvenance,
    ArtifactRef, ArtifactVersion, AssignmentDirectory, AssignmentId, AttemptId, AttemptStatus,
    AuditActionKind, AuditDecision, AuditExportFormat, AuditId, AuditQueryCursor, AuditRecord,
    AuthenticatedPrincipalRef, AuthenticationAssurance, AuthorityFence, AuthorityLedger,
    AuthoritySnapshot, BudgetLease, BudgetLeaseId, BudgetReservationFact, BudgetScope,
    BudgetSettlementFact, CapabilityErrorCode, CapabilityErrorPolicy, CapabilityExecutionState,
    CapabilityGrant, CapabilityGrantId, CapabilityProcessState, CapabilityResultDimensions,
    CapabilityResultReceipt, CatalogCandidate, CatalogEntry, CatalogEntryKind, CellId,
    CellLifecycle, CellSpec, ChildCellId, ChildHarnessBounds, ChildHarnessBudget,
    ChildHarnessCancellation, ChildHarnessIntent, ChildHarnessOutcome, ChildHarnessOutcomeKind,
    ChunkKind, ChunkRange, ChunkSet, ClarificationAnswer, ClarificationCancelPolicy,
    ClarificationOption, ClarificationRequest, ClarificationResolution, ClarificationSource,
    ClarificationStatus, ClarificationWaitView, CleanupCause, CleanupResource, CleanupResourceKind,
    CleanupResourceStatus, ClosingReceipt, CodeGraphEdge, CodeGraphNodeKind, CodeGraphRebuildPlan,
    CodeGraphRelation, CodeGraphTemporalState, CommunicationLifecycleEvent,
    CommunicationLifecycleStatus, CommunicationMessage, CommunicationMessageKind,
    CompactEvidenceKind, CompactEvidenceStatus, CompactSummary, CompactSummaryEvidence,
    CompanyCommandReceipt, CompanyReceiptStatus, CompanyScope, CompanyScopeRegistry, ComponentId,
    ConfigSnapshot, ContextCacheBinding, ContextCandidate, ContextCheckpoint,
    ContextCheckpointState, ContextInvalidationPlan, ContextInvalidationTarget,
    ContextMaterialClass, ContextMaterialType, ContextMemoryInspectorSnapshot, ContextPlan,
    ContextPlanItem, ContextResumeView, ContextSourceBudget, CredentialDisplayStatus, Criterion,
    CriterionId, DataBoundary, DataBoundaryId, DecisionActorKind, DecisionOption, DecisionPurpose,
    DelegationId, DelegationPacket, DeliveryAttempt, DeliveryAttemptId, DeliveryAttemptStatus,
    DeliveryReceipt, DeliveryReceiptId, DeliveryReceiptStatus, DepartmentCatalog,
    DepartmentSnapshot, DispatchIntent, DispatchIntentId, DispatchIntentStatus, DispatchPermit,
    EffectObservation, EffectObservationState, EmbeddingDevice, EmbeddingIndexBinding,
    EmbeddingManifest, EmbeddingMetadata, EmbeddingPooling, EmbeddingProvider,
    EmbeddingRotationPlan, EntryPointKind, EntryPointParitySnapshot, EvalCase, EvalCaseId,
    EvalDataset, EvalDatasetId, EvalSplit, EvalSuite, EvalSuiteId, EvalSuiteStatus, EventKindSpec,
    EventStoreHealth, EvidenceId, EvidenceRef, EvidenceRefId, EvidenceStatus, ExecutionId,
    ExecutionOutputBudget, ExecutionOutputRef, ExecutionReceipt, ExecutionScope, ExecutionStatus,
    ExtensionCatalog, ExtensionCommand, ExtensionCommandError, ExtensionCommandErrorCode,
    ExtensionCommandReceipt, ExtensionCommandRequest, ExtensionCommandResponse, ExtensionEffect,
    ExtensionError, ExtensionErrorCode, ExtensionExecutionScope, ExtensionId, ExtensionManifest,
    ExtensionNetworkPolicy, ExtensionPackage, ExtensionRequires, ExtensionSignature,
    ExtensionSnapshot, ExtensionSnapshotState, ExtensionType, ExtensionVisibilityAction,
    ExtensionVisibilityActionKind, ExtensionVisibilityEntry, ExtensionVisibilityKind,
    ExtensionVisibilityRisk, ExtensionVisibilitySnapshot, ExtensionVisibilitySource,
    ExtensionVisibilityStatus, ExtensionVisibilityTrust, ExternalResourceKind,
    ExternalResourceRequest, ExternalResourceSnapshot, FenceTokenId, Freshness, GoldenContextCase,
    GoldenContextFixture, GoldenTrace, GoldenTraceId, GrantId, HarnessContextStrategy,
    HarnessEvalComparison, HarnessEvalMetric, HarnessEvalMetricObservation, HarnessEvalMetricSet,
    HarnessEvalMetricStatus, HarnessEvalVariant, HarnessExecutionMode, HarnessReplayDifference,
    HarnessReplayDifferenceKind, HarnessReplayMode, HarnessReplayReport, HarnessTraceBinding,
    HistoricalReceiptInvalidation, HookBudget, HookDecision, HookDecisionKind, HookDescriptor,
    HookExecutionRole, HookFailurePolicy, HookLifecycleBinding, HookLifecycleInput,
    HookLifecyclePlan, HookLifecyclePoint, HookLifecycleResult, HookOrderCandidate, HookPhase,
    HookRunId, HumanDecision, HumanTask, HumanTaskStatus, IdentityMigration, IndexCacheKey,
    IndexComponentKind, IndexGenerationState, IndexGenerationStatus, IndexInvalidationPlan,
    IndexManifest, InteractionId, InvocationId, InvocationIdentity, LegacyCheckpointDecision,
    LegacyCompatibilityDecision, LegacyDisposition, LegacyWireKind, LlmTextMetadata, Membership,
    MembershipId, MembershipStatus, MemoryAccessPath, MemoryAclDecision, MemoryAclRequest,
    MemoryDistillationSource, MemoryDistillationSourceStatus, MemoryDistillationSourceType,
    MemoryEvidenceQuote, MemoryExtractionRequest, MemoryInvalidationKind, MemoryInvalidationPlan,
    MemoryModelWriteGate, MemoryPropagationRecord, MemoryPropagationState, MemoryPropagationTarget,
    MemoryScope, MemoryTemporalOmission, MemoryTemporalRecord, MemoryTemporalSelection,
    MemoryTemporalStatus, MemoryVisibility, MergeDecisionId, MergeReceipt, Message, MessageId,
    MessageKind, ModelAttemptId, ModelAttemptIdentity, ModelContent, ModelOutcome,
    ModelSideEffectState, ModelStopReason, NormalizedText, Notification, NotificationChannel,
    NotificationEventClass, NotificationEventSource, NotificationEventSpec, NotificationId,
    NotificationStatus, OperationId, OrganizationBinding, OrganizationId, OutputContract,
    OutputValueType, Partition, PartitionId, PartitionProjection, PartitionStatus,
    PermissionProfile, PluginLifecycle, PluginLifecycleState, PolicyProfile, PolicyProfileId,
    PolicyProfileStatus, PreparedModelRequest, Principal, PrincipalId, PrincipalKind,
    PrincipalStatus, ProgressAction, ProgressDecision, ProgressEvidence, ProgressInput,
    ProgressTracker, ProjectAssignment, ProjectAssignmentId, ProjectBinding, ProjectId,
    ProjectIdentity, ProjectTrustSnapshot, ProjectionCheckpoint, ProjectionLagStatus,
    ProjectionLagView, ProviderAccount, ProviderAccountId, ProviderAccountStatus,
    ProviderConfigCheck, ProviderConfigCheckState, ProviderConfigSnapshot, ProviderConfigSource,
    ProviderConnectionTestRequest, ProviderContinuation, ProviderDiagnosticEntry,
    ProviderDiagnosticError, ProviderDiagnosticStatus, ProviderDiagnosticsCursor,
    ProviderDiagnosticsSnapshot, ProviderDisplayMode, ProviderProfileSnapshot,
    ProviderSelectionMode, ProviderTerminalReplay, ProviderUsageDiagnostic, Purpose,
    QualityArtifact, QualityArtifactId, QualityArtifactStatus, QualityStateTransition,
    QualityTransitionId, QueueEntryId, ReceiptAggregation, ReceiptId, RecoveryResourceSnapshot,
    RepoMapDependencyEdge, RepoMapEvidenceLevel, RepoMapSymbol, RepoMapTaskCandidate,
    RepoMapTaskSelection, RequestId, ResolvedAssignment, ResolvedStepContext, ResourceCleanupPlan,
    ResourceCleanupReport, ResourceLease, ResourceRetention, RetrievalCandidate,
    RetrievalEvaluationReport, RetrievalEvidence, RetrievalHealth, RetrievalHealthStatus,
    RetrievalHit, RetrievalItem, RetrievalPack, RetrievalProfile, RetrievalQualityEvidence,
    RetrievalQualityMetrics, RetrievalReceipt, RetrievalReceiptEntry, RetrievalReceiptOmission,
    RetrievalReceiptStage, RetrievalRequest, RetrievalResponse, RetrievalResult,
    RetrievalSafetyMetrics, RetrievalSourceKind, ReviewPacket, ReviewerCitation, RiskLevel,
    RoleAssignment, RoleAssignmentStatus, RoleCatalog, RoleDescriptor, RoleSpec,
    RunCancellationFact, RunCancellationState, RunId, RunReceipt, RuntimeEvent, ScopeSet,
    SecretRef, SecretRefId, SecretScanChannel, SecretSentinelFinding, SecretSentinelKind,
    SecurityContextId, SecurityDecisionId, SecurityEventEnvelope, SecurityEventId,
    SecurityObjectEnvelope, SecurityObjectKind, SecurityPolicyId, SecurityReason,
    SecurityReasonClass, SecurityReasonCode, SecurityReasonPolicy, SecurityRegistryId,
    SecurityRemediation, SecurityRetryability, SecuritySchemaEntry, SecuritySchemaRegistry,
    SensitiveDisposition, SensitiveHandling, ServiceIdentity, ServiceIdentityId, SessionAssertion,
    SessionAssignment, SessionId, SessionStatus, SharingGrant, SharingGrantId, SignalStatus,
    SkillDescriptor, SkillLifecycle, SnapshotId, SourceKind, SourceRef, SourceSnapshot, SpawnPlan,
    SpawnPlanId, StablePrefix, StablePrefixSegment, StepId, StepIdentity, StorageBackend,
    StorageCapabilities, StorageError, StorageErrorClass, StorageErrorId, StorageHealth,
    StorageHealthId, StorageHealthStatus, StorageIntegrityIncident, StorageIntegrityIncidentClass,
    StorageIntegrityIncidentId, StorageLockId, StorageLockRecord, StorageNamespace,
    StorageOwnerScope, StorageRetryDisposition, StorageRoot, StorageRootId, StorageSchemaRegistry,
    StoreIdentity, StoreIdentityId, Subscription, SubscriptionId, SubscriptionStatus,
    SupervisionLease, SupervisionLeaseId, SwarmGraphError, SwarmLineage, SwarmPlanId,
    SwarmTransitionEntity, SwarmTransitionEvent, SwarmTransitionReducer, SwarmWorkGraph, Symposium,
    TemplateId, TextNormalizationProfile, TokenAccounting, ToolOutputPage, ToolOutputSpill,
    ToolSpec, TurnId, TurnIdentity, TurnOutcome, TurnOutcomeInput, TurnOutcomeKind, TurnSemantics,
    UnknownMutationReconciliation, UnknownMutationState, UserMemoryCorrection, WireBudget,
    WireBudgetInput, WorkPacket, WorkPacketStatus, WorkspaceBinding, WorkspaceChange,
    WorkspaceChangeKind, WorkspaceEntryKind, WorkspaceFileIdentity, WorkspaceFileSnapshot,
    WorkspaceId, WorkspaceReadDisposition, WorkspaceSnapshot, WorkspaceSnapshotLimits,
    WorkspaceTrust, ACTION_REF_SCHEMA, ADAPTER_RESULT_MAX_EVIDENCE,
    ADAPTER_RESULT_MAX_OUTPUT_BYTES, ADAPTER_RESULT_SCHEMA, ADAPTER_RESULT_VERSION,
    APPROVAL_CONSUMPTION_FACT_SCHEMA, APPROVAL_DECISION_FACT_SCHEMA, APPROVAL_FACT_VERSION,
    APPROVAL_MATERIAL_SCHEMA, APPROVAL_MATERIAL_VERSION, APPROVAL_PLAN_PREVIEW_SCHEMA,
    APPROVAL_PLAN_PREVIEW_VERSION, ARTIFACT_REF_SCHEMA, ARTIFACT_VERSION_SCHEMA,
    AUTHENTICATED_PRINCIPAL_SCHEMA, AUTHORITY_FENCE_SCHEMA, AUTHORITY_FENCE_VERSION,
    AUTHORITY_LEDGER_SCHEMA, AUTHORITY_SNAPSHOT_SCHEMA, BUDGET_FACT_VERSION,
    BUDGET_RESERVATION_SCHEMA, BUDGET_SETTLEMENT_SCHEMA, CAPABILITY_OUTCOME_SCHEMA,
    CAPABILITY_RESULT_DIMENSIONS_SCHEMA, CAPABILITY_RESULT_RECEIPT_SCHEMA,
    CAPABILITY_RESULT_RECEIPT_VERSION, CHILD_HARNESS_CANCEL_SCHEMA, CHILD_HARNESS_INTENT_SCHEMA,
    CHILD_HARNESS_OUTCOME_SCHEMA, CHILD_HARNESS_VERSION, CHUNK_RANGE_SCHEMA, CHUNK_SCHEMA_VERSION,
    CHUNK_SET_SCHEMA, CLARIFICATION_ANSWER_SCHEMA, CLARIFICATION_REQUEST_SCHEMA,
    CLARIFICATION_RESOLUTION_SCHEMA, CLARIFICATION_VERSION, CLARIFICATION_WAITING_STATUS,
    CLARIFICATION_WAIT_SCHEMA, CODE_GRAPH_EDGE_SCHEMA, CODE_GRAPH_REBUILD_SCHEMA,
    COMMUNICATION_LIFECYCLE_SCHEMA, COMMUNICATION_MESSAGE_SCHEMA, COMPACT_SUMMARY_SCHEMA,
    COMPANY_COMMAND_POLICY_SCHEMA, COMPANY_COMMAND_RECEIPT_SCHEMA, COMPANY_DISPATCH_INTENT_SCHEMA,
    CONFIG_SNAPSHOT_SCHEMA, CONTEXT_CACHE_BINDING_SCHEMA, CONTEXT_CHECKPOINT_SCHEMA,
    CONTEXT_INVALIDATION_SCHEMA, CONTEXT_PLAN_SCHEMA, CONTEXT_RESUME_VIEW_SCHEMA,
    DATA_BOUNDARY_SCHEMA, DELIVERY_ATTEMPT_SCHEMA, DELIVERY_RECEIPT_SCHEMA,
    DEPARTMENT_CATALOG_SCHEMA, DEPARTMENT_EXECUTING, DEPARTMENT_MONITORING,
    DEPARTMENT_SNAPSHOT_SCHEMA, DEPARTMENT_SPEC_SCHEMA, DISPATCH_PERMIT_SCHEMA,
    DISPATCH_PERMIT_VERSION, EFFECT_OBSERVATION_SCHEMA, EFFECT_OBSERVATION_VERSION,
    EMBEDDING_INDEX_BINDING_SCHEMA, EMBEDDING_MANIFEST_SCHEMA, EMBEDDING_METADATA_SCHEMA,
    EMBEDDING_ROTATION_SCHEMA, ENTRYPOINT_PARITY_SCHEMA, ENTRYPOINT_PARITY_SCHEMA_VERSION,
    EVAL_CASE_OBJECT_SCHEMA, EVAL_DATASET_SCHEMA, EVAL_SUITE_OBJECT_SCHEMA, EVENT_KIND_SPECS,
    EVENT_STORE_HEALTH_SCHEMA, EVENT_STORE_HEALTH_VERSION, EVIDENCE_REF_SCHEMA,
    EXECUTION_IDENTITY_SCHEMA_VERSION, EXECUTION_OUTPUT_BUDGET_SCHEMA, EXECUTION_OUTPUT_REF_SCHEMA,
    EXECUTION_RECEIPT_SCHEMA, EXECUTION_SCOPE_SCHEMA, EXECUTION_SCOPE_SCHEMA_VERSION,
    EXTENSION_CATALOG_SCHEMA, EXTENSION_COMMAND_ERROR_SCHEMA, EXTENSION_COMMAND_RECEIPT_SCHEMA,
    EXTENSION_COMMAND_SCHEMA, EXTENSION_COMMAND_VERSION, EXTENSION_ERROR_SCHEMA,
    EXTENSION_MANAGE_OPERATION, EXTENSION_MANIFEST_SCHEMA, EXTENSION_PACKAGE_SCHEMA,
    EXTENSION_SNAPSHOT_CACHE_ENTRY_SCHEMA, EXTENSION_SNAPSHOT_CACHE_KEY_SCHEMA,
    EXTENSION_SNAPSHOT_SCHEMA, EXTENSION_SOURCE_RESOLUTION_SCHEMA,
    EXTERNAL_RESOURCE_REQUEST_SCHEMA, EXTERNAL_RESOURCE_SNAPSHOT_SCHEMA,
    GOLDEN_CONTEXT_CASE_SCHEMA, GOLDEN_CONTEXT_FIXTURE_SCHEMA, GOLDEN_TRACE_SCHEMA,
    HARNESS_EVAL_COMPARISON_SCHEMA, HARNESS_EVAL_SCHEMA_VERSION, HARNESS_METRIC_SET_SCHEMA,
    HARNESS_REPLAY_REPORT_SCHEMA, HARNESS_TRACE_BINDING_SCHEMA,
    HISTORICAL_RECEIPT_INVALIDATION_SCHEMA, HOOK_DECISION_SCHEMA, HOOK_DESCRIPTOR_SCHEMA,
    HOOK_LIFECYCLE_BINDING_SCHEMA, HOOK_LIFECYCLE_INPUT_SCHEMA, HOOK_LIFECYCLE_PLAN_SCHEMA,
    HOOK_LIFECYCLE_RESULT_SCHEMA, HOOK_LIFECYCLE_VERSION, HOOK_MANIFEST_SCHEMA,
    HUMAN_DECISION_SCHEMA, HUMAN_TASK_SCHEMA, IDENTITY_MIGRATION_SCHEMA, INDEX_CACHE_KEY_SCHEMA,
    INDEX_GENERATION_STATE_SCHEMA, INDEX_GENERATION_VERSION, INDEX_INVALIDATION_SCHEMA,
    INDEX_INVALIDATION_VERSION, INDEX_MANIFEST_SCHEMA, INSPECTOR_SNAPSHOT_SCHEMA,
    INVOCATION_IDENTITY_SCHEMA, LEGACY_CHECKPOINT_DECISION_SCHEMA, LEGACY_COMPATIBILITY_SCHEMA,
    LEGACY_COMPATIBILITY_VERSION, LLM_TEXT_METADATA_SCHEMA, MAX_EXTENSION_VISIBILITY_ENTRIES,
    MAX_EXTENSION_VISIBILITY_QUERY_BYTES, MEMBERSHIP_SCHEMA, MEMORY_ACL_DECISION_SCHEMA,
    MEMORY_ACL_REQUEST_SCHEMA, MEMORY_DISTILLATION_SOURCE_SCHEMA, MEMORY_EXTRACTION_REQUEST_SCHEMA,
    MEMORY_INVALIDATION_SCHEMA, MEMORY_MODEL_WRITE_GATE_SCHEMA, MEMORY_SCOPE_SCHEMA,
    MEMORY_TEMPORAL_SCHEMA, MERGE_RECEIPT_PATH, MESSAGE_SCHEMA, MODEL_ATTEMPT_IDENTITY_SCHEMA,
    MODEL_CONTENT_SCHEMA, MODEL_OUTCOME_SCHEMA, NORMALIZED_TEXT_SCHEMA,
    NOTIFICATION_EVENT_REGISTRY_SCHEMA, NOTIFICATION_SCHEMA, OUTPUT_CONTRACT_SCHEMA,
    PLUGIN_LIFECYCLE_SCHEMA, PLUGIN_MANIFEST_SCHEMA, POLICY_PROFILE_SCHEMA,
    PREPARED_MODEL_REQUEST_SCHEMA, PRINCIPAL_SCHEMA, PROGRESS_DECISION_SCHEMA,
    PROGRESS_EVIDENCE_SCHEMA, PROGRESS_TRACKER_SCHEMA, PROJECTION_CHECKPOINT_SCHEMA,
    PROJECTION_CHECKPOINT_VERSION, PROJECTION_LAG_VIEW_SCHEMA, PROJECT_ASSIGNMENT_SCHEMA,
    PROJECT_IDENTITY_SCHEMA, PROJECT_TRUST_SNAPSHOT_SCHEMA, PROVIDER_ACCOUNT_SCHEMA,
    PROVIDER_CONFIG_CHECK_SCHEMA, PROVIDER_CONFIG_SNAPSHOT_SCHEMA, PROVIDER_CONNECTION_TEST_SCHEMA,
    PROVIDER_CONTINUATION_SCHEMA, PROVIDER_DIAGNOSTICS_CURSOR_SCHEMA, PROVIDER_DIAGNOSTICS_SCHEMA,
    PROVIDER_DIAGNOSTICS_SNAPSHOT_SCHEMA, PROVIDER_PROFILE_SNAPSHOT_SCHEMA,
    PROVIDER_TERMINAL_REPLAY_SCHEMA, QUALITY_ARTIFACT_SCHEMA, QUALITY_TRANSITION_SCHEMA,
    RECEIPT_AGGREGATION_SCHEMA, RECEIPT_AGGREGATION_VERSION, RECEIPT_CONTRACT_VERSION,
    RECOVERY_RESOURCE_SNAPSHOT_SCHEMA, RECOVERY_RESOURCE_SNAPSHOT_VERSION,
    REPO_MAP_DEPENDENCY_SCHEMA, REPO_MAP_SYMBOL_SCHEMA, REPO_MAP_TASK_SELECTION_SCHEMA,
    REQUEST_BUDGET_VERSION, RESOLVED_ASSIGNMENT_SCHEMA, RESOLVED_STEP_CONTEXT_SCHEMA,
    RESOURCE_CLEANUP_PLAN_SCHEMA, RESOURCE_CLEANUP_REPORT_SCHEMA, RESOURCE_CLEANUP_VERSION,
    RESOURCE_LEASE_SCHEMA, RESOURCE_LEASE_VERSION, RESOURCE_RETENTION_SCHEMA,
    RETRIEVAL_CANDIDATE_SCHEMA, RETRIEVAL_EVALUATION_SCHEMA, RETRIEVAL_EVIDENCE_SCHEMA,
    RETRIEVAL_HEALTH_SCHEMA, RETRIEVAL_PACK_SCHEMA, RETRIEVAL_RECEIPT_SCHEMA,
    RETRIEVAL_REQUEST_SCHEMA, RETRIEVAL_RESPONSE_SCHEMA, RETRIEVAL_RESULT_SCHEMA,
    RETRIEVAL_VERSION, REVIEWER_CITATION_SCHEMA, REVIEW_PACKET_SCHEMA, ROLE_ANALYST,
    ROLE_ARCHITECT, ROLE_ASSIGNMENT_SCHEMA, ROLE_BUILDER, ROLE_CATALOG_SCHEMA, ROLE_CLOSER,
    ROLE_INPUT_SCHEMA_PREFIX, ROLE_LIBRARIAN, ROLE_OUTPUT_SCHEMA_PREFIX, ROLE_PM, ROLE_QA,
    ROLE_REVIEWER, ROLE_SPEC_SCHEMA, RUNTIME_EVENT_SCHEMA, RUN_CANCELLATION_SCHEMA,
    RUN_CANCELLATION_VERSION, RUN_RECEIPT_SCHEMA, SCOPE_SET_SCHEMA, SCOPE_SET_SCHEMA_VERSION,
    SECRET_REF_SCHEMA, SECURITY_OBJECT_SCHEMA, SECURITY_OBJECT_VERSION, SECURITY_REASON_SCHEMA,
    SECURITY_REASON_VERSION, SECURITY_SCHEMA_REGISTRY_SCHEMA, SECURITY_SCHEMA_REGISTRY_VERSION,
    SERVICE_IDENTITY_SCHEMA, SESSION_ASSERTION_SCHEMA, SESSION_ASSERTION_VERSION,
    SESSION_ASSIGNMENT_SCHEMA, SHARING_GRANT_SCHEMA, SKILL_DESCRIPTOR_SCHEMA, SOURCE_CHUNK_SCHEMA,
    SOURCE_REF_SCHEMA, SOURCE_SNAPSHOT_SCHEMA, STABLE_PREFIX_SCHEMA, STEP_IDENTITY_SCHEMA,
    STORAGE_CAPABILITIES_SCHEMA, STORAGE_ERROR_SCHEMA, STORAGE_HEALTH_SCHEMA,
    STORAGE_INTEGRITY_INCIDENT_SCHEMA, STORAGE_LOCK_SCHEMA, STORAGE_OWNER_SCOPE_SCHEMA,
    STORAGE_ROOT_SCHEMA, STORAGE_SCHEMA_REGISTRY_SCHEMA, STORE_IDENTITY_SCHEMA,
    SUBSCRIPTION_SCHEMA, SWARM_LINEAGE_SCHEMA, SWARM_PARTITION_SCHEMA,
    SWARM_TRANSITION_EVENT_SCHEMA, SWARM_WORK_GRAPH_SCHEMA, TEXT_NORMALIZATION_PROFILE_SCHEMA,
    TEXT_NORMALIZATION_VERSION, TOOL_AUTHORITY_SCHEMA, TOOL_OUTPUT_PAGE_SCHEMA,
    TOOL_OUTPUT_SPILL_SCHEMA, TOOL_SPECS, TRUST_SNAPSHOT_VERSION, TURN_IDENTITY_SCHEMA,
    TURN_OUTCOME_SCHEMA, TURN_OUTCOME_VERSION, UNIFIED_RETRIEVAL_PROFILE_SCHEMA,
    UNIFIED_RETRIEVAL_VERSION, UNKNOWN_MUTATION_RECONCILIATION_SCHEMA, USER_CORRECTION_SCHEMA,
    WIRE_BUDGET_SCHEMA, WORKSPACE_FILE_SNAPSHOT_SCHEMA, WORKSPACE_SNAPSHOT_SCHEMA,
    WORKSPACE_SNAPSHOT_VERSION, WORK_PACKET_SCHEMA,
};
pub use kiana_domain::{
    AllocationCostKind, AllocationScope, CostAllocation, CostAllocationId, SharingGrantRef,
    WorkflowInstanceId, COST_ALLOCATION_EVENT, COST_ALLOCATION_OPERATION, COST_ALLOCATION_SCHEMA,
    COST_ALLOCATION_VERSION,
};
pub use kiana_domain::{
    BillingQueryCursor, BillingQueryKind, BillingQueryRequest, BillingQueryResponse,
    BILLING_QUERY_CURSOR_SCHEMA, BILLING_QUERY_MAX_LIMIT, BILLING_QUERY_RESPONSE_SCHEMA,
    BILLING_QUERY_SCHEMA,
};
pub use kiana_domain::{
    ConnectorSurface, ConnectorSurfaceCursor, ConnectorSurfaceItem, ConnectorSurfaceQuery,
    ConnectorSurfaceQueryKind, ConnectorSurfaceResponse, CONNECTOR_SURFACE_CURSOR_SCHEMA,
    CONNECTOR_SURFACE_MAX_LIMIT, CONNECTOR_SURFACE_QUERY_SCHEMA, CONNECTOR_SURFACE_RESPONSE_SCHEMA,
};
pub use kiana_domain::{
    CostBreakdown, CostBreakdownKind, CostLine, ReceiptCostBreakdown, COST_BREAKDOWN_SCHEMA,
    COST_BREAKDOWN_VERSION, COST_EVENT_ESTIMATED, COST_EVENT_MEASURED, COST_EVENT_UNKNOWN,
    RECEIPT_COST_BREAKDOWN_SCHEMA,
};
pub use kiana_domain::{
    CostCorrection, CostCorrectionAppendOutcome, CostCorrectionApproval, CostCorrectionCommand,
    CostLedger, CostLedgerEntry, CostLedgerEntryKind, CostLedgerView,
    COST_CORRECTION_APPROVAL_SCHEMA, COST_CORRECTION_COMMAND, COST_CORRECTION_COMMAND_SCHEMA,
    COST_CORRECTION_EVENT, COST_CORRECTION_SCHEMA, COST_LEDGER_ENTRY_EVENT,
    COST_LEDGER_ENTRY_SCHEMA, COST_LEDGER_VERSION,
};
pub use kiana_domain::{
    ModelAttemptError, ModelAttemptEventKind, ModelAttemptLifecycleEvent,
    ModelAttemptLifecycleRecord, ModelAttemptState, MODEL_ATTEMPT_EVENT_SCHEMA,
    MODEL_ATTEMPT_EVENT_VERSION, MODEL_ATTEMPT_LIFECYCLE_SCHEMA, MODEL_ATTEMPT_LIFECYCLE_VERSION,
};
pub use kiana_domain::{
    NotificationActionCommand, NotificationActionKind, NOTIFICATION_ACTION_COMMAND_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod ops;
pub use ops::*;

mod ui_contracts;
pub use ui_contracts::*;

pub use kiana_domain::{CapabilityRequest, ConversationMessage, ConversationRole};

pub use kiana_domain::{
    normalize_connector_intent, ConnectorCommand, ConnectorCommandRequest, ConnectorDataBoundary,
    ConnectorManageAction, ConnectorNormalizedIntent, ConnectorProtocolError,
    ConnectorProtocolErrorCode, CONNECTOR_COMMAND_SCHEMA, CONNECTOR_COMMAND_VERSION,
    CONNECTOR_DATA_BOUNDARY_SCHEMA, CONNECTOR_NORMALIZED_INTENT_SCHEMA,
    CONNECTOR_PROTOCOL_ERROR_SCHEMA, CONNECTOR_PROTOCOL_MAX_IDEMPOTENCY_BYTES,
    CONNECTOR_PROTOCOL_MAX_PAYLOAD_BYTES, CONNECTOR_PROTOCOL_MAX_REASON_BYTES,
};

pub use kiana_domain::{
    AuditRecordEvent, AUDIT_EVENT_KIND, AUDIT_EVENT_SCHEMA, AUDIT_EVENT_SCHEMA_VERSION,
};

pub use kiana_domain::{
    CompanyClosingReceipt, CompanyCommand, CompanyCommandRequest, CompanyState, COMPANY_COMMAND,
    COMPANY_COMMAND_SCHEMA, COMPANY_GOVERNANCE, COMPANY_SNAPSHOT, COMPANY_STATE_SCHEMA,
};

///  【作用】 整个 `kiana.protocol.v1` 的唯一版本常量。请求、响应、运行中事件共用这一个字符串——注意 `RunStreamEnvelope` 也用它，故意不给事件通道单独开版本号，这样一条流里不会出现两种 schema 名。
///  【调用者】 daemon 在 `DaemonHost::handle` 里做 schema 相等 + 版本兼容性双重检查后，回 `protocol_schema_unsupported`；`kiana-client` 的 typed 客户端收到不认识的 schema 会报 `ClientError::UnknownSchema` 而不是继续解析。
/// 【⚠ 不要给某个命令写死一个私有版本串来"更精确地升级"。本文件后面几十个 `kiana.<x>-command.v1` 之类常量只是 payload 级的形状标识，不是第二层协议版本；换掉 `PROTOCOL_SCHEMA` 会同时让所有老 daemon 回拒绝。】
///
pub const PROTOCOL_SCHEMA: &str = "kiana.protocol.v1";
///  成本更正命令的**注册名**（command registry key），不是 schema 名。请求、事件、审计三处共用同一串字符串；改它等于换一条命令。
/// 【⚠ 它是 `RequestBody::Command{name, arguments}` 里的 `name`，而 `name` 是自由字符串——协议层无法阻止调用方拼错成 `cost.correction.v2`，那会在 Core 的命令路由里变成未知命令而不是这里报 schema 错误。】
///
pub const COST_CORRECTION_COMMAND_NAME: &str = "cost.correction";
pub const COST_CORRECTION_COMMAND_REQUEST_SCHEMA: &str = "kiana.cost-correction-command-request.v1";
pub const COST_CORRECTION_COMMAND_RESPONSE_SCHEMA: &str =
    "kiana.cost-correction-command-response.v1";
pub const AUDIT_QUERY_SCHEMA: &str = "kiana.audit-query.v1";
///  质量/评测命令 payload 的形状标识。`kiana-core` 的 schema 契约注册表把它登记为 owner_crate = kiana-protocol，所以这个字符串同时是"谁负责这个 schema"的声明。
/// 【⚠ 它只出现在 `QualityCommandRequest.schema` 字段里，不会出现在 `RequestEnvelope.schema`。加了新字段想让老 daemon 读得动，得靠 `serde(default)`，不是靠改这个串。】
///
pub const QUALITY_COMMAND_SCHEMA: &str = "kiana.quality-command.v1";
///  质量命令的完整白名单，与本文件里的 `QUALITY_EVENT_KINDS` 常量目前逐项相同。
///  【作用】 两份列表分开写而不是共用一个，是为了将来命令名和事件名可以分化（例如某个命令产生多个事件）。目前它们相等，测试 `eq06_quality_protocol.rs` 只断言每项都含 `.` 且每个事件名都能在 `kiana-domain` 的事件注册表里查到。
/// 【⚠ 加新命令必须同步 `kiana-core` 的命令路由和 `kiana-domain` 的 `EVENT_KIND_SPECS`，否则事件会在落盘时被当成"未注册的必需事件"直接拒收。】
///
pub const QUALITY_COMMAND_KINDS: &[&str] = &[
    "eval.run",
    "eval.capture",
    "eval.compare",
    "eval.explain",
    "eval.list",
    "quality.feedback",
    "quality.promote",
    "quality.rollback",
];
pub const QUALITY_EVENT_KINDS: &[&str] = &[
    "eval.run",
    "eval.capture",
    "eval.compare",
    "eval.explain",
    "eval.list",
    "quality.feedback",
    "quality.promote",
    "quality.rollback",
];
///  澄清（clarification，模型挂起并向用户提问）回答所走的命令名。
///  【核心流程】 它是普通 `RequestBody::Command`，因此复用同一条 daemon 命令路由；但 `kiana-core` 处理它时会先落库持久化答案再恢复原 turn，调用方不能自己重发 prompt 绕过挂起态。
///
pub const CLARIFICATION_ANSWER_COMMAND: &str = "run.clarification.answer";
///  客户端侧展示状态的形状标识，由 `RunDisplayState::new` 写入、由 `RunDisplayState::validate` 比对。
///  【设计意图】 状态机自己也带 schema，这样"这块屏幕的数据是哪一版形状的"在客户端本地就能判定，不必回头问服务端。
///
pub const RUN_DISPLAY_STATE_SCHEMA: &str = "kiana.run-display-state.v1";
///  展示缓冲区上限，护的是 `RunDisplayState.text`（客户端内存），不是模型上下文或事件日志。
///  【核心流程】 两处强制：`validate()` 校验当前累积长度，`apply_event()` 在 push 之前用 `saturating_add` 预判。
/// 【⚠ 这个 128 KiB 的来源在仓库里查不到可发现的理由，不要假装它是某个窗口预算的推导值；它是防御性上限——超限返回 `display_text_limit`，防止一条畸形增量流把客户端撑爆。】
///
pub const MAX_RUN_DISPLAY_TEXT_BYTES: usize = 128 * 1024;
/// A projection epoch is a server-owned opaque token, never a path or a free-text message.
///  epoch（投影世代标识）长度上限，强制点在 `UiCursor::validate`。
///  【作用】 epoch 标识 daemon 的一次存续。daemon 重启换新 epoch 后，旧 epoch 的增量一律作废、必须重新水合快照——这是防止"重启后把两段不相干的输出拼在一起"的唯一机制。
/// 【⚠ `RunStreamEnvelope` 刻意用 `#【serde(default)】` 允许空 epoch（老 envelope 没有这个字段）。这个上限管的是"有 epoch 时不能太长/不能含控制字符"，管不了缺失的 epoch。】
///
pub const MAX_UI_EPOCH_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
///  质量命令的类型化枚举。`as_str()` 产出的是注册名，最终仍以 `RequestBody::Command` 出去。
///  【设计意图】 目的是让"这批命令"在协议层就**被枚举穷尽**——加一个变体只改这一处，不会漏掉某个字符串常量表。`QUALITY_COMMAND_KINDS` 反过来是给遍历用的扁平列表，两者必须同步改。
///
pub enum QualityCommandKind {
    EvalRun,
    EvalCapture,
    EvalCompare,
    EvalExplain,
    EvalList,
    Feedback,
    Promote,
    Rollback,
}

impl QualityCommandKind {
    ///      【作用】 把枚举翻译成 wire 上的点分命令名。这是命令名唯一的产生点，`QUALITY_COMMAND_KINDS` 里的字符串就是本方法的字面量复制。
    ///      【为什么是 const fn】 让常量表和运行时编码共享同一份真值，避免"常量表写错但构造走另一条路"这种只有单测能发现的漂移。
    ///
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EvalRun => "eval.run",
            Self::EvalCapture => "eval.capture",
            Self::EvalCompare => "eval.compare",
            Self::EvalExplain => "eval.explain",
            Self::EvalList => "eval.list",
            Self::Feedback => "quality.feedback",
            Self::Promote => "quality.promote",
            Self::Rollback => "quality.rollback",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  质量命令的 payload DTO，最终被 `RequestEnvelope::quality_command` 压成 `CommandRequest.arguments`。
///  【作用】 它存在的意义是让 `arguments` 这个 `serde_json::Value` 有一个可校验的静态外形；`deny_unknown_fields` 保证拼错的字段会直接反序列化失败，而不是被静默忽略。
/// 【⚠ `arguments: Value` 这里没有类型化，所以 `validate()` 必须自己检查它是 JSON object——否则 `eval.run` 之类命令会收到数组。】
///
pub struct QualityCommandRequest {
    pub schema: String,
    pub command: QualityCommandKind,
    pub request_id: RequestId,
    pub arguments: Value,
}

impl QualityCommandRequest {
    ///      【作用】 构造时就把 `schema` 填成本文件的常量，调用方无法构造出一个"忘记填 schema"的请求。
    ///      【注意】 这里**不**调用 `validate()`。先编码后校验（`RequestEnvelope::quality_command` 里才调）是刻意的：这样测试可以先造出一个坏 schema 再手动改字段来测错误路径。
    ///
    pub fn new(command: QualityCommandKind, request_id: RequestId, arguments: Value) -> Self {
        Self {
            schema: QUALITY_COMMAND_SCHEMA.to_owned(),
            command,
            request_id,
            arguments,
        }
    }

    ///      【作用】 fail-closed 的形状检查。两个失败原因分开报：schema 不匹配 → `quality_command_schema_invalid`；`arguments` 不是 object → `quality_command_arguments_invalid`。
    ///      【失败情况】 两者都是返回 `Err(String)` 而不是 panic——协议层只能拒绝，不能替调用方猜一个默认值。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_COMMAND_SCHEMA || self.request_id.as_uuid().is_nil() {
            return Err("quality_command_schema_invalid".to_owned());
        }
        if !self.arguments.is_object() {
            return Err("quality_command_arguments_invalid".to_owned());
        }
        Ok(())
    }
}

///  serde 反序列化缺字段时的兜底角色。
///  【设计意图】 选 Builder / executing（执行部门）而不是"无角色"，是因为这两个角色是**权限最小**的一档：一个没声明角色的老客户端应当自动落到能力最弱的位置，而不是被放行。
/// 【⚠ 这是兼容性策略不是授权结论。daemon 拿到后仍要 `RoleSpec::lookup` 并检查 `principal.allowed_roles`，默认值通过不代表这次请求被批准。】
///
fn default_role_id() -> String {
    ROLE_BUILDER.to_owned()
}

///  角色缺省时对应的部门，与 `default_role_id` 必须成对出现。
/// 【⚠ daemon 会额外做 `metadata.department_id == role.department_id` 的一致性检查（不符即 `role_department_mismatch`）。只改其中一个默认值会让所有缺字段的老请求在 daemon 侧被拒。】
///
fn default_department_id() -> String {
    DEPARTMENT_EXECUTING.to_owned()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 请求级身份、项目和权限范围快照。
///  请求级身份、项目和权限范围快照——**每个** envelope 都必须带的那块。
///  【作用】 它是控制面重建上下文的唯一输入：daemon 不信任传输层，只信任它能自己复核的字段。所以 `project_trusted` 只是个声明（daemon 会用 `project_authority` 重新查一遍），`permission_profile` 是请求档位不是授权结果。
///  【兼容性策略】 大量字段带 `#【serde(default)】`/`skip_serializing_if`，是为了让新客户端写新字段时老 daemon 仍能反序列化。但注意本结构体**没有** `deny_unknown_fields`——加字段天然兼容，删/改字段会让老客户端把新值丢掉继续跑。
///  【副作用】 无。它是纯数据。
///
pub struct RequestMetadata {
    /// 请求唯一 ID，用于事件和响应关联。
    pub request_id: RequestId,
    /// Optional client deadline carried through the wire envelope.  The daemon still owns the
    /// authoritative timeout and must fail closed when this value is stale or malformed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_unix_ms: Option<u64>,
    /// session 稳定 ID。
    pub session_id: SessionId,
    /// 项目根目录文字。
    pub project_root: String,
    /// 可选调用主体。
    pub actor_id: Option<String>,
    /// 项目是否已通过 trust 检查。
    pub project_trusted: bool,
    /// 请求权限档位；不是授权结果。
    pub permission_profile: PermissionProfile,
    #[serde(default = "default_role_id")]
    pub role_id: String,
    #[serde(default = "default_department_id")]
    pub department_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_packet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_allow: Vec<String>,
    /// Optional protected-ingress instance identity; absent means legacy local compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// Optional Origin header projection checked by the daemon for loopback-only transports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Optional Host header projection checked by the daemon for loopback-only transports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Opaque credential reference; raw bearer/API values are never accepted on the wire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<kiana_domain::SecretRef>,
    /// Explicit compatibility marker for the historical local-user migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_mode: Option<String>,
    /// Optional server-recognized entrypoint label; it is correlation metadata, never authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<EntryPointKind>,
}

impl RequestMetadata {
    /// 构造默认本地元数据，默认 Safe 且未信任项目。
    pub fn local(session_id: impl Into<String>, project_root: impl Into<String>) -> Self {
        Self {
            request_id: RequestId::new(),
            deadline_unix_ms: None,
            session_id: SessionId::new(session_id),
            project_root: project_root.into(),
            actor_id: Some("local-user".to_owned()),
            project_trusted: false,
            permission_profile: PermissionProfile::Safe,
            role_id: default_role_id(),
            department_id: default_department_id(),
            work_packet_id: None,
            path_allow: Vec::new(),
            instance_id: None,
            origin: None,
            host: None,
            credential_ref: None,
            identity_mode: None,
            entrypoint: None,
        }
    }

    pub fn with_entrypoint(mut self, entrypoint: EntryPointKind) -> Self {
        self.entrypoint = Some(entrypoint);
        self
    }

    /// 用角色快照同步 role/department 字段。
    pub fn assign_role(&mut self, role: &RoleSpec) {
        self.role_id = role.role_id.clone();
        self.department_id = role.department_id.clone();
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 带 schema、元数据和具体请求体的完整协议 envelope。
pub struct RequestEnvelope {
    /// 协议 schema 版本。
    pub schema: String,
    /// 请求身份和范围元数据。
    pub metadata: RequestMetadata,
    /// 具体操作及其参数。
    pub body: RequestBody,
}

///  所有请求构造器的宿主。
///  【核心流程】 两种产出路径：1) `command(metadata, name, arguments)`——通用命令，所有新业务都走它，daemon 侧统一落到 `ControlPlane::handle_command`；2) 每个具体 body 变体各有一个构造器，最终都等价于拼一个 `RequestEnvelope { schema, metadata, body }`。
///  【设计意图】 这些构造器只做"编码"，不做"授权"。授权决策必须在能拿到 server 状态的地方做一次，不能每个 client 复制一遍。
/// 【⚠ 不要给某个 helper 加"便利的"前置校验（比如信任检查）。这类检查一旦出现在 client 侧，绕过 client 直接发 envelope 的路径就失去了一道防线，而 daemon 侧未必有对应检查。】
///
impl RequestEnvelope {
    /// 构造通用命令 envelope。
    /// Submit a versioned Company business command through the existing command route.
    ///      公司（Company）业务命令的统一入口。
    ///      【设计意图】 Company / Workflow / Swarm 三族业务刻意复用 `RequestBody::Command` 这条既有命令通道，而不是给 `RequestBody` 加新变体——那样每加一族业务就要动 `kiana-daemon` 的 `match`、权限映射表、只读判定表三处。
    ///      【代价】 换来的是命令名变成自由字符串，协议层无法静态校验拼写。
    ///
    pub fn company_command(
        metadata: RequestMetadata,
        request: CompanyCommandRequest,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self::command(
            metadata,
            COMPANY_COMMAND,
            serde_json::to_value(request)?,
        ))
    }

    /// Read the event-derived Company state for this principal and workspace.
    pub fn company_snapshot(metadata: RequestMetadata) -> Self {
        Self::command(metadata, COMPANY_SNAPSHOT, serde_json::json!({}))
    }

    /// Read a server-owned CompanyOS runtime→review→acceptance→delivery→close chain.
    pub fn company_governance(metadata: RequestMetadata, project_id: impl Into<String>) -> Self {
        Self::command(
            metadata,
            COMPANY_GOVERNANCE,
            serde_json::json!({"project_id": project_id.into()}),
        )
    }

    pub fn workflow_command(
        metadata: RequestMetadata,
        request: AutomationCommandRequest,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self::command(
            metadata,
            AUTOMATION_COMMAND,
            serde_json::to_value(request)?,
        ))
    }
    pub fn workflow_snapshot(metadata: RequestMetadata) -> Self {
        Self::command(metadata, AUTOMATION_SNAPSHOT, serde_json::json!({}))
    }
    pub fn swarm_command(
        metadata: RequestMetadata,
        request: SwarmCommandRequest,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self::command(
            metadata,
            SWARM_COMMAND,
            serde_json::to_value(request)?,
        ))
    }
    pub fn swarm_snapshot(metadata: RequestMetadata) -> Self {
        Self::command(metadata, SWARM_SNAPSHOT, serde_json::json!({}))
    }

    ///      通用命令构造器：`RequestBody::Command` 的唯一产地。
    ///      【作用】 `arguments` 是 `serde_json::Value`，所以这个函数不做任何形状检查。typed helper（`ops_command` / `quality_command` / `connector_command` / `extension_command` / `cost_correction_command`）先各自 `validate()` 再落到这里。
    /// 【⚠ 直接用裸字符串命令名调用它可以编出任何命令，包括 daemon 只读白名单里没有的那些。daemon 侧的 `request_may_execute` 对未知命令名默认返回"可执行"，所以安全性完全落在 Core 的命令路由上。】
    ///
    pub fn command(metadata: RequestMetadata, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Command(CommandRequest {
                name: name.into(),
                arguments,
            }),
        }
    }

    /// Submit a correction through the normal versioned command route. This helper only encodes
    /// the command; authorization, approval consumption and EventLog append remain ControlPlane
    /// responsibilities.
    ///      成本更正命令的编码入口。
    ///      【核心流程】 先构造并校验 `CostCorrectionCommandRequest`，再序列化成 `arguments`。返回 `Result` 是因为这里会 fail-closed：非法请求在**发出去之前**就变成错误字符串。
    ///      【设计意图】 调用方不能提交一个替换后的账本数值——目标条目和审批上下文都由 daemon 解析。
    ///
    pub fn cost_correction_command(
        metadata: RequestMetadata,
        command: CostCorrectionCommand,
    ) -> Result<Self, String> {
        let request = CostCorrectionCommandRequest::new(command);
        request.validate()?;
        let arguments = serde_json::to_value(request)
            .map_err(|_| "cost_correction_command_encode_failed".to_owned())?;
        Ok(Self::command(
            metadata,
            COST_CORRECTION_COMMAND_NAME,
            arguments,
        ))
    }

    /// Construct a typed extension command through the shared `DaemonHost → ControlPlane`
    /// route. Validation only checks the wire contract; authorization and lifecycle mutation
    /// remain server-owned.
    pub fn extension_command(
        metadata: RequestMetadata,
        request: ExtensionCommandRequest,
    ) -> Result<Self, String> {
        let name = request.command.wire_name();
        let arguments = request.to_arguments()?;
        Ok(Self::command(metadata, name, arguments))
    }

    /// Construct one of the registered evaluation/quality commands through the normal daemon
    /// command route. This helper does not execute or authorize the command.
    pub fn quality_command(
        metadata: RequestMetadata,
        request: QualityCommandRequest,
    ) -> Result<Self, String> {
        request.validate()?;
        Ok(Self::command(
            metadata,
            request.command.as_str(),
            request.arguments,
        ))
    }

    /// Construct a typed ops command on the shared RequestEnvelope spine. This only validates
    /// and encodes the DTO; Core/DaemonHost still own authority comparison and admission.
    pub fn ops_command(
        metadata: RequestMetadata,
        request: OpsCommandRequest,
    ) -> Result<Self, String> {
        request.validate()?;
        let name = request.command.clone();
        let arguments =
            serde_json::to_value(request).map_err(|_| "ops_command_encode_failed".to_owned())?;
        Ok(Self::command(metadata, name, arguments))
    }

    /// Construct a typed read-only ops query on the shared RequestEnvelope spine.
    pub fn ops_query(metadata: RequestMetadata, request: OpsQueryRequest) -> Result<Self, String> {
        request.validate()?;
        let name = request.query.clone();
        let arguments =
            serde_json::to_value(request).map_err(|_| "ops_query_encode_failed".to_owned())?;
        Ok(Self::command(metadata, name, arguments))
    }

    /// Construct a typed connector command through the same versioned route used by every
    /// surface. This helper only validates/encodes the wire DTO; trust, binding lookup, policy,
    /// approval and Broker dispatch remain server-owned ControlPlane work.
    pub fn connector_command(
        metadata: RequestMetadata,
        request: ConnectorCommandRequest,
    ) -> Result<Self, String> {
        let name = request.command.wire_name();
        let arguments = request.to_arguments()?;
        Ok(Self::command(metadata, name, arguments))
    }

    /// 构造不带证明的审批决定 envelope。
    pub fn approval_decision(
        metadata: RequestMetadata,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Self {
        Self::approval_decision_with_proof(metadata, approval_id, decision, None, None)
    }

    /// 构造带 request hash/nonce 证明字段的审批决定 envelope。
    pub fn approval_decision_with_proof(
        metadata: RequestMetadata,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<String>,
        nonce: Option<String>,
    ) -> Self {
        Self::approval_decision_with_proof_and_version(
            metadata,
            approval_id,
            decision,
            request_hash,
            nonce,
            None,
        )
    }

    /// Construct a decision with challenge proof and an optional server-issued aggregate
    /// version.  The version is an optimistic-concurrency assertion, not a client authorization.
    ///      审批决定三个构造器中最完整的一个，前两个（`approval_decision` / `approval_decision_with_proof`）都是它的特例。
    ///      [为什么走独立 body 变体而不走通用 command] 因为审批消费是控制面里唯一带乐观并发（optimistic concurrency）前置条件的写入。它在 daemon 里有专门的早退检查：`request_hash` 或 `nonce` 任一为空 → `approval_proof_required`；`actor_id` 与 principal 不符 → `approval_context_mismatch`。
    ///      【关键】 `expected_version` 是客户端"我看到的是第 N 版"的断言，不是一张授权票。版本不符时 `kiana-core` 返回 `approval_expected_version_conflict`，不会替客户端重试。
    ///
    pub fn approval_decision_with_proof_and_version(
        metadata: RequestMetadata,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<String>,
        nonce: Option<String>,
        expected_version: Option<u64>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::ApprovalDecision(ApprovalDecisionRequest {
                approval_id,
                decision,
                request_hash,
                nonce,
                expected_version,
            }),
        }
    }

    /// Construct the regular-input clarification answer command. Core must validate and persist
    /// the answer before resuming the original turn; this helper carries no approval material.
    ///      把澄清回答（clarification answer）编成普通命令。
    ///      【设计意图】 它不带任何审批材料——回答问题不是授权动作。Core 必须先把答案持久化再恢复被挂起的 turn，所以调用方不能靠再发一条 `run` 来"自己接上"。
    ///
    pub fn answer_clarification(
        metadata: RequestMetadata,
        answer: ClarificationAnswer,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self::command(
            metadata,
            CLARIFICATION_ANSWER_COMMAND,
            serde_json::to_value(answer)?,
        ))
    }

    /// 构造没有显式历史的 run envelope。
    pub fn run(
        metadata: RequestMetadata,
        prompt: impl Into<String>,
        sandbox: Option<String>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Run(RunRequest {
                prompt: prompt.into(),
                history: Vec::new(),
                sandbox,
            }),
        }
    }

    /// 构造带结构化会话历史的 run envelope。
    pub fn run_with_history(
        metadata: RequestMetadata,
        prompt: impl Into<String>,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Run(RunRequest {
                prompt: prompt.into(),
                history,
                sandbox,
            }),
        }
    }

    /// 构造 continue envelope。
    /// Negotiated v2 turn semantics; the v1 Continue request retains its old wire behavior.
    ///      v2 轮次（turn）语义的新入口，注意它**不是** `RequestBody::Continue`。
    ///      【核心流程】 编成 `run.turn.v2` 命令，`kiana-core` 识别这个名字后调 `continue_new_turn`——要求前一个 run 已终态（否则 `run_not_terminal_use_resume`），并且终态若是 `ResultUnknown` 会被拒绝（未知结果必须先对账，不能直接开新轮）。
    ///      【为什么用命令而不是新 body 变体】 v1 的 `Continue` 保持原样不动，v2 走显式版本化命令名，这样老 daemon 收到 `run.turn.v2` 只当未知命令处理，不会误解成 continue。
    /// 【⚠ 名字写死在这里、也写死在 `kiana-core` 的命令路由和事件记录里（`request.accepted` 的 `command` 字段在有前驱时记 `run.turn.v2`，无前驱时记 `run.start`）。三处必须一致，否则事件回放会串。】
    ///
    pub fn new_turn(
        metadata: RequestMetadata,
        prompt: impl Into<String>,
        sandbox: Option<String>,
        run_id: Option<RunId>,
    ) -> Self {
        Self::command(
            metadata,
            "run.turn.v2",
            serde_json::json!({"prompt":prompt.into(),"sandbox":sandbox,"run_id":run_id}),
        )
    }

    pub fn continue_run(
        metadata: RequestMetadata,
        prompt: impl Into<String>,
        sandbox: Option<String>,
        run_id: Option<RunId>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Continue(ContinueRequest {
                prompt: prompt.into(),
                sandbox,
                run_id,
            }),
        }
    }

    /// Queue a steering message for the current turn; Core validates the expected turn before
    /// forwarding it to Runner.
    ///      给当前轮排队一条转向指令（steer）。
    ///      【核心流程】 `expected_turn_id` 是乐观并发检查：Core 里比对当前 run 的 turn，不符即 `stale_turn_steer`。这是防止"用户对着第 3 轮的界面打字，指令却插进第 5 轮"。
    ///      【设计意图】 steer 在下一个安全步骤边界生效，不打断正在进行的工具调用。
    ///
    pub fn steer_run(
        metadata: RequestMetadata,
        run_id: RunId,
        expected_turn_id: TurnId,
        text: impl Into<String>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Steer(SteerRequest {
                run_id,
                expected_turn_id,
                text: text.into(),
            }),
        }
    }

    /// Queue non-waking context input for a future safe turn/step boundary.
    ///      投递上下文输入，**不唤醒**空闲的 run。
    ///      【核心流程】 与 `steer_run` 共用 Core 的 `queue_run_input`，差别在两点：target 由调用方给（Core 限 `next-step` / `next-turn`），且不传 `expected_turn_id`，所以不做轮次过期检查。
    ///      【设计意图】 `source` 是必填标签，Core 会对它做长度和 NUL 字节校验——上下文注入必须能追溯来源，否则"这段记忆从哪来的"就永远说不清。
    ///
    pub fn inject_run(
        metadata: RequestMetadata,
        run_id: RunId,
        target: impl Into<String>,
        source: impl Into<String>,
        text: impl Into<String>,
        target_turn_id: Option<TurnId>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Inject(InjectRequest {
                run_id,
                target: target.into(),
                source: source.into(),
                text: text.into(),
                target_turn_id,
            }),
        }
    }

    /// 构造 cancel envelope。
    ///      取消 run 的构造器。
    ///      【设计意图】 `reason` 是必填（`#【serde(default)】` 只保证老包能反序列化），因为它要进事件和 receipt——"为什么取消"是事后对账的一部分。
    /// 【⚠ 取消不等于回滚。已发生的副作用由 capability 层的补偿逻辑处理，这里只表达"不要再往下走"。】
    ///
    pub fn cancel_run(
        metadata: RequestMetadata,
        run_id: Option<RunId>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Cancel(CancelRequest {
                run_id,
                reason: reason.into(),
            }),
        }
    }

    /// 构造 WorkPacket spawn envelope。
    ///      WorkPacket（工作包）执行请求。
    ///      【设计意图】 Builder packet 执行必须是**全新会话**：这里只传 packet，不传 prompt 或 transcript。`WorkPacket` 自己有 `as_prompt()`，由 daemon 侧从 packet 渲染，客户端无权改写。
    /// 【⚠ 这是有意的隔离：测试 `spawn_envelope_round_trips_packet_without_transcript` 断言 envelope 里不存在 `prompt` 字段，`packet.as_prompt()` 也不含规划阶段的 secret 标记。】
    ///
    pub fn spawn(metadata: RequestMetadata, packet: WorkPacket, sandbox: Option<String>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Spawn(SpawnRequest { packet, sandbox }),
        }
    }

    /// 构造 symposium convene envelope。
    ///      规划研讨会（symposium） convene 请求。
    ///      【设计意图】 `anti_meeting` 和 `max_rounds` 是防发散的结构性闸门，不是提示词技巧。`max_rounds` 走 `default_symposium_max_rounds` 兜底，指向 `Symposium::DEFAULT_MAX_ROUNDS`，保证老客户端省略时仍有上限。
    /// 【⚠ 与 spawn 同理：envelope 只传 `goal`，不带 transcript；角色由 metadata 声明（测试断言 `ROLE_PM`）。】
    ///
    pub fn symposium(
        metadata: RequestMetadata,
        goal: impl Into<String>,
        anti_meeting: bool,
        max_rounds: u32,
        sandbox: Option<String>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Symposium(SymposiumRequest {
                goal: goal.into(),
                anti_meeting,
                max_rounds,
                sandbox,
            }),
        }
    }

    /// 构造 review envelope。
    ///      监控方（Reviewer）对某个作者会话发起评审。
    ///      【设计意图】 只带 `author_session_id`（可选 `author_run_id`）做引用，不传 transcript——评审要基于事件账本重新读事实，而不是接收别人递过来的叙述。
    ///  【边界】 daemon 会把 Review/Close 的权限档位固定成 `Balanced`，不采信 metadata 里声明的值。
    ///
    pub fn review(
        metadata: RequestMetadata,
        author_session_id: impl Into<String>,
        author_run_id: Option<RunId>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Review(ReviewRequest {
                author_session_id: author_session_id.into(),
                author_run_id,
            }),
        }
    }

    /// 构造 close envelope。
    ///      收尾（Close）请求，形状与 review 对称。
    ///      【设计意图】 同样只引用作者标识。close 产出收据类事实，是"这条链到此为止"的证据，不是"删除记录"。
    ///
    pub fn close(
        metadata: RequestMetadata,
        author_session_id: impl Into<String>,
        author_run_id: Option<RunId>,
    ) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Close(CloseRequest {
                author_session_id: author_session_id.into(),
                author_run_id,
            }),
        }
    }

    /// 构造 receipt 查询 envelope。
    ///      读取 run receipt（收据，即执行事实的投影）的请求。
    ///      【调用者】 daemon 侧 `Receipt` 属于只读分支：权限档位强制 `Safe`，`request_may_execute` 返回 false（不占用执行预约）。
    ///      【设计意图】 receipt 是唯一的事实来源——流式增量里的文本不是执行事实。
    ///
    pub fn receipt(metadata: RequestMetadata, run_id: Option<RunId>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Receipt(ReceiptRequest { run_id }),
        }
    }

    /// Construct a bounded audit query; ownership and actor filters are added by the daemon.
    pub fn audit_query(metadata: RequestMetadata, query: AuditQueryRequest) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::AuditQuery(query),
        }
    }

    /// Construct a read-only billing projection query. Budget/usage/export/reconciliation data
    /// remain derived from the server-owned BQ-20 snapshot; this envelope does not authorize or
    /// consume a reservation or approval.
    pub fn billing_query(metadata: RequestMetadata, query: BillingQueryRequest) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::BillingQuery(query),
        }
    }

    /// Construct a controlled audit export request; the daemon supplies ownership and evidence.
    pub fn audit_export(metadata: RequestMetadata, export: AuditExportRequest) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::AuditExport(export),
        }
    }

    /// Construct a read-only cross-entrypoint parity projection request.
    pub fn parity(metadata: RequestMetadata, request: ParityRequest) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Parity(request),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "request", rename_all = "snake_case")]
/// 协议允许的请求种类；新增分支必须由 daemon 统一处理。
///  协议允许的请求种类。这是**封闭枚举**：daemon 的 `match request.body` 覆盖每一个变体。
///  【核心流程】 serde 把它编成带 `type` / `request` 两个键的**相邻**标签形式（adjacently tagged），所以每种请求在 JSON 上都长成 `{"type": "...", "request": {...}}`。测试大量断言 `encoded【"body"】【"type"】` 正是这个约定的守卫。
///  【设计意图】 新增一种"不产生副作用的读取"时优先加变体（会被 `match` 逼着补齐处理分支）；只有当"载荷形状和路由都要变"时才走 `Command`。变体多本身是代价：daemon 里有三张表要同步（`handle` 的 match、`request_may_execute`、`effective_permission_profile`），漏一张就等于给新请求默认放开了错误档位。
/// 【⚠ 这三张表的兜底方向并不一致：`request_may_execute` 有 `_ => true` 兜底（漏改会静默放行为"可执行"），而 `effective_permission_profile` 没有 `_` 分支（漏改直接编译不过）。两种失败后果不同，加变体时别照抄。】
///
pub enum RequestBody {
    /// 通用命令。
    Command(CommandRequest),
    /// 审批决定。
    ApprovalDecision(ApprovalDecisionRequest),
    /// 新建 run。
    Run(RunRequest),
    /// 继续 run。
    Continue(ContinueRequest),
    /// Steer the current turn at its next safe step boundary.
    Steer(SteerRequest),
    /// Inject source-labelled input without waking an idle turn.
    Inject(InjectRequest),
    /// Explicitly restore a paused run from the event ledger.
    Resume(ResumeRequest),
    /// Read the same pending approvals from every surface.
    ListApprovals(ApprovalListRequest),
    /// 取消 run。
    Cancel(CancelRequest),
    /// 读取 receipt。
    Receipt(ReceiptRequest),
    /// Read a bounded server-authenticated audit projection.
    AuditQuery(AuditQueryRequest),
    /// Materialize a bounded redacted audit export through ControlPlane.
    AuditExport(AuditExportRequest),
    /// Read a bounded billing projection without mutating reservations or approvals.
    BillingQuery(BillingQueryRequest),
    /// Read the same audit/health/receipt parity projection on every surface.
    Parity(ParityRequest),
    /// 申请 spawn。
    Spawn(SpawnRequest),
    /// 召开 symposium。
    Symposium(SymposiumRequest),
    /// 发起 review。
    Review(ReviewRequest),
    /// 发起 close。
    Close(CloseRequest),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 通用命令名称和 JSON 参数。
///  通用命令信封：`name` 是注册名，`arguments` 是不透明 JSON。
///  【设计意图】 这是协议的"逃生舱"。所有不想动 `RequestEnvelope` 的业务族（Company / Workflow / Swarm / ops / quality / extension / connector / cost correction / 澄清回答 / v2 turn）都走它。
///  【代价】 完全放弃了编译期检查：拼错的命令名要到 `kiana-core` 的路由里才会变成未知命令。
///
pub struct CommandRequest {
    /// 命令注册名。
    pub name: String,
    /// 命令参数。
    pub arguments: Value,
}

/// Typed wire request for the append-only cost correction command. The daemon resolves the
/// server-owned target and approval context; callers cannot submit a replacement ledger value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  成本更正命令的请求 DTO，随后被塞进 `CommandRequest.arguments`。
///  【设计意图】 `deny_unknown_fields` 是这条命令的防篡改点：调用方能传的字段只有 schema 和 command 本身，目标条目/审批上下文必须由 daemon 解析出来。多一个字段就是多一条绕过路径，所以直接拒收而不是忽略。
///
pub struct CostCorrectionCommandRequest {
    pub schema: String,
    pub command: CostCorrectionCommand,
}

impl CostCorrectionCommandRequest {
    pub fn new(command: CostCorrectionCommand) -> Self {
        Self {
            schema: COST_CORRECTION_COMMAND_REQUEST_SCHEMA.to_owned(),
            command,
        }
    }

    ///      两层校验：先比 `schema` 常量，再把 `command` 的自有校验（`CostCorrectionCommand::validate`）透传出去。
    ///      【失败情况】 统一返回字符串原因，不 panic——协议边界必须能被远端错误文本表达。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COST_CORRECTION_COMMAND_REQUEST_SCHEMA {
            return Err("cost_correction_command_request_schema_invalid".to_owned());
        }
        self.command.validate()
    }
}

/// Server response shape for a committed or replayed correction. The receipt/digest is a
/// projection of EventLog facts; it does not claim an external invoice was paid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  成本更正的响应投影。
///  【关键】 它**不**声称外部发票已支付——所有字段都是 EventLog 事实的投影（command_id / correction_id / 两个 digest / source_cursor / replayed）。
///  `replayed` 区分"这次真的追加了"和"幂等重放命中了已有记录"，调用方不能靠 status 猜。
///
pub struct CostCorrectionCommandResponse {
    pub schema: String,
    pub command_id: RequestId,
    pub correction_id: kiana_domain::CostCorrectionId,
    pub correction_digest: String,
    pub target_entry_digest: String,
    pub source_cursor: u64,
    pub replayed: bool,
}

impl CostCorrectionCommandResponse {
    ///      【核心流程】 1) 三个 ID/游标的非空与非零检查，其中 `source_cursor == 0` 视为非法；2) 两个 digest 都要有 `sha256:` 前缀、去掉前缀后正好 64 个 ASCII 十六进制字符。
    ///      [为什么自己解析 digest 而不调 `json_digest` 比较] 这里是验证**形状**（前缀+长度+字符集），不是验证内容——内容一致性由生成方保证。
    /// 【⚠ 64 是 SHA-256 输出字节数的十六进制表示；换成别的哈希算法时这两处长度判断必须一起改，否则合法 digest 会被判非法。】
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COST_CORRECTION_COMMAND_RESPONSE_SCHEMA
            || self.command_id.as_uuid().is_nil()
            || self.correction_id.as_uuid().is_nil()
            || self.source_cursor == 0
        {
            return Err("cost_correction_command_response_invalid".to_owned());
        }
        for (field, value) in [
            ("correction_digest", &self.correction_digest),
            ("target_entry_digest", &self.target_entry_digest),
        ] {
            let Some(hex) = value.strip_prefix("sha256:") else {
                return Err(format!("cost_correction_{field}_invalid"));
            };
            if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("cost_correction_{field}_invalid"));
            }
        }
        Ok(())
    }
}

/// Read-only provider credential probe request.  The request carries only a secret reference;
/// raw credential material is resolved by the provider boundary, never by the wire protocol.
pub const PROVIDER_CREDENTIAL_PROBE_REQUEST_SCHEMA: &str =
    "kiana.provider-credential-probe-request.v1";
/// Read-only provider credential probe response.  The response is a bounded status projection and
/// intentionally contains digests/generations rather than token or key values.
pub const PROVIDER_CREDENTIAL_PROBE_RESPONSE_SCHEMA: &str =
    "kiana.provider-credential-probe-response.v1";
/// Protocol projection for the server-owned provider.use decision.
pub const PROVIDER_USE_POLICY_VIEW_SCHEMA: &str = "kiana.provider-use-policy-view.v1";

///  供应商凭据探测（provider credential probe）三个 DTO 共用的边界检查。
///  【核心流程】 三条：非空（trim 后）、长度不超上限、不含 `\0` / `\r` / `\n`。
///  【为什么禁控制字符】 这些字符串会进事件、日志和 UI；换行能伪造多行输出，NUL 能截断 C 侧字符串。
/// 【⚠ 上限全部在调用点以字面量给出（128 / 256），仓库里没有推导依据，不要假装它们来自某个统一预算。】
///
fn provider_probe_bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
}

///  "是一个 `sha256:` 开头的合法摘要"这一形状判断。与 `CostCorrectionCommandResponse::validate` 里内联的那段逻辑同形。
/// 【⚠ 与上面那条一样只验形状不验内容。真正绑定的位置是 `ProviderCredentialProbeResponse::validate` 里的 `self.response_digest != self.digest()`。】
///
fn provider_probe_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

///  `reason` 字段的字符集白名单：ASCII 字母数字加 `_` `-` `.`，并且要过本文件里 `provider_probe_bounded` 的通用边界检查。
///  【设计意图】 reason 是要进 UI 和事件的机器可读短码，刻意限制成 token 形式，不允许自由句子——避免调用方把诊断文本塞进 reason 造成注入或不可解析的下游分支。
///
fn provider_probe_reason(value: &str) -> bool {
    provider_probe_bounded(value, 256)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// A request to inspect provider credential readiness without performing a provider side effect.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  只读凭据探测请求。请求里只带 `SecretRef`（秘密引用，指纹库里的句柄），**绝不带原始 token/key**。
///  【设计意图】 探测动作本身没有 provider 侧副作用，它的用途是回答"凭据现在可用吗、过期了吗、覆盖哪些 scope"。
///  `expected_policy_revision` 让调用方能在探测时断言策略版本，避免拿着过期结论做决定。
///
pub struct ProviderCredentialProbeRequest {
    pub schema: String,
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<SecretRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requested_scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_policy_revision: Option<String>,
}

impl ProviderCredentialProbeRequest {
    pub fn new(
        provider_id: impl Into<String>,
        credential_ref: Option<SecretRef>,
        requested_scopes: Vec<String>,
        expected_policy_revision: Option<String>,
    ) -> Result<Self, String> {
        let requested_scopes = canonical_scopes(requested_scopes)?;
        let request = Self {
            schema: PROVIDER_CREDENTIAL_PROBE_REQUEST_SCHEMA.to_owned(),
            provider_id: provider_id.into(),
            credential_ref,
            requested_scopes,
            expected_policy_revision,
        };
        request.validate()?;
        Ok(request)
    }

    ///      【核心流程】 1) schema 必须是本协议常量；2) `provider_id` 走 128 字节边界检查；3) `requested_scopes` 必须是**已规范化**形式——把原值再规范化一次，结果必须与原值逐字节相等，不相等即拒。
    ///      【为什么要求已规范化】 `canonical_scopes` 会排序去重且拒绝重复项。如果允许未规范化的 scope 进 wire，摘要/比对就会对同一组 scope 得出不同字符串，缓存键和授权判定会不一致。
    ///      【设计意图】 这是一个"规范化不变量"检查，不是"格式检查"——它防的是等价但不相等的表示。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CREDENTIAL_PROBE_REQUEST_SCHEMA
            || !provider_probe_bounded(&self.provider_id, 128)
            || canonical_scopes(self.requested_scopes.clone()).as_ref()
                != Ok(&self.requested_scopes)
        {
            return Err("provider_credential_probe_request_invalid".to_owned());
        }
        if let Some(reference) = &self.credential_ref {
            reference.validate()?;
        }
        if self
            .expected_policy_revision
            .as_deref()
            .is_some_and(|revision| !provider_probe_digest(revision))
        {
            return Err("provider_credential_probe_policy_revision_invalid".to_owned());
        }
        Ok(())
    }
}

/// Bounded, non-authorizing result of a credential probe.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  凭据探测响应。字段全是状态投影和摘要（digest），没有 token/key 值。
///  【设计意图】 `credential_ref_digest` 是"引用本身的指纹"，不是凭据的指纹——它回答"我们查的是不是同一份记录"，且泄露它不会泄露凭据。
/// 【⚠ `credential_generation` 和 `expires_at_unix_ms` 都是 `Option<u64>` 且 0 被判非法（见 validate）。0 在这里代表"没填"而不是"1970 年"，两种语义混用一个整数会出错。】
///
pub struct ProviderCredentialProbeResponse {
    pub schema: String,
    pub provider_id: String,
    pub status: CredentialDisplayStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub granted_scopes: Vec<String>,
    pub policy_revision: String,
    pub reason: String,
    pub checked_at_unix_ms: u64,
    pub response_digest: String,
}

impl ProviderCredentialProbeResponse {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: impl Into<String>,
        status: CredentialDisplayStatus,
        credential_ref: Option<&SecretRef>,
        credential_generation: Option<u64>,
        expires_at_unix_ms: Option<u64>,
        granted_scopes: Vec<String>,
        policy_revision: impl Into<String>,
        reason: impl Into<String>,
        checked_at_unix_ms: u64,
    ) -> Result<Self, String> {
        if let Some(reference) = credential_ref {
            reference.validate()?;
        }
        let granted_scopes = canonical_scopes(granted_scopes)?;
        let mut response = Self {
            schema: PROVIDER_CREDENTIAL_PROBE_RESPONSE_SCHEMA.to_owned(),
            provider_id: provider_id.into(),
            status,
            credential_ref_digest: credential_ref
                .map(|reference| reference.reference_digest.clone()),
            credential_generation,
            expires_at_unix_ms,
            granted_scopes,
            policy_revision: policy_revision.into(),
            reason: reason.into(),
            checked_at_unix_ms,
            response_digest: String::new(),
        };
        response.response_digest = response.digest();
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CREDENTIAL_PROBE_RESPONSE_SCHEMA
            || !provider_probe_bounded(&self.provider_id, 128)
            || canonical_scopes(self.granted_scopes.clone()).as_ref() != Ok(&self.granted_scopes)
            || !provider_probe_digest(&self.policy_revision)
            || !provider_probe_reason(&self.reason)
            || self.checked_at_unix_ms == 0
            || !provider_probe_digest(&self.response_digest)
            || self.response_digest != self.digest()
        {
            return Err("provider_credential_probe_response_invalid".to_owned());
        }
        if let Some(reference_digest) = &self.credential_ref_digest {
            if !provider_probe_digest(reference_digest) {
                return Err("provider_credential_probe_ref_digest_invalid".to_owned());
            }
        }
        if self
            .credential_generation
            .is_some_and(|generation| generation == 0)
        {
            return Err("provider_credential_probe_generation_invalid".to_owned());
        }
        if self.expires_at_unix_ms.is_some_and(|expires| expires == 0) {
            return Err("provider_credential_probe_expiry_invalid".to_owned());
        }
        Ok(())
    }

    ///      计算响应摘要。**故意不包含 `response_digest` 自身**——否则自引用无法收敛。
    ///      【核心流程】 先把除 digest 外的全部字段塞进 `json!` 交给 `json_digest`，后者按键名排序后规范化序列化，保证同一逻辑内容在任何 map 顺序下都得到同一串。
    ///      【为什么 load-bearing】 `validate()` 里的 `self.response_digest != self.digest()` 是这个 DTO 的完整性自检：任何字段被改过而 digest 没跟着改，探测结果就不可信。
    /// 【⚠ 往这个 struct 加字段时必须同步加进 `digest()`，否则新字段可以不带摘要地通过校验——这是个静默的完整性漏洞。】
    ///
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "provider_id": self.provider_id,
            "status": self.status,
            "credential_ref_digest": self.credential_ref_digest,
            "credential_generation": self.credential_generation,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "granted_scopes": self.granted_scopes,
            "policy_revision": self.policy_revision,
            "reason": self.reason,
            "checked_at_unix_ms": self.checked_at_unix_ms,
        }))
    }
}

/// Wire-level copy of the provider.use decision.  It carries no policy internals beyond the
/// matched rule identifier and revision needed to explain a read-only diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
///  wire 层对 `provider.use` 决策的复述，只有 Allow / Deny 两个值。
///  【设计意图】 刻意做成两值枚举：它表达的是"匹配到的结果"，不是"策略本身"。策略内部、规则全文、评测细节一律不进 wire——诊断只需要知道命中了哪条规则的标识和当前 revision。
///
pub enum ProviderUsePolicyEffect {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  只读诊断用的策略命中投影。
///  【关键】 这是 Allow/Deny 的**解释**，不是授权凭据。拿到它不等于拿到许可；执行仍要重新走 provider 边界的策略评估。
///  `matched_rule` 可选：无匹配时为 None，调用方不能把"无规则标识"读成"规则未生效"。
///
pub struct ProviderUsePolicyView {
    pub schema: String,
    pub provider_id: String,
    pub operation: String,
    pub effect: ProviderUsePolicyEffect,
    pub credential_status: CredentialDisplayStatus,
    pub reason: String,
    pub policy_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<String>,
}

impl ProviderUsePolicyView {
    pub fn new(
        provider_id: impl Into<String>,
        operation: impl Into<String>,
        effect: ProviderUsePolicyEffect,
        credential_status: CredentialDisplayStatus,
        reason: impl Into<String>,
        policy_revision: impl Into<String>,
        matched_rule: Option<String>,
    ) -> Result<Self, String> {
        let view = Self {
            schema: PROVIDER_USE_POLICY_VIEW_SCHEMA.to_owned(),
            provider_id: provider_id.into(),
            operation: operation.into(),
            effect,
            credential_status,
            reason: reason.into(),
            policy_revision: policy_revision.into(),
            matched_rule,
        };
        view.validate()?;
        Ok(view)
    }

    ///      fail-closed 的形状校验。`reason` 走本文件里 `provider_probe_reason` 的严格字符集白名单（不是宽松的 bounded 检查），`policy_revision` 必须是摘要形状——两者含义不同：前者会进 UI，后者必须能唯一标识策略版本。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_USE_POLICY_VIEW_SCHEMA
            || !provider_probe_bounded(&self.provider_id, 128)
            || !provider_probe_bounded(&self.operation, 128)
            || !provider_probe_reason(&self.reason)
            || !provider_probe_digest(&self.policy_revision)
            || self
                .matched_rule
                .as_deref()
                .is_some_and(|rule| !provider_probe_bounded(rule, 256))
        {
            return Err("provider_use_policy_view_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
///  恢复（resume）一个暂停 run 的请求。
///  【设计意图】 resume 与 `run`/`continue` 分开：它是**从事件账本重建**，不是新开一轮。Core 侧要重放账本，因此这里的 `run_id` 可选（由 Core 解析当前 run）。
///
pub struct ResumeRequest {
    #[serde(default)]
    pub run_id: Option<RunId>,
}

///  列出待处理审批的请求。
///  【设计意图】 单独一个 body 变体而不是复用通用命令，是为了让它在 daemon 里被归入只读分支（`request_may_execute` 返回 false），并且在所有 surface 上返回**同一份**待批列表——这是跨入口一致性（parity）检查的一项。
///
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApprovalListRequest {
    #[serde(default)]
    pub run_id: Option<RunId>,
}

impl RequestEnvelope {
    pub fn resume_run(metadata: RequestMetadata, run_id: Option<RunId>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Resume(ResumeRequest { run_id }),
        }
    }

    pub fn pending_approvals(metadata: RequestMetadata, run_id: Option<RunId>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::ListApprovals(ApprovalListRequest { run_id }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 审批决定和可选的绑定证明。
///  审批决定及其绑定证明。
///  【核心流程】 `request_hash` + `nonce` 是 challenge 证明：它们把这次决定绑到 daemon 之前发出的具体挑战材料上。缺任一 → `approval_proof_required`。
///  【⚠】 这两个字段本身不是"审批权"。它们只防重放和串改，真正的授权判定在 `kiana-core::decide_approval_with_proof_and_version`。测试 `approval_decision_only_carries_the_daemon_challenge_and_decision` 断言不带证明时 envelope 里**不存在** `arguments` 字段——审批通道不允许夹带任意载荷。
///  `expected_version` 是乐观并发断言（见本文件里的 `approval_decision_with_proof_and_version` 构造器）。
///
pub struct ApprovalDecisionRequest {
    /// 被决定的审批 ID。
    pub approval_id: ApprovalId,
    /// approve 或 deny。
    pub decision: ApprovalDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Optional aggregate version observed when the approval was listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 新建 harness run 的输入。
///  新建 run 的输入。
///  【设计意图】 只有 prompt、history、sandbox 三样。**没有 tools 字段**——模型可见的工具面是服务端固定的，客户端不能通过 envelope 扩工具。测试 `run_envelope_round_trips_prompt_without_capability_payload` 专门断言 `body.request.tools` 不存在。
///  `history` 和 `sandbox` 都是可选的：老客户端不带它们时分别退化为空历史和 daemon 默认档位。
///
pub struct RunRequest {
    /// 当前提示词。
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// 可选结构化历史消息。
    pub history: Vec<kiana_domain::ConversationMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 请求的沙箱档位。
    pub sandbox: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 继续已有 run 的输入。
///  继续已有 run 的输入（v1 语义）。
///  【关键】 它**没有** `expected_turn_id` 这类并发断言，也不要求前一轮已终态——这是它和 `run.turn.v2` 路径的本质区别。想拿到 v2 的检查请用 `new_turn`。
/// 【⚠ daemon 会用 `sandbox` 覆盖 metadata 声明的权限档位（`effective_permission_profile` 对 Run/Continue/Spawn/Symposium 一律走 `permission_profile_for_sandbox`，非 `workspace-write` 一律落到 `Safe`）。】
///
pub struct ContinueRequest {
    /// 新增提示词。
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  转向指令请求。`deny_unknown_fields` + 必填 `expected_turn_id` 是这里的核心：多一个字段就是多一条能绕过轮次检查的路径。
///  【说明】 三个字段全部必填，没有 `#【serde(default)】`——这是本文件里少数几个不容忍缺字段的结构体。
///
pub struct SteerRequest {
    pub run_id: RunId,
    pub expected_turn_id: TurnId,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  上下文注入请求。`source` 必填（Core 校验长度 256 且禁 NUL），`target_turn_id` 可选。
///  【设计意图】 与 `SteerRequest` 的区别在于不做轮次过期检查——注入是"排队等未来某个安全点"，不是"插进当前轮"。
///
pub struct InjectRequest {
    pub run_id: RunId,
    pub target: String,
    pub source: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_turn_id: Option<TurnId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 取消 run 的输入。
///  取消请求。`reason` 带 `#【serde(default)】` 仅为反序列化兼容，但语义上必填——它会进 `run.cancel_requested` 事件。
///
pub struct CancelRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(default)]
    /// 取消原因，供事件/receipt 记录。
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// receipt 查询参数。
///  receipt 查询参数。`run_id` 可选表示"当前 run"。
///  【设计意图】 receipt 是只读事实投影；daemon 强制 `Safe` 档位且 `request_may_execute` 返回 false。
///
pub struct ReceiptRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  有界审计查询。
///  【设计意图】 双游标设计：`after_cursor` 是"我从哪之后读"，`cursor` 是服务端签发的续读令牌（含 `source_cursor`）。后者让翻页和快照保持一致——源账本还在增长时，翻出来的多页不会串。
///  `deny_unknown_fields` 保证过滤条件拼错时直接失败，而不是被当成"没过滤"从而多导出数据。
///
pub struct AuditQueryRequest {
    #[serde(default)]
    pub source_cursor: Option<kiana_domain::EventCursor>,
    #[serde(default)]
    pub after_cursor: kiana_domain::EventCursor,
    pub limit: usize,
    #[serde(default)]
    pub action_kind: Option<AuditActionKind>,
    #[serde(default)]
    pub decision: Option<AuditDecision>,
    #[serde(default)]
    pub target_kind: Option<String>,
    #[serde(default)]
    pub cursor: Option<AuditQueryCursor>,
}

impl AuditQueryRequest {
    ///      【核心流程】 1) `limit` 必须在 1..=1000；2) `source_cursor` 若给出则不能是 0，且 `after_cursor` 不得越过它；3) `target_kind` 过滤值非空且 ≤128 字节；4) 若带 `cursor`，它自身要通过校验，且必须与显式给出的 `source_cursor` / `after_cursor` **完全一致**。
    ///      【为什么第 4 条要交叉校验】 令牌和显式游标并存时，两者不一致意味着调用方在拼凑一个"从没被服务端签发过的翻页状态"。只信令牌会忽略显式值，只信显式值就等于让客户端自己编翻页位置——所以冲突一律拒。
    ///      【⚠】 `1_000` 这个上限在 `kiana-core/src/audit_projection.rs` 里有一份同名常量 `MAX_AUDIT_QUERY_LIMIT`，值相同。两处必须同步；仓库里没有记录这个数字的来源理由。
    ///      【调用者】 `kiana-daemon::handle` 在构造 `RequestContext` 之前就调它，失败即回 `audit_query_invalid`。
    ///
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.limit == 0 || self.limit > 1_000 {
            return Err("audit_query_limit_invalid");
        }
        if self.source_cursor == Some(0)
            || self.after_cursor > self.source_cursor.unwrap_or(u64::MAX)
        {
            return Err("audit_query_cursor_invalid");
        }
        if self
            .target_kind
            .as_deref()
            .is_some_and(|target| target.trim().is_empty() || target.len() > 128)
        {
            return Err("audit_query_filter_invalid");
        }
        if let Some(cursor) = &self.cursor {
            cursor
                .validate()
                .map_err(|_| "audit_query_cursor_invalid")?;
            if self
                .source_cursor
                .is_some_and(|value| value != cursor.source_cursor)
                || (self.after_cursor != 0 && self.after_cursor != cursor.after_cursor)
            {
                return Err("audit_query_cursor_invalid");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  审计查询响应。
///  【关键】 `limitations` 字段让服务端能显式声明"这份投影不完整"。这是刻意设计的：宁可让调用方知道有缺口，也不要给出一个看起来完整、其实是被投影上限截断过的快照。
///  `projection_version` 让调用方判断投影格式是否变化，而不必猜。
///
pub struct AuditQueryResponse {
    pub schema: String,
    pub records: Vec<AuditRecord>,
    #[serde(default)]
    pub next_cursor: Option<AuditQueryCursor>,
    pub source_cursor: kiana_domain::EventCursor,
    pub projection_version: u64,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  审计导出请求。`purpose` / `recipient` / `retention_class` 三个都是必填字符串——导出是"把数据交给某个收件人"，这三项决定保留多久、给谁看。
///  `deliver` 默认 false：先生成 manifest（清单）再决定是否投递，两步分离。
///
pub struct AuditExportRequest {
    pub query: AuditQueryRequest,
    pub format: AuditExportFormat,
    pub purpose: String,
    pub recipient: String,
    pub retention_class: String,
    #[serde(default)]
    pub deliver: bool,
}

impl AuditExportRequest {
    ///      【核心流程】 先透传 `self.query.validate()`，再逐项检查三个必填字符串非空且不超长（256 / 256 / 64）。
    ///      【⚠】 三个上限在本文件里是字面量，仓库里查不到推导依据。
    ///      注意它**不**校验 `format` 和 `deliver`——这两项由各自类型和后续投递逻辑负责。
    ///
    pub fn validate(&self) -> Result<(), &'static str> {
        self.query.validate()?;
        for (value, field, max) in [
            (&self.purpose, "audit_export_purpose", 256),
            (&self.recipient, "audit_export_recipient", 256),
            (&self.retention_class, "audit_export_retention_class", 64),
        ] {
            if value.trim().is_empty() || value.len() > max {
                return Err(field);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  审计导出响应。`content` 是一段已脱敏（redacted）的文本，`manifest` 记录这次导出的范围和证据。
///  【设计意图】 `delivery` 与内容分开放：投递回执（谁在什么时候把哪份 manifest 发出去了）是独立事实，不该靠解析 `content` 推断。
///
pub struct AuditExportResponse {
    pub schema: String,
    pub manifest: kiana_domain::AuditExportManifest,
    pub content: String,
    #[serde(default)]
    pub delivery: Option<kiana_domain::AuditDeliveryReceipt>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  跨入口一致性（parity）查询。`entrypoint` 是必填的——问的是"某个入口看到的状态"，不是"所有入口"。
///  【调用者】 daemon 对 `RequestBody::Parity` 有专门的早退检查：`metadata.actor_id` 必须等于 principal，否则回 `parity_unauthenticated`。
///
pub struct ParityRequest {
    pub entrypoint: EntryPointKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  parity 响应，只回一个 `EntryPointParitySnapshot`。
///  【设计意图】 单结构体而非多段字段：parity 的意义就是"把各入口折叠成一个可比对的快照"，返回散装字段反而丢掉可比性。
///
pub struct ParityResponse {
    pub schema: String,
    pub snapshot: EntryPointParitySnapshot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// WorkPacket spawn 参数。
///  WorkPacket spawn 参数。结构体里**没有** prompt 字段——理由同本文件里的 `RequestEnvelope::spawn` 构造器：packet 是唯一事实来源。
///
pub struct SpawnRequest {
    pub packet: WorkPacket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
}

///  缺省轮次上限指向 `Symposium::DEFAULT_MAX_ROUNDS`。
///  【设计意图】 放在 domain 类型的常量上而不是在本文件另写一个数字，是为了让"上限是多少"只有一个真值。改上限时不会漏掉 wire 默认值。
///
fn default_symposium_max_rounds() -> u32 {
    Symposium::DEFAULT_MAX_ROUNDS
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Symposium 参数。
///  研讨会参数。`anti_meeting` 和 `max_rounds` 都有 serde 缺省值，缺省路径是"关掉会议模式、轮次按 domain 常量"——即最保守的一档。
/// 【⚠ 这里的缺省是"最小权限"方向的缺省，不是"最方便"方向的。】
///
pub struct SymposiumRequest {
    pub goal: String,
    #[serde(default)]
    pub anti_meeting: bool,
    #[serde(default = "default_symposium_max_rounds")]
    pub max_rounds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Review 参数。
///  评审参数。只引用作者，不带 transcript——评审要自己从账本读事实。
///
pub struct ReviewRequest {
    pub author_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Close 参数。
///  收尾参数，与 `ReviewRequest` 对称。
///
pub struct CloseRequest {
    pub author_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// daemon 对请求的统一响应 envelope。
///  统一的响应 envelope，**所有** surface（CLI / workbench / web / SDK）都拿它。
///  【设计意图】 单一形状是跨入口一致性检查能成立的前提——各入口读的是同一个 `status` / `output` / `error` 三元组。
///  【关键】 `error: Option<String>` 的存在本身是契约：非空时不能把响应当作成功，调用方必须先看它。`output` 是 `Value` 而不是定型结构，是因为各命令的输出形状差异太大，强定型会让每加一条命令都改这个 envelope。
///  【⚠】 本结构体没有 `deny_unknown_fields`——响应方向选择容忍未知字段，方便加输出而不破坏老客户端。
///
pub struct ResponseEnvelope {
    /// 响应使用的协议 schema。
    pub schema: String,
    /// 对应请求 ID。
    pub request_id: RequestId,
    /// 执行生命周期状态。
    pub status: ExecutionStatus,
    /// 结构化输出或空值。
    pub output: Value,
    /// 可读错误原因；非空时不能把响应当作成功。
    pub error: Option<String>,
}

impl ResponseEnvelope {
    /// Stable failure classification shared by CLI and HTTP adapters.
    ///
    /// Lifecycle remains authoritative: an unknown result always requires
    /// reconciliation, even when its diagnostic text resembles another error.
    /// A response with an error is never classified as successful.
    ///      把（状态，错误文本）归一成稳定的机器可读错误码。这是全仓错误契约的收敛点。
    ///      【核心流程】 生命周期状态优先于错误文本：`ResultUnknown` / `Cancelled` / `AwaitingApproval` 先直接映射，不看 `error` 字符串。剩下的才去查 `error`，且用的是 `CapabilityErrorCode::from_reason` 的**结构化前缀匹配**（按 `:` 逐段剥 `port_failed` 一类前缀再匹配已知码表），不是子串搜索。
    ///      【⚠ load-bearing】 顺序是刻意的：一条 `ResultUnknown` 的响应，即使诊断文本看起来像别的错误，也必须归类为 `ResultUnknown`——因为未知结果意味着"不知道副作用有没有发生"，必须先对账。反过来先看文本就会把它误判成可重试的普通失败。
    ///      【调用者】 CLI、HTTP 适配器与 `kiana-client` 共用；`ui_contracts::stable_error_from_response` 也基于它映射到 UI 错误码（那里把 `ResultUnknown` 映射成 `UiErrorCode::Unknown` + `QueryOriginal` 重试倾向）。
    ///      【守卫】 `kiana-core/tests/p0_a02_error_codes_guard.rs` 断言本文件必须包含 `failure_policy`，`cap04_state_guard.rs` 断言核心状态判定不得改用 `result_unknown` 子串匹配。删改本方法会直接打破这些守卫。
    ///
    pub fn failure_code(&self) -> Option<CapabilityErrorCode> {
        match self.status {
            ExecutionStatus::ResultUnknown => Some(CapabilityErrorCode::ResultUnknown),
            ExecutionStatus::Cancelled => Some(CapabilityErrorCode::Cancelled),
            ExecutionStatus::AwaitingApproval => Some(CapabilityErrorCode::ApprovalRequired),
            _ => self
                .error
                .as_deref()
                .map(CapabilityErrorCode::from_reason)
                .or_else(|| match self.status {
                    ExecutionStatus::Blocked | ExecutionStatus::Denied => {
                        Some(CapabilityErrorCode::PermissionDenied)
                    }
                    ExecutionStatus::Failed => Some(CapabilityErrorCode::ExecutionFailed),
                    _ => None,
                }),
        }
    }

    /// Uniform exit/status/recovery mapping, without authorizing an attempt.
    ///      一次性拿到错误码对应的策略（CLI 退出码、HTTP 状态码、是否可重试、是否需要新的授权）。
    ///      【为什么在这里而不是各入口自己映射】 CLI 退出码和 HTTP 状态码如果由两端各写一份映射表，必然漂移。走 `CapabilityErrorCode::policy()` 这一个真值，测试 `cap04_mapping.rs` 就是钉这个的：`ExecutionFailed` → exit 1 / HTTP 500 / 不可重试 / 需重新授权。
    ///      【注意】 它只做分类，**不**授权任何重试。是否真的重发由调用方按 `requires_reconciliation` 决定。
    ///
    pub fn failure_policy(&self) -> Option<CapabilityErrorPolicy> {
        self.failure_code().map(CapabilityErrorCode::policy)
    }

    /// 将 core 的内部响应投影为 wire 响应。
    ///      把 ControlPlane 的内部响应（`CoreResponse`）投影到 wire 形状。
    ///      【设计意图】 边界在此处收口：Core 内部可以随意增字段，但只要映射到这五个字段，协议形状就不变。schema 在这里强制回 `PROTOCOL_SCHEMA`，调用方无法让 Core 产出别的 schema 名。
    ///
    pub fn from_core(response: CoreResponse) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: response.request_id,
            status: response.status,
            output: response.output,
            error: response.error,
        }
    }

    /// 构造统一 blocked 响应，不执行任何副作用。
    ///      统一的拒绝响应，**不做任何副作用**。
    ///      【调用者】 daemon 里有 30 多处早退走它：schema 不支持、身份不匹配、审计未认证、role 未知、trust 不可用、审批证明缺失……
    ///      【设计意图】 它固定产出 `status: Blocked` + `output: Value::Null` + 非空 `error`。因为 `failure_code()` 会把 `Blocked` 归为 `PermissionDenied`，所以任何早退都自动带着正确的策略（HTTP 403 之类），而不会退化成普通失败。
    ///      【⚠】 新增一条 daemon 早退检查时务必用它，别手搓 `ResponseEnvelope { .. }` 字面量——手搓的很容易忘了设 `status` 或 `error`，那样 `failure_code()` 会返回 `None`，策略就漏了。
    ///
    pub fn rejected(request_id: RequestId, reason: impl Into<String>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id,
            status: ExecutionStatus::Blocked,
            output: Value::Null,
            error: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// additive 的运行中事件 envelope；不改变请求/响应 envelope 的 required 字段。
///
/// 该通道只投影展示所需的增量，不能替代 EventLog 或 Receipt。老客户端可以继续只读取
/// [`ResponseEnvelope::output`]；新事件类型和未知事件不会改变原有响应语义。
///  运行中事件 envelope。**additive**：它不改变请求/响应 envelope 的任何必填字段。
///  【设计意图】 单开这个类型而不是塞进 `ResponseEnvelope` 的变体，是为了让"最终事实"和"展示增量"在类型上就分家。老客户端继续只读 `ResponseEnvelope` 也不受影响。
///  【⚠】 这里的 schema 字符串**故意复用** `kiana.protocol.v1`，不给事件通道单独版本号。一条流里出现两种 schema 名会让订阅端无法用单一判据过滤。
///  【字段】 `epoch` / `sequence` / `ui_cursor` 三者都是 `#【serde(default)】`，因此老事件（无游标）仍能反序列化，但代价是 `sequence == 0 && epoch 为空` 的组合必须被下游当作 legacy 特判（见 `advance_cursor`）。
///
pub struct RunStreamEnvelope {
    /// 事件使用的协议 schema；与请求/响应保持同一个 `kiana.protocol.v1`。
    pub schema: String,
    /// Opaque daemon incarnation. A changed epoch requires snapshot hydration.
    #[serde(default)]
    pub epoch: String,
    /// Monotonic per-run display cursor within an epoch; zero is a legacy envelope.
    #[serde(default)]
    pub sequence: u64,
    /// UI revision at publication, used to fence an older in-flight snapshot.
    #[serde(default)]
    pub ui_cursor: u64,
    /// 运行中事件。
    pub event: RunStreamEvent,
}

impl RunStreamEnvelope {
    /// 构造一个使用当前协议 schema 的运行中事件。
    pub fn new(event: RunStreamEvent) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            epoch: String::new(),
            sequence: 0,
            ui_cursor: 0,
            event,
        }
    }

    /// Reject changed incarnations and gaps; repeated deliveries are harmless.
    ///      推进订阅端游标，并回答"这条事件是不是新的"。
    ///      【核心流程】 1) `sequence == 0 && epoch` 空 → legacy envelope，无条件接受（返回 true），不触碰游标；2) 两者只有一个有值 → `stream_cursor_invalid`（半截游标无法比较）；3) 游标已有 epoch 且与本事件不同 → `stream_epoch_changed`；4) 同 epoch 且 `sequence <= cursor.sequence` → 重复投递，返回 `Ok(false)` 而**不是**错误；5) 有前驱但 `sequence` 不连续 → `stream_sequence_gap`。
    ///      [为什么重复是 `Ok(false)` 而 gap 是 `Err`] 重复投递在多订阅者/重连场景下是正常的，必须静默去重；序号缺口意味着中间有事实没被看到，这是要上报的完整性问题。
    ///      【⚠】 这里**没有**让 `Terminal` 事件豁免 gap 检查（`RunDisplayState::apply` 里对 `Terminal` 事件有豁免）。两条路径规则不同，不要以为可以互换。
    ///      【调用者】 CLI 的三个渲染器（`stream_render` / `workbench_render` / `workbench_chat`）都走它；`kiana-entrypoints/tests/h32_display_guard.rs` 断言 `advance_cursor`、`stream_epoch_changed`、`stream_sequence_gap` 三个字符串标记必须留在本文件。
    ///
    pub fn advance_cursor(&self, cursor: &mut UiCursor) -> Result<bool, &'static str> {
        if self.sequence == 0 && self.epoch.is_empty() {
            return Ok(true);
        }
        if self.sequence == 0 || self.epoch.is_empty() {
            return Err("stream_cursor_invalid");
        }
        if !cursor.epoch.is_empty() && cursor.epoch != self.epoch {
            return Err("stream_epoch_changed");
        }
        if cursor.epoch == self.epoch && self.sequence <= cursor.sequence {
            return Ok(false);
        }
        if cursor.sequence != 0 && self.sequence != cursor.sequence.saturating_add(1) {
            return Err("stream_sequence_gap");
        }
        *cursor = UiCursor {
            epoch: self.epoch.clone(),
            sequence: self.sequence,
        };
        Ok(true)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// additive 的运行中事件类型。
///
/// `delta` 是易失的展示投影；`terminal` 携带最终响应，调用方仍必须从其中的 receipt 读取
/// 事实，不能把增量文本当作执行事实。未知类型反序列化为 [`Self::Unknown`]，供新老客户端
/// 在协议演进时安全忽略。
///  运行中事件类型。`#【serde(other)】` 的 `Unknown` 变体是协议向前兼容的关键机制。
///  【设计意图】 老客户端遇到未来事件类型时反序列化成 `Unknown` 而不是整条流失败。这让"协议演进"和"客户端升级"解耦：新事件可以先上服务端。
///  【⚠ load-bearing】 但这个宽容只对**读取**成立。写入方向如果误用 `Unknown`，下游会认为什么都没发生。所以它必须永远只作为反序列化的兜底，不能被构造出来发送。
///  【语义分层】 `delta` 是易失的展示投影；`terminal` 携带的 `response` 才是最终事实。调用方不能把累计文本当执行事实——事实要从 receipt 读。
///  `ToolCall` 变体的注释已写明"never an instruction for the UI to execute it"：这是防止 UI 把工具调用当指令去执行的契约点。
///
pub enum RunStreamEvent {
    /// 模型文本增量。
    Delta {
        /// 对应 run ID。
        run_id: RunId,
        /// 本次新增文本。
        text: String,
    },
    /// 运行终态；`response` 是最终协议响应及 receipt 投影。
    Terminal {
        /// 对应 run ID。
        run_id: RunId,
        /// 最终响应 envelope。
        response: ResponseEnvelope,
    },
    /// A projection of an already committed usage record.
    Usage { run_id: RunId, data: Value },
    /// A committed tool request; never an instruction for the UI to execute it.
    ToolCall { run_id: RunId, data: Value },
    /// An approval recorded by ControlPlane. Decisions still require challenge proof.
    ApprovalRequested { run_id: RunId, data: Value },
    /// A recorded error; it does not imply the run has reached a terminal state.
    Error { run_id: RunId, data: Value },
    /// 当前客户端不认识的未来事件类型；必须忽略而不是拒绝整个通道。
    #[serde(other)]
    Unknown,
}

/// Version of the daemon's disposable UI projection, not an execution authority.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
///  daemon 的**一次性** UI 投影位置（epoch + sequence）。
///  【设计意图】 它是"可丢弃的"——丢了重新水合快照即可。它**不是**账本游标：账本事实要靠 `UiSnapshot` / receipt 重新读，UI 游标只是让界面能检测"我是不是漏看了什么"。
///  `Default` 给出空 epoch + 0 sequence，这是"还没有任何投影"的合法初值（见 `validate`）。
///
pub struct UiCursor {
    pub epoch: String,
    pub sequence: u64,
}

impl UiCursor {
    /// An empty epoch with a zero sequence is the "no projection yet" default and is accepted.
    /// Once either half is set, both must be set: a sequence without an epoch cannot be compared
    /// across daemon incarnations, and an epoch without a sequence carries no position.
    ///      【核心流程】 1) 两半都空/零 → 通过（初值）；2) 只填一半 → `ui_cursor_invalid`；3) epoch 超过 128 字节或含 `\0` / 换行 → `ui_cursor_invalid`。
    ///      【为什么"只填一半"必须拒绝】 没有 epoch 的 sequence 无法跨 daemon 重启比较；没有 sequence 的 epoch 不携带位置信息。半截游标会让上层以为自己在某个确定位置上，实际不是。
    ///      【⚠】 这个"两半同进同出"的不变量是 `RunStreamEnvelope` 三个字段全用 `#【serde(default)】` 换来的兼容性的代价——默认值让老包能读，但半填的组合必须在这里拦住。
    ///
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.epoch.is_empty() && self.sequence == 0 {
            return Ok(());
        }
        if self.sequence == 0 || self.epoch.is_empty() {
            return Err("ui_cursor_invalid");
        }
        if self.epoch.len() > MAX_UI_EPOCH_BYTES || self.epoch.contains(['\0', '\n', '\r']) {
            return Err("ui_cursor_invalid");
        }
        Ok(())
    }
}

/// Optimistic precondition for one explicit human action. Core still authorizes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
///  一次显式人工动作的乐观并发前置条件。
///  【核心流程】 `expected_epoch` + `expected_cursor` 组成 CAS 断言，`idempotency_key` 让重试不会重复执行。
///  【关键】 文档明说 "Core still authorizes it"：这三个字段是**防止基于陈旧界面做动作**，不是权限。谁点了、能不能点，仍然由 ControlPlane 结合当前身份和状态判定。
///  【调用者】 `kiana-daemon::claim_ui_action` 消费它并返回新的 `UiCursor`；`kiana-entrypoints/src/web.rs` 与 `kiana-daemon/src/run_stream.rs` 都在构造它。
///
pub struct UiAction {
    pub target_id: String,
    pub expected_epoch: String,
    pub expected_cursor: u64,
    pub idempotency_key: String,
}

/// Server-owned projection of one session's event facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
///  某个会话的服务端事件事实快照。
///  【关键】 它与 `stream_cursor` 是两回事：`cursor` 是事件账本位置，`stream_cursor` 是"独立的展示流"当前位置。混淆二者会导致"补齐了账本却以为展示也补齐了"。
///  【设计意图】 `pending_actions: Vec<Value>` 刻意不定型——它是"界面上还有哪些事等人做"的临时投影，不是可执行指令。
///
pub struct UiSnapshot {
    pub schema: String,
    pub cursor: UiCursor,
    pub session_id: String,
    pub run_id: Option<RunId>,
    /// Current tail of the independent display stream; it is not a ledger cursor.
    pub stream_cursor: Option<UiCursor>,
    pub status: Option<ExecutionStatus>,
    pub pending_actions: Vec<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
///  客户端侧的 run 展示状态机。`deny_unknown_fields` 意味着它是封闭的展示模型。
///  【设计意图】 把"如何把一串可能重复、可能缺口、可能属于旧 daemon 的事件投影成一块屏幕"这套规则集中在一个可单测的类型里，而不是散在三个渲染器里各自实现。
///  【⚠】 它是**展示状态**，不是执行事实。`status` 字段是 `String` 而非 `ExecutionStatus`，正是因为它的取值来自事件流投影，权威状态要看 `Terminal` 事件里那份 `ResponseEnvelope`。
///
pub struct RunDisplayState {
    pub schema: String,
    pub run_id: RunId,
    pub cursor: UiCursor,
    pub status: String,
    pub terminal: bool,
    pub text: String,
    pub gap_detected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_digest: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
///  `RunDisplayState::apply` 的结果。
///  【设计意图】 三值而不是 bool：`IgnoredDuplicate` 让调用方能区分"我忽略了"和"我应用了"，用于统计和遥测；`GapRequiresSnapshot` 是明确的行动指令（去水合快照），不是错误。
///
pub enum DisplayApply {
    Applied,
    IgnoredDuplicate,
    GapRequiresSnapshot,
}

impl RunDisplayState {
    pub fn new(run_id: RunId) -> Self {
        Self {
            schema: RUN_DISPLAY_STATE_SCHEMA.to_owned(),
            run_id,
            cursor: UiCursor::default(),
            status: "queued".to_owned(),
            terminal: false,
            text: String::new(),
            gap_detected: false,
            last_event_digest: None,
        }
    }

    pub fn hydrate(&mut self, snapshot: &UiSnapshot) -> Result<(), String> {
        if snapshot.run_id != Some(self.run_id) {
            return Err("display_snapshot_run_mismatch".to_owned());
        }
        if !snapshot.cursor.epoch.is_empty() {
            snapshot
                .cursor
                .validate()
                .map_err(|error| error.to_owned())?;
        }
        self.cursor = snapshot.cursor.clone();
        self.status = snapshot
            .status
            .map(|status| status.as_str().to_owned())
            .unwrap_or_else(|| "queued".to_owned());
        self.terminal = snapshot.status.is_some_and(ExecutionStatus::is_terminal);
        self.gap_detected = false;
        self.text.clear();
        self.last_event_digest = None;
        self.validate()
    }

    ///      把一条流事件投影进展示状态。
    ///      【核心流程】 1) legacy envelope（epoch 空 + sequence 零）直接应用；2) 半截游标 → `display_cursor_invalid`；3) 已终态 → 一律 `IgnoredDuplicate`（终态之后的一切都不再改屏幕）；4) epoch 变化 → `display_epoch_changed`；5) 同 epoch 序号不前进 → `IgnoredDuplicate`；6) 序号有缺口：若**不是** `Terminal` 事件则标记 `gap_detected` 并返回 `GapRequiresSnapshot`；`Terminal` 事件豁免缺口检查，因为终态响应的价值高于丢失的中间增量。
    ///      【⚠ load-bearing】 第 6 条的 Terminal 豁免是本文件里最容易被人"顺手修正"成一致逻辑的地方。修了之后，daemon 重启丢过增量时界面会永远停在"检测到缺口"而收不到最终状态。
    ///      【⚠】 第 3 条（终态后忽略一切）也是终态保护：晚到的 delta 不能把已经完成的 run 又刷成进行中。
    ///
    pub fn apply(&mut self, envelope: &RunStreamEnvelope) -> Result<DisplayApply, String> {
        if envelope.epoch.is_empty() && envelope.sequence == 0 {
            if self.terminal {
                return Ok(DisplayApply::IgnoredDuplicate);
            }
            self.apply_event(&envelope.event)?;
            return Ok(DisplayApply::Applied);
        }
        if envelope.epoch.is_empty() || envelope.sequence == 0 {
            return Err("display_cursor_invalid".to_owned());
        }
        if self.terminal {
            return Ok(DisplayApply::IgnoredDuplicate);
        }
        if !self.cursor.epoch.is_empty() && self.cursor.epoch != envelope.epoch {
            return Err("display_epoch_changed".to_owned());
        }
        if self.cursor.epoch == envelope.epoch && envelope.sequence <= self.cursor.sequence {
            return Ok(DisplayApply::IgnoredDuplicate);
        }
        let gap = self.cursor.sequence != 0
            && envelope.sequence != self.cursor.sequence.saturating_add(1);
        if gap && !matches!(envelope.event, RunStreamEvent::Terminal { .. }) {
            self.gap_detected = true;
            return Ok(DisplayApply::GapRequiresSnapshot);
        }
        self.apply_event(&envelope.event)?;
        self.cursor = UiCursor {
            epoch: envelope.epoch.clone(),
            sequence: envelope.sequence,
        };
        self.gap_detected |= gap;
        self.validate()?;
        Ok(DisplayApply::Applied)
    }

    ///      【核心流程】 schema 常量比对、`run_id` 非 nil、`status` 非空且 ≤64 字节、累积文本不超 128 KiB、游标合法、`last_event_digest` 形状检查。
    ///      【⚠】 `digest.len() != 71` 是硬编码的算术结果：`"sha256:"`（7 字符）+ 64 个十六进制 = 71。换哈希算法时这一行和 `provider_probe_digest` 必须一起改，否则合法的终态摘会被判非法、终态状态永远写不进。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RUN_DISPLAY_STATE_SCHEMA
            || self.run_id.as_uuid().is_nil()
            || self.status.trim().is_empty()
            || self.status.len() > 64
            || self.text.len() > MAX_RUN_DISPLAY_TEXT_BYTES
        {
            return Err("display_state_invalid".to_owned());
        }
        if !self.cursor.epoch.is_empty() {
            self.cursor.validate().map_err(|error| error.to_owned())?;
        }
        if let Some(digest) = &self.last_event_digest {
            if !digest.starts_with("sha256:") || digest.len() != 71 {
                return Err("display_event_digest_invalid".to_owned());
            }
        }
        Ok(())
    }

    ///      【核心流程】 每个变体先做**同一个**前置检查：`run_id` 必须等于本状态的 `run_id`，否则 `display_event_run_mismatch`。这是防止一条流里混入别的 run 的事件。
    ///      【⚠】 `Usage` / `ToolCall` 校验完 run_id 后**什么都不做**——它们是纯遥测投影，不进展示文本。让它们改屏幕等于让 UI 呈现变成事实来源。
    ///      `Terminal` 会把整个 `response` 的摘要存进 `last_event_digest`，这是"屏幕上的终态对应哪一份响应"的锚点。
    ///      【⚠】 `RunStreamEvent::Unknown` 分支是空实现且**必须**保持为空：它代表"这条事件我读不懂"，如果在这里猜测性地改状态，就等于让协议演进静默污染展示。
    ///
    fn apply_event(&mut self, event: &RunStreamEvent) -> Result<(), String> {
        match event {
            RunStreamEvent::Delta { run_id, text } => {
                if *run_id != self.run_id {
                    return Err("display_event_run_mismatch".to_owned());
                }
                if self.text.len().saturating_add(text.len()) > MAX_RUN_DISPLAY_TEXT_BYTES {
                    return Err("display_text_limit".to_owned());
                }
                self.text.push_str(text);
            }
            RunStreamEvent::Terminal { run_id, response } => {
                if *run_id != self.run_id {
                    return Err("display_event_run_mismatch".to_owned());
                }
                self.status = response.status.as_str().to_owned();
                self.terminal = response.status.is_terminal();
                self.last_event_digest = Some(json_digest(
                    &serde_json::to_value(response)
                        .map_err(|_| "display_terminal_encode_failed".to_owned())?,
                ));
            }
            RunStreamEvent::ApprovalRequested { run_id, .. } => {
                if *run_id != self.run_id {
                    return Err("display_event_run_mismatch".to_owned());
                }
                self.status = "awaiting_approval".to_owned();
            }
            RunStreamEvent::Error { run_id, data } => {
                if *run_id != self.run_id {
                    return Err("display_event_run_mismatch".to_owned());
                }
                self.status = data
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("runtime_error")
                    .to_owned();
            }
            RunStreamEvent::Usage { run_id, .. } | RunStreamEvent::ToolCall { run_id, .. } => {
                if *run_id != self.run_id {
                    return Err("display_event_run_mismatch".to_owned());
                }
            }
            RunStreamEvent::Unknown => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_envelope_round_trip_preserves_security_context_and_arguments() {
        let mut metadata = RequestMetadata::local("session-1", "/repo");
        metadata.project_trusted = true;
        metadata.permission_profile = PermissionProfile::Balanced;
        let request = RequestEnvelope::command(
            metadata,
            "system.architecture",
            serde_json::json!({ "format": "json" }),
        );
        let encoded = serde_json::to_vec(&request).unwrap();
        let decoded: RequestEnvelope = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, request);
        assert_eq!(decoded.schema, PROTOCOL_SCHEMA);
        assert_eq!(decoded.metadata.role_id, ROLE_BUILDER);
        assert_eq!(decoded.metadata.department_id, DEPARTMENT_EXECUTING);
    }

    #[test]
    fn missing_role_fields_default_to_executing_builder() {
        let json = serde_json::json!({
            "schema": PROTOCOL_SCHEMA,
            "metadata": {
                "request_id": RequestId::new(),
                "session_id": "session-1",
                "project_root": "/repo",
                "actor_id": "local-user",
                "project_trusted": true,
                "permission_profile": "safe"
            },
            "body": {
                "type": "run",
                "request": { "prompt": "hello" }
            }
        });
        let decoded: RequestEnvelope = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.metadata.role_id, ROLE_BUILDER);
        assert_eq!(decoded.metadata.department_id, DEPARTMENT_EXECUTING);
    }

    #[test]
    fn approval_decision_only_carries_the_daemon_challenge_and_decision() {
        let metadata = RequestMetadata::local("session-1", "/repo");
        let request = RequestEnvelope::approval_decision(
            metadata,
            ApprovalId::new(),
            ApprovalDecision::Approve,
        );
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "approval_decision");
        assert_eq!(encoded["body"]["request"]["decision"], "approve");
        assert!(encoded["body"]["request"].get("arguments").is_none());
        assert!(encoded["body"]["request"].get("request_hash").is_none());
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }

    #[test]
    fn approval_decision_proof_round_trips_when_present() {
        let request = RequestEnvelope::approval_decision_with_proof(
            RequestMetadata::local("session-1", "/repo"),
            ApprovalId::new(),
            ApprovalDecision::Approve,
            Some("sha256:abc".to_owned()),
            Some("nonce-1".to_owned()),
        );
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["request"]["request_hash"], "sha256:abc");
        assert_eq!(encoded["body"]["request"]["nonce"], "nonce-1");
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }

    #[test]
    fn close_envelope_round_trips_author_reference_without_transcript() {
        let mut metadata = RequestMetadata::local("closer-1", "/repo");
        metadata.assign_role(&RoleSpec::closer());
        let request = RequestEnvelope::close(metadata, "builder-1", Some(RunId::new()));
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "close");
        assert_eq!(encoded["body"]["request"]["author_session_id"], "builder-1");
        assert!(encoded["body"]["request"].get("transcript").is_none());
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }

    #[test]
    fn run_envelope_round_trips_prompt_without_capability_payload() {
        let mut metadata = RequestMetadata::local("session-1", "/repo");
        metadata.project_trusted = true;
        let request = RequestEnvelope::run(metadata, "map the architecture", None);
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "run");
        assert_eq!(encoded["body"]["request"]["prompt"], "map the architecture");
        assert!(encoded["body"]["request"].get("tools").is_none());
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }

    #[test]
    fn continue_and_cancel_envelopes_round_trip() {
        let mut metadata = RequestMetadata::local("session-1", "/repo");
        metadata.project_trusted = true;
        let run_id = RunId::new();
        let continue_request =
            RequestEnvelope::continue_run(metadata.clone(), "keep going", None, Some(run_id));
        let encoded = serde_json::to_value(&continue_request).unwrap();
        assert_eq!(encoded["body"]["type"], "continue");
        assert_eq!(encoded["body"]["request"]["prompt"], "keep going");
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            continue_request
        );

        let cancel_request = RequestEnvelope::cancel_run(metadata.clone(), Some(run_id), "user");
        let encoded = serde_json::to_value(&cancel_request).unwrap();
        assert_eq!(encoded["body"]["type"], "cancel");
        assert_eq!(encoded["body"]["request"]["reason"], "user");
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            cancel_request
        );

        let receipt_request = RequestEnvelope::receipt(metadata, Some(run_id));
        let encoded = serde_json::to_value(&receipt_request).unwrap();
        assert_eq!(encoded["body"]["type"], "receipt");
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            receipt_request
        );
    }

    #[test]
    fn new_turn_envelope_is_explicitly_versioned_and_round_trips() {
        let mut metadata = RequestMetadata::local("session-1", "/repo");
        metadata.project_trusted = true;
        let previous = RunId::new();
        let request = RequestEnvelope::new_turn(
            metadata,
            "start a fresh turn",
            Some("workspace-write".to_owned()),
            Some(previous),
        );
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "command");
        assert_eq!(encoded["body"]["request"]["name"], "run.turn.v2");
        assert_eq!(
            encoded["body"]["request"]["arguments"]["run_id"],
            previous.to_string()
        );
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }

    #[test]
    fn spawn_envelope_round_trips_packet_without_transcript() {
        let mut metadata = RequestMetadata::local("builder-1", "/repo");
        metadata.project_trusted = true;
        metadata.assign_role(&RoleSpec::builder());
        let packet = WorkPacket::builder_task("wp-1", "create GOLDEN_PATH.txt");
        let request =
            RequestEnvelope::spawn(metadata, packet.clone(), Some("workspace-write".to_owned()));
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "spawn");
        assert_eq!(encoded["body"]["request"]["packet"]["id"], "wp-1");
        assert_eq!(
            encoded["body"]["request"]["packet"]["schema"],
            WORK_PACKET_SCHEMA
        );
        assert!(encoded["body"]["request"].get("prompt").is_none());
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
        assert!(!packet.as_prompt().contains("PLANNER_SECRET_TOKEN"));
    }

    #[test]
    fn symposium_envelope_round_trips_goal_without_transcript() {
        let mut metadata = RequestMetadata::local("chair-1", "/repo");
        metadata.project_trusted = true;
        metadata.assign_role(&RoleSpec::pm());
        let request = RequestEnvelope::symposium(
            metadata,
            "create GOLDEN_PATH.txt containing hello",
            true,
            Symposium::DEFAULT_MAX_ROUNDS,
            Some("workspace-write".to_owned()),
        );
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "symposium");
        assert_eq!(
            encoded["body"]["request"]["goal"],
            "create GOLDEN_PATH.txt containing hello"
        );
        assert_eq!(encoded["body"]["request"]["anti_meeting"], true);
        assert_eq!(encoded["metadata"]["role_id"], ROLE_PM);
        assert!(encoded["body"]["request"].get("prompt").is_none());
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }

    #[test]
    fn review_envelope_round_trips_author_session_without_transcript() {
        let mut metadata = RequestMetadata::local("reviewer-1", "/repo");
        metadata.project_trusted = true;
        metadata.assign_role(&RoleSpec::reviewer());
        let request = RequestEnvelope::review(metadata, "builder-session", None);
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["body"]["type"], "review");
        assert_eq!(
            encoded["body"]["request"]["author_session_id"],
            "builder-session"
        );
        assert!(encoded["body"]["request"].get("prompt").is_none());
        assert!(encoded["body"]["request"].get("transcript").is_none());
        assert_eq!(encoded["metadata"]["role_id"], ROLE_REVIEWER);
        assert_eq!(encoded["metadata"]["department_id"], DEPARTMENT_MONITORING);
        assert_eq!(
            serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
            request
        );
    }
}

pub use kiana_domain::{
    AutomationCommand, AutomationCommandRequest, AutomationState, TriggerDefinition,
    WorkflowDefinition, AUTOMATION_COMMAND, AUTOMATION_SCHEMA, AUTOMATION_SNAPSHOT,
};

pub use kiana_domain::{
    SwarmCommand, SwarmCommandRequest, SwarmPlan, SwarmState, SWARM_COMMAND, SWARM_SCHEMA,
    SWARM_SNAPSHOT,
};
