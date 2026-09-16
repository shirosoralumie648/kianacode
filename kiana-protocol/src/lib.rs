//! Kiana client 与 daemon 之间的 versioned wire contract。
//!
//! `kiana.protocol.v1` 只定义可序列化的请求/响应外形。它是跨进程边界，不包含执行器实现
//! 或权限绕过能力；daemon 收到 envelope 后仍必须重建上下文、检查项目 trust、策略、
//! gate、审批和生命周期。新增字段优先使用 `serde(default)` 保持旧客户端可读取，但这
//! 只是兼容性策略，不代表缺失字段自动安全或自动允许。

use kiana_domain::CoreResponse;
pub use kiana_domain::{
    normalize_role_path, AgentTemplate, ApprovalChallenge, ApprovalDecision, ApprovalId,
    ArtifactId, ArtifactProvenance, ArtifactRef, ArtifactVersion, AssignmentDirectory,
    AssignmentId, AuditActionKind, AuditDecision, AuditExportFormat, AuditQueryCursor, AuditRecord,
    AuthenticatedPrincipalRef, BudgetLease, BudgetLeaseId, CapabilityErrorCode,
    CapabilityErrorPolicy, CapabilityExecutionState, CapabilityGrant, CapabilityGrantId,
    CapabilityProcessState, CapabilityResultDimensions, CatalogCandidate, CatalogEntry,
    CatalogEntryKind, CellId, CellLifecycle, CellSpec, ClosingReceipt, CommunicationMessage,
    CommunicationMessageKind, CompanyCommandReceipt, CompanyReceiptStatus, CompanyScope,
    CompanyScopeRegistry, ComponentId, Criterion, CriterionId, DecisionActorKind, DecisionOption,
    DecisionPurpose, DelegationId, DelegationPacket, DepartmentCatalog, DispatchIntent,
    DispatchIntentStatus, EntryPointKind, EntryPointParitySnapshot, EventKindSpec, EvidenceId,
    EvidenceRef, EvidenceStatus, ExecutionId, ExecutionScope, ExecutionStatus, ExtensionCatalog,
    ExtensionEffect, ExtensionError, ExtensionErrorCode, ExtensionExecutionScope, ExtensionId,
    ExtensionManifest, ExtensionNetworkPolicy, ExtensionPackage, ExtensionRequires,
    ExtensionSignature, ExtensionSnapshot, ExtensionSnapshotState, ExtensionType, Freshness,
    HookDecision, HookDecisionKind, HookDescriptor, HookOrderCandidate, HookPhase, HookRunId,
    HumanDecision, HumanTask, HumanTaskStatus, InvocationId, InvocationIdentity, MembershipId,
    MemoryScope, MergeReceipt, ModelAttemptId, ModelAttemptIdentity, ModelContent, ModelOutcome,
    ModelSideEffectState, ModelStopReason, OrganizationBinding, OrganizationId, PermissionProfile,
    PluginLifecycle, PluginLifecycleState, ProjectAssignment, ProjectAssignmentId, ProjectBinding,
    ProjectId, ProjectIdentity, ProviderConfigSnapshot, ProviderConfigSource, ProviderContinuation,
    ProviderProfileSnapshot, ProviderSelectionMode, Purpose, ReceiptId, RequestId,
    ResolvedAssignment, ReviewPacket, RiskLevel, RoleAssignment, RoleAssignmentStatus, RoleCatalog,
    RoleDescriptor, RoleSpec, RunId, RuntimeEvent, SessionAssignment, SessionId, SignalStatus,
    SkillDescriptor, SkillLifecycle, SnapshotId, SourceKind, SourceRef, SourceSnapshot, SpawnPlan,
    SpawnPlanId, StepId, StepIdentity, SupervisionLease, SupervisionLeaseId, Symposium, TemplateId,
    TurnId, TurnIdentity, TurnSemantics, WorkPacket, WorkPacketStatus, WorkspaceBinding,
    WorkspaceId, ARTIFACT_REF_SCHEMA, ARTIFACT_VERSION_SCHEMA, AUTHENTICATED_PRINCIPAL_SCHEMA,
    CAPABILITY_OUTCOME_SCHEMA, CAPABILITY_RESULT_DIMENSIONS_SCHEMA, COMMUNICATION_MESSAGE_SCHEMA,
    COMPANY_COMMAND_POLICY_SCHEMA, COMPANY_COMMAND_RECEIPT_SCHEMA, COMPANY_DISPATCH_INTENT_SCHEMA,
    DEPARTMENT_CATALOG_SCHEMA, DEPARTMENT_EXECUTING, DEPARTMENT_MONITORING, DEPARTMENT_SPEC_SCHEMA,
    ENTRYPOINT_PARITY_SCHEMA, ENTRYPOINT_PARITY_SCHEMA_VERSION, EVENT_KIND_SPECS,
    EVIDENCE_REF_SCHEMA, EXECUTION_IDENTITY_SCHEMA_VERSION, EXECUTION_SCOPE_SCHEMA,
    EXECUTION_SCOPE_SCHEMA_VERSION, EXTENSION_CATALOG_SCHEMA, EXTENSION_ERROR_SCHEMA,
    EXTENSION_MANAGE_OPERATION, EXTENSION_MANIFEST_SCHEMA, EXTENSION_PACKAGE_SCHEMA,
    EXTENSION_SNAPSHOT_CACHE_ENTRY_SCHEMA, EXTENSION_SNAPSHOT_CACHE_KEY_SCHEMA,
    EXTENSION_SNAPSHOT_SCHEMA, EXTENSION_SOURCE_RESOLUTION_SCHEMA, HOOK_DECISION_SCHEMA,
    HOOK_DESCRIPTOR_SCHEMA, HOOK_MANIFEST_SCHEMA, HUMAN_DECISION_SCHEMA, HUMAN_TASK_SCHEMA,
    INVOCATION_IDENTITY_SCHEMA, MEMORY_SCOPE_SCHEMA, MERGE_RECEIPT_PATH,
    MODEL_ATTEMPT_IDENTITY_SCHEMA, MODEL_CONTENT_SCHEMA, MODEL_OUTCOME_SCHEMA,
    PLUGIN_LIFECYCLE_SCHEMA, PLUGIN_MANIFEST_SCHEMA, PROJECT_ASSIGNMENT_SCHEMA,
    PROJECT_IDENTITY_SCHEMA, PROVIDER_CONFIG_SNAPSHOT_SCHEMA, PROVIDER_CONTINUATION_SCHEMA,
    PROVIDER_PROFILE_SNAPSHOT_SCHEMA, RESOLVED_ASSIGNMENT_SCHEMA, REVIEW_PACKET_SCHEMA,
    ROLE_ANALYST, ROLE_ARCHITECT, ROLE_ASSIGNMENT_SCHEMA, ROLE_BUILDER, ROLE_CATALOG_SCHEMA,
    ROLE_CLOSER, ROLE_INPUT_SCHEMA_PREFIX, ROLE_LIBRARIAN, ROLE_OUTPUT_SCHEMA_PREFIX, ROLE_PM,
    ROLE_QA, ROLE_REVIEWER, ROLE_SPEC_SCHEMA, RUNTIME_EVENT_SCHEMA, SCOPE_SET_SCHEMA,
    SCOPE_SET_SCHEMA_VERSION, SESSION_ASSIGNMENT_SCHEMA, SKILL_DESCRIPTOR_SCHEMA,
    SOURCE_REF_SCHEMA, SOURCE_SNAPSHOT_SCHEMA, STEP_IDENTITY_SCHEMA, TURN_IDENTITY_SCHEMA,
    WORK_PACKET_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod ui_contracts;
pub use ui_contracts::*;

pub use kiana_domain::{CapabilityRequest, ConversationMessage, ConversationRole};

pub use kiana_domain::{
    CompanyClosingReceipt, CompanyCommand, CompanyCommandRequest, CompanyState, COMPANY_COMMAND,
    COMPANY_COMMAND_SCHEMA, COMPANY_GOVERNANCE, COMPANY_SNAPSHOT, COMPANY_STATE_SCHEMA,
};

pub const PROTOCOL_SCHEMA: &str = "kiana.protocol.v1";
pub const AUDIT_QUERY_SCHEMA: &str = "kiana.audit-query.v1";

fn default_role_id() -> String {
    ROLE_BUILDER.to_owned()
}

fn default_department_id() -> String {
    DEPARTMENT_EXECUTING.to_owned()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 请求级身份、项目和权限范围快照。
pub struct RequestMetadata {
    /// 请求唯一 ID，用于事件和响应关联。
    pub request_id: RequestId,
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
}

impl RequestMetadata {
    /// 构造默认本地元数据，默认 Safe 且未信任项目。
    pub fn local(session_id: impl Into<String>, project_root: impl Into<String>) -> Self {
        Self {
            request_id: RequestId::new(),
            session_id: SessionId::new(session_id),
            project_root: project_root.into(),
            actor_id: Some("local-user".to_owned()),
            project_trusted: false,
            permission_profile: PermissionProfile::Safe,
            role_id: default_role_id(),
            department_id: default_department_id(),
            work_packet_id: None,
            path_allow: Vec::new(),
        }
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

impl RequestEnvelope {
    /// 构造通用命令 envelope。
    /// Submit a versioned Company business command through the existing command route.
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
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::ApprovalDecision(ApprovalDecisionRequest {
                approval_id,
                decision,
                request_hash,
                nonce,
            }),
        }
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

    /// 构造 cancel envelope。
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
    pub fn spawn(metadata: RequestMetadata, packet: WorkPacket, sandbox: Option<String>) -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_owned(),
            metadata,
            body: RequestBody::Spawn(SpawnRequest { packet, sandbox }),
        }
    }

    /// 构造 symposium convene envelope。
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
pub enum RequestBody {
    /// 通用命令。
    Command(CommandRequest),
    /// 审批决定。
    ApprovalDecision(ApprovalDecisionRequest),
    /// 新建 run。
    Run(RunRequest),
    /// 继续 run。
    Continue(ContinueRequest),
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
pub struct CommandRequest {
    /// 命令注册名。
    pub name: String,
    /// 命令参数。
    pub arguments: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResumeRequest {
    #[serde(default)]
    pub run_id: Option<RunId>,
}

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
pub struct ApprovalDecisionRequest {
    /// 被决定的审批 ID。
    pub approval_id: ApprovalId,
    /// approve 或 deny。
    pub decision: ApprovalDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 新建 harness run 的输入。
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
pub struct ContinueRequest {
    /// 新增提示词。
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 取消 run 的输入。
pub struct CancelRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(default)]
    /// 取消原因，供事件/receipt 记录。
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// receipt 查询参数。
pub struct ReceiptRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
pub struct ParityRequest {
    pub entrypoint: EntryPointKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParityResponse {
    pub schema: String,
    pub snapshot: EntryPointParitySnapshot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// WorkPacket spawn 参数。
pub struct SpawnRequest {
    pub packet: WorkPacket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
}

fn default_symposium_max_rounds() -> u32 {
    Symposium::DEFAULT_MAX_ROUNDS
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Symposium 参数。
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
pub struct ReviewRequest {
    pub author_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Close 参数。
pub struct CloseRequest {
    pub author_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// daemon 对请求的统一响应 envelope。
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
    pub fn failure_policy(&self) -> Option<CapabilityErrorPolicy> {
        self.failure_code().map(CapabilityErrorCode::policy)
    }

    /// 将 core 的内部响应投影为 wire 响应。
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
pub struct UiCursor {
    pub epoch: String,
    pub sequence: u64,
}

/// Optimistic precondition for one explicit human action. Core still authorizes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UiAction {
    pub target_id: String,
    pub expected_epoch: String,
    pub expected_cursor: u64,
    pub idempotency_key: String,
}

/// Server-owned projection of one session's event facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
