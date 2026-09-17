//! Kiana 核心控制面与外部适配器之间的稳定端口契约。
//!
//! 本 crate 只描述“核心层需要什么能力”，不负责选择具体实现。`kiana-core`
//! 依赖这些 trait 发起 Cell 管理、事件落盘、能力执行、审批、工具前置钩子和
//! Runner 调用；`kiana-daemon` 等上层组合根再注入内存、JSONL 或本地执行适配器。
//! 这样可以让授权与生命周期判断留在控制面，同时避免核心层反向依赖文件系统、
//! 查询索引或具体运行时。
//!
//! # 安全边界
//!
//! - 端口不是授权来源。调用 `CapabilityBrokerPort` 前，请求必须已经由控制面包装为
//!   `AuthorizedCapabilityRequest`；适配器不能根据原始模型输入自行扩权。
//! - 带有资源预留、版本比较或一次性消费语义的方法，失败时必须保持原状态，不能把
//!   不确定结果伪装成成功。
//! - 多个默认方法会对尚未实现的高级能力返回稳定错误，而不是静默降级。个别为旧实现
//!   保留的兼容默认值会在对应方法注释中明确标出，不能据此声称具备 durable 保证。
//! - 这里的 trait 允许内存实现和持久化实现共存；“实现了端口”本身不等于跨进程恢复、
//!   原子落盘或实时外部执行已经得到证明。

use async_trait::async_trait;
use kiana_domain::{
    AgentTemplate, ApprovalChallenge, ApprovalId, ArtifactRef, ArtifactVersion,
    AssignmentDirectory, AssignmentId, AuditActionKind, AuditDecision, AuditRecord,
    AuthenticatedPrincipalRef, AuthoritySnapshot, AuthorizedCapabilityRequest, BudgetLease,
    BudgetLeaseId, CapabilityGrant, CapabilityGrantId, CapabilityRequest, CapabilityResult, CellId,
    CellLifecycle, CellSpec, CommunicationMessage, ConfigSnapshot, CorrelationContext,
    CorrelationScope, EvalCase, EvalCaseId, EvalCaseResult, EvalDataset, EvalDatasetId, EvalSuite,
    EvalSuiteId, EventCursor, GoldenTrace, GoldenTraceId, HealthSnapshot, MetricPoint,
    ObservabilityRecord, OrganizationId, PendingApproval, Principal, ProjectId, ProjectIdentity,
    QualityArtifact, QualityArtifactId, RequestContext, RequestId, ResolvedAssignment,
    RetirementRecord, RoleAssignment, RunId, RuntimeEvent, SecretRef, SignalKind, SpanLinkKind,
    SpawnPlan, SpawnPlanId, StorageError, StorageErrorClass, SupervisionLease, SwarmLineage,
    SwarmPlanId, TraceSummary, WorkFingerprint,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

mod observability_queue;
pub use observability_queue::{
    ObservabilityQueue, ObservabilityQueueClass, ObservabilityQueueError, ObservabilityQueueStats,
    QueuedObservabilityItem,
};

/// Server-side identity assignment lookup boundary.
///
/// The caller supplies only the already-authenticated principal and a server-derived project
/// scope. Implementations must resolve the role, department, validity window and authority epoch
/// from their own assignment store; a role string from a wire request is only a lookup key and
/// can never create authority. A revocation must be visible to both dispatch and approval
/// consumers before they mint or consume a new effect.
#[async_trait]
pub trait AssignmentDirectoryPort: Send + Sync {
    async fn resolve_assignment(
        &self,
        principal: &AuthenticatedPrincipalRef,
        organization_id: OrganizationId,
        project_id: ProjectId,
        role_id: &str,
        now_unix_ms: u64,
    ) -> Result<ResolvedAssignment, PortError>;

    async fn revoke_assignment(
        &self,
        assignment_id: AssignmentId,
    ) -> Result<RoleAssignment, PortError>;
}

/// Alias used by adapters that expose the same port as an authenticated identity provider.
pub use AssignmentDirectoryPort as IdentityAssignmentPort;

/// Protected ingress identity resolver. Implementations authenticate the transport and derive
/// principal/authority metadata from their own store; wire actor/role/project fields are never
/// accepted as authority inputs.
#[async_trait]
pub trait IdentityResolver: Send + Sync {
    async fn resolve_principal(
        &self,
        authenticated: &AuthenticatedPrincipalRef,
    ) -> Result<Principal, PortError>;

    async fn resolve_authority(
        &self,
        principal: &Principal,
        project: &ProjectIdentity,
        session_owner: &str,
        requested_role: &str,
        now_unix_ms: u64,
    ) -> Result<AuthoritySnapshot, PortError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialState {
    Available,
    Missing,
    Expired,
    Revoked,
    Unknown,
}

/// Credential availability returned to core/runner. It carries only the opaque SecretRef,
/// generation, expiry and a digest; a provider transport may resolve the actual value later at
/// the effect boundary, but this port never returns raw secret bytes or strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialResolution {
    pub secret_ref: SecretRef,
    pub state: CredentialState,
    pub expires_at_unix_ms: Option<u64>,
    pub resolved_digest: Option<String>,
}

impl CredentialResolution {
    pub fn validate(&self, now_unix_ms: u64) -> Result<(), PortError> {
        self.secret_ref
            .validate()
            .map_err(|error| PortError::Failed(format!("credential_ref_invalid:{error}")))?;
        if self
            .expires_at_unix_ms
            .is_some_and(|expires| expires == 0 || expires <= now_unix_ms)
            && self.state == CredentialState::Available
        {
            return Err(PortError::Conflict(
                "credential_resolution_expired".to_owned(),
            ));
        }
        if let Some(digest) = &self.resolved_digest {
            if !digest.starts_with("sha256:") || digest.len() != 71 {
                return Err(PortError::Failed(
                    "credential_resolution_digest_invalid".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// Resolve an opaque SecretRef without exposing the secret value to core, runner or EventLog.
#[async_trait]
pub trait CredentialResolver: Send + Sync {
    async fn resolve_credential(
        &self,
        secret_ref: &SecretRef,
        now_unix_ms: u64,
    ) -> Result<CredentialResolution, PortError>;
}

/// Read/publish the immutable, non-secret configuration snapshot for a project.
#[async_trait]
pub trait ConfigSnapshotStore: Send + Sync {
    async fn read_snapshot(&self, project: &ProjectIdentity) -> Result<ConfigSnapshot, PortError>;

    async fn publish_snapshot(
        &self,
        snapshot: ConfigSnapshot,
        expected_revision: Option<&str>,
    ) -> Result<ConfigSnapshot, PortError>;
}

/// Rotate or revoke a SecretRef using compare-and-swap generation semantics. Returned values are
/// references only; a stale generation must fail without mutating the store.
#[async_trait]
pub trait CredentialRotationPort: Send + Sync {
    async fn rotate_credential(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError>;

    async fn revoke_credential(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError>;
}

pub use CredentialRotationPort as RotationRevokePort;

/// Versioned quality-object persistence boundary. Implementations must preserve object digests,
/// reject stale revisions and return a typed conflict instead of silently overwriting facts.
#[async_trait]
pub trait EvalStore: Send + Sync {
    async fn put_quality_artifact(&self, _artifact: QualityArtifact) -> Result<(), PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn read_quality_artifact(
        &self,
        _artifact_id: QualityArtifactId,
    ) -> Result<Option<QualityArtifact>, PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn put_dataset(&self, _dataset: EvalDataset) -> Result<(), PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn read_dataset(
        &self,
        _dataset_id: EvalDatasetId,
    ) -> Result<Option<EvalDataset>, PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn put_suite(&self, _suite: EvalSuite) -> Result<(), PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn read_suite(&self, _suite_id: EvalSuiteId) -> Result<Option<EvalSuite>, PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn put_case(&self, _case: EvalCase) -> Result<(), PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn read_case(&self, _case_id: EvalCaseId) -> Result<Option<EvalCase>, PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn put_golden_trace(&self, _trace: GoldenTrace) -> Result<(), PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }

    async fn read_golden_trace(
        &self,
        _trace_id: GoldenTraceId,
    ) -> Result<Option<GoldenTrace>, PortError> {
        Err(PortError::Unavailable("eval_store_unsupported".to_owned()))
    }
}

/// Read-only fixture bytes by an opaque reference and server-derived scope digest. A fixture
/// adapter must not expose filesystem paths or fall back to the operator workspace.
#[async_trait]
pub trait FixtureStore: Send + Sync {
    async fn read_fixture(
        &self,
        fixture_ref: &str,
        scope_digest: &str,
    ) -> Result<Vec<u8>, PortError>;
}

/// Read committed RuntimeEvent facts for a run after a logical cursor. Deltas are never authority
/// and an unsupported cursor read must remain a typed error rather than an empty result.
#[async_trait]
pub trait TraceSource: Send + Sync {
    async fn read_trace_events(
        &self,
        run_id: RunId,
        after_cursor: EventCursor,
        limit: usize,
    ) -> Result<Vec<RuntimeEvent>, PortError>;
}

/// Read immutable artifact bytes referenced by a GoldenTrace. The port does not accept a current
/// workspace path and cannot mutate an artifact or its authorization scope.
#[async_trait]
pub trait ArtifactReader: Send + Sync {
    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError>;
}

/// Semantic judge boundary. A judge returns bounded JSON findings only; it cannot promote a
/// candidate, authorize capabilities or mutate canonical history.
#[async_trait]
pub trait Judge: Send + Sync {
    async fn judge(
        &self,
        case: &EvalCase,
        trace: &GoldenTrace,
    ) -> Result<serde_json::Value, PortError>;
}

/// Quality metrics projection sink. A metrics write is not a quality verdict or policy mutation.
#[async_trait]
pub trait MetricsSink: Send + Sync {
    async fn record_eval_result(&self, result: &EvalCaseResult) -> Result<(), PortError>;
}

/// Deterministic clock boundary for evaluation and expiry checks; it has no sleep or I/O effect.
pub trait Clock: Send + Sync {
    fn now_unix_ms(&self) -> u64;
}

/// Append/read typed Swarm lineage without granting dispatch authority. Implementations must
/// preserve the lineage digest and reject cross-swarm or stale-epoch records.
#[async_trait]
pub trait SwarmLineagePort: Send + Sync {
    async fn append_lineage(&self, lineage: SwarmLineage) -> Result<(), PortError>;

    async fn read_lineage(
        &self,
        swarm_plan_id: SwarmPlanId,
    ) -> Result<Vec<SwarmLineage>, PortError>;
}

/// Read-only artifact blob boundary used when an immutable EvidenceRef is rechecked.
///
/// Implementations must return the exact bytes for the requested `(artifact_id, version)` and
/// reject missing blobs, scope mismatches and content-hash drift. The port never follows a current
/// workspace path, so a later file edit cannot rewrite historical evidence.
#[async_trait]
pub trait ArtifactContentPort: Send + Sync {
    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError>;
}

pub use ArtifactContentPort as ArtifactReadPort;

/// Message persistence boundary. A communication record is an event/fact or a request for a
/// later ControlPlane command; implementations must never interpret it as an authority grant.
#[async_trait]
pub trait CommunicationPort: Send + Sync {
    async fn append_message(
        &self,
        context: &RequestContext,
        message: &CommunicationMessage,
    ) -> Result<RuntimeEvent, PortError>;
}

/// Non-durable fixture blob store implementing the artifact read boundary.
#[derive(Clone, Debug, Default)]
pub struct InMemoryArtifactContentStore {
    entries: Arc<tokio::sync::RwLock<BTreeMap<(String, u64), (Vec<u8>, String)>>>,
}

impl InMemoryArtifactContentStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn put(&self, version: &ArtifactVersion, content: Vec<u8>) -> Result<(), PortError> {
        version
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_version_invalid:{error}")))?;
        if content.len() as u64 != version.size_bytes
            || kiana_domain::journal_sha256(&content) != version.content_hash
        {
            return Err(PortError::Failed(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        let key = (version.artifact_id.to_string(), version.version);
        let mut entries = self.entries.write().await;
        if entries.contains_key(&key) {
            return Err(PortError::Conflict(
                "artifact_version_already_stored".to_owned(),
            ));
        }
        entries.insert(key, (content, version.scope_digest.clone()));
        Ok(())
    }
}

#[async_trait]
impl ArtifactContentPort for InMemoryArtifactContentStore {
    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        reference
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_reference_invalid:{error}")))?;
        let key = (reference.artifact_id.to_string(), reference.version);
        let (content, stored_scope) = self
            .entries
            .read()
            .await
            .get(&key)
            .cloned()
            .ok_or_else(|| PortError::Unavailable("artifact_blob_missing".to_owned()))?;
        if stored_scope != reference.scope_digest {
            return Err(PortError::Conflict("artifact_scope_mismatch".to_owned()));
        }
        if kiana_domain::journal_sha256(&content) != reference.content_hash {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(content)
    }
}

/// Explicitly non-durable in-process assignment adapter for fixtures and the local daemon.
///
/// It is a server-owned snapshot and therefore rejects malformed or cross-boundary bindings, but
/// it does not claim persistence or cross-process recovery. Production adapters can implement
/// [`AssignmentDirectoryPort`] against the durable identity store without changing core callers.
#[derive(Clone, Debug, Default)]
pub struct InMemoryAssignmentDirectory {
    inner: Arc<tokio::sync::RwLock<AssignmentDirectory>>,
}

impl InMemoryAssignmentDirectory {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_role(&self, assignment: RoleAssignment) -> Result<(), PortError> {
        self.inner
            .write()
            .await
            .register_role(assignment)
            .map_err(PortError::Failed)
    }

    pub async fn register_project(
        &self,
        assignment: kiana_domain::ProjectAssignment,
    ) -> Result<(), PortError> {
        self.inner
            .write()
            .await
            .register_project(assignment)
            .map_err(PortError::Failed)
    }

    pub async fn snapshot(&self) -> AssignmentDirectory {
        self.inner.read().await.clone()
    }
}

#[async_trait]
impl AssignmentDirectoryPort for InMemoryAssignmentDirectory {
    async fn resolve_assignment(
        &self,
        principal: &AuthenticatedPrincipalRef,
        organization_id: OrganizationId,
        project_id: ProjectId,
        role_id: &str,
        now_unix_ms: u64,
    ) -> Result<ResolvedAssignment, PortError> {
        self.inner
            .read()
            .await
            .resolve(principal, organization_id, project_id, role_id, now_unix_ms)
            .map_err(PortError::Failed)
    }

    async fn revoke_assignment(
        &self,
        assignment_id: AssignmentId,
    ) -> Result<RoleAssignment, PortError> {
        self.inner
            .write()
            .await
            .revoke_role(assignment_id)
            .map_err(PortError::Failed)
    }
}

/// Read-only edit checkpoint adapter. Restoring files is deliberately absent: it is brokered.
#[async_trait]
pub trait WorkspaceCheckpointPort: Send + Sync {
    async fn capture_files(
        &self,
        project_root: &str,
        paths: &[String],
    ) -> Result<Vec<kiana_domain::WorkspaceFileSnapshot>, PortError>;
    async fn preview(
        &self,
        checkpoint: &kiana_domain::WorkspaceCheckpoint,
    ) -> Result<kiana_domain::CheckpointPreview, PortError>;
}

#[derive(Clone, Debug, PartialEq)]
/// 一次幂等事件追加的可核验结果。
///
/// 调用方必须通过 [`Self::replayed`] 区分“本次新写入”和“命中既有事件”。两种情况都
/// 返回事实源中最终采用的事件，避免重试方继续使用一个未真正落盘的候选值。
pub struct EventAppendResult {
    /// 事件存储最终接受的事件；重放时通常是先前已保存的那一条。
    pub event: RuntimeEvent,
    /// `true` 表示幂等键命中了既有事件，本次没有产生第二条事实记录。
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq)]
/// 为创建一个 Cell 提交给注册表的完整预留请求。
///
/// 这些字段必须作为一个整体校验。实现不应先占用部分预算或路径锁，再在后续校验失败
/// 时遗留资源；成功也只表示资源已经预留，并不表示 Cell 已启动或 Runner 已执行。
pub struct SpawnReservationRequest {
    /// 待预留的分裂计划；通常应处于 `Proposed` 状态。
    pub plan: SpawnPlan,
    /// 与计划对应的 Cell 规格，包含父子关系、角色和生命周期等约束。
    pub cell: CellSpec,
    /// 已按角色与精确版本解析出的模板；注册表仍需核对其身份和有效性。
    pub template: AgentTemplate,
    /// 为该 Cell 划定的预算租约；子 Cell 的额度不能突破上级可委派范围。
    pub budget: BudgetLease,
    /// 控制该 Cell 可申请哪些能力的授权；它不是 Broker 的执行结果。
    pub grant: CapabilityGrant,
    /// 心跳、停滞阈值等监督约束，用于限制 Cell 的存活和失联行为。
    pub supervision: SupervisionLease,
    /// 工作内容指纹，用于识别仍在活动的重复任务。
    pub fingerprint: WorkFingerprint,
    /// 本次预留声明独占的写路径；实现负责与 Cell 写集及现有路径锁交叉校验。
    pub owned_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
/// 注册表接受预留后保存的权威快照。
///
/// 返回完整快照而不是单独的 ID，是为了让控制面后续提交、回滚和绑定能力请求时使用
/// 同一组计划、授权、预算与路径数据。调用方不得自行拼接字段来扩大这份预留。
pub struct SpawnReservation {
    /// 注册表实际保存的分裂计划及其当前状态。
    pub plan: SpawnPlan,
    /// 注册表实际保存的 Cell 规格及其当前生命周期。
    pub cell: CellSpec,
    /// 此预留绑定的不可变 Agent 模板快照。
    pub template: AgentTemplate,
    /// 此预留绑定的预算账本快照；消耗后返回值可能包含更新后的计数。
    pub budget: BudgetLease,
    /// 此预留绑定的能力授权快照。
    pub grant: CapabilityGrant,
    /// 此预留绑定的监督租约快照。
    pub supervision: SupervisionLease,
    /// 用于重复工作检测的工作指纹。
    pub fingerprint: WorkFingerprint,
    /// 当前由该 Cell 持有的路径锁范围。
    pub owned_paths: Vec<String>,
    /// `true` 表示相同幂等请求复用了既有预留，而非再次占用资源。
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq)]
/// 一次 Cell 能力调用在注册表中的在途租约。
///
/// [`CellRegistryPort::begin_capability`] 成功后，调用方必须保存并原样交回这份值；注册表
/// 可据此拒绝重复调用、伪造完成或跨 Cell 结算。租约只证明预算与授权检查已经通过，
/// 不证明外部副作用已经发生。
pub struct CapabilityLease {
    /// 此次能力调用的稳定请求 ID，也是重复在途调用的冲突键。
    pub request_id: RequestId,
    /// 发起调用且被计入并发额度的 Cell。
    pub cell_id: CellId,
    /// 开始调用时核验过的能力授权 ID。
    pub capability_grant_id: CapabilityGrantId,
    /// 本次调用扣减的预算租约 ID。
    pub budget_lease_id: BudgetLeaseId,
    /// 为本次调用预记的有后果动作数量；只读调用通常为零。
    pub effect_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Broker 返回后，控制面对能力调用现实结果的认知。
///
/// 这个枚举描述“能证明到什么程度”，不用于倒推是否退款。预算是否在开始时扣减、失败时
/// 是否返还由注册表实现明确决定；尤其不能因为结果是 [`Self::Unknown`] 就假定没有副作用。
pub enum CapabilityOutcome {
    /// Broker 明确报告调用已完成并得到成功结果。
    Succeeded,
    /// Broker 明确报告调用失败；仍不自动代表所有外部副作用均已回滚。
    Failed,
    /// 连接中断、取消竞态等原因导致结果不可判定，必须按可能已产生后果来处理。
    Unknown,
}

#[async_trait]
/// Cell 准入、资源预留、能力围栏和退休回收的状态端口。
///
/// 实现需要把相互依赖的状态变化放在同一个一致性边界内。例如分裂预留必须同时检查
/// 父子权限、预算、并发和路径锁；能力开始必须同时校验 Cell 状态、Grant、Budget 与
/// 请求绑定。当前默认组合可以是进程内实现，但 trait 本身不承诺 durable 恢复。
pub trait CellRegistryPort: Send + Sync {
    /// Charge a completed model turn against the same Cell budget as tool work.
    async fn account_model_usage(
        &self,
        _cell_id: CellId,
        _turn_key: &str,
        _tokens: u64,
    ) -> Result<BudgetLease, PortError> {
        Err(PortError::Unavailable(
            "cell_model_accounting_unsupported".to_owned(),
        ))
    }

    /// Export quiescent authority and consumed budgets for an event-backed run checkpoint.
    async fn checkpoint_run(&self, _run_id: RunId) -> Result<serde_json::Value, PortError> {
        Err(PortError::Unavailable(
            "cell_checkpoint_unsupported".to_owned(),
        ))
    }

    /// Revalidate and atomically install a ledger snapshot without starting a worker.
    async fn restore_run(
        &self,
        _run_id: RunId,
        _snapshot: serde_json::Value,
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "cell_restore_unsupported".to_owned(),
        ))
    }

    /// 按角色 ID 和精确版本解析 Agent 模板。
    ///
    /// 不应在版本不匹配时自动回退到“最接近”版本，否则审批时看到的模板可能与执行时
    /// 不同。未知角色、空版本或不受支持版本应返回结构化端口错误。
    async fn resolve_template(
        &self,
        role_id: &str,
        version: &str,
    ) -> Result<AgentTemplate, PortError>;

    /// 原子校验并预留一次 Cell 创建所需的全部资源。
    ///
    /// 成功返回 [`SpawnReservation`] 后，调用方仍需显式调用 [`Self::commit_spawn`]；如果
    /// 后续启动失败，则必须调用 [`Self::abort_spawn`] 回收预算和路径锁。
    async fn reserve_spawn(
        &self,
        request: SpawnReservationRequest,
    ) -> Result<SpawnReservation, PortError>;

    /// 以比较并交换方式推进 Cell 生命周期。
    ///
    /// 只有当前状态等于 `expected` 且领域状态机允许 `expected -> next` 时才能成功。
    /// 状态已被其他任务推进时应返回冲突，不能覆盖较新的状态。
    async fn transition_cell(
        &self,
        cell_id: CellId,
        expected: CellLifecycle,
        next: CellLifecycle,
    ) -> Result<CellSpec, PortError>;

    /// 在真正调用 Broker 前，为 Cell 请求建立能力租约并预扣相应预算。
    ///
    /// 实现至少应核对请求中的 Cell、Grant 和 Budget ID 与注册表预留完全一致，并检查
    /// 生命周期、授权期限、能力范围、并发上限和重复请求。默认实现选择 fail-closed，
    /// 使未升级的注册表不能绕过能力围栏。
    async fn begin_capability(
        &self,
        _cell_id: CellId,
        _capability_grant_id: CapabilityGrantId,
        _budget_lease_id: BudgetLeaseId,
        _request: &CapabilityRequest,
    ) -> Result<CapabilityLease, PortError> {
        Err(PortError::Failed(
            "cell_registry_capability_fence_unsupported".to_owned(),
        ))
    }

    /// 结束一个在途能力租约并登记结果认知。
    ///
    /// `lease` 必须与开始阶段返回值完全一致。实现应拒绝重复完成和错配租约；对
    /// [`CapabilityOutcome::Unknown`] 不得擅自宣称副作用未发生。默认实现不支持结算，
    /// 以免调用方误以为并发槽或预算已经正确释放。
    async fn finish_capability(
        &self,
        _lease: CapabilityLease,
        _outcome: CapabilityOutcome,
    ) -> Result<(), PortError> {
        Err(PortError::Failed(
            "cell_registry_capability_accounting_unsupported".to_owned(),
        ))
    }

    /// 提交先前的分裂预留，使计划和 Cell 进入可启动状态。
    ///
    /// 该操作按 `plan_id` 寻址，应当可安全重试；它不能隐式创建一个不存在的预留。
    /// 默认实现显式拒绝，避免旧注册表跳过两阶段准入。
    async fn commit_spawn(&self, plan_id: SpawnPlanId) -> Result<SpawnReservation, PortError> {
        let _ = plan_id;
        Err(PortError::Failed(
            "cell_registry_commit_unsupported".to_owned(),
        ))
    }

    /// 回滚尚未完成的分裂流程，并释放它占用的预算与路径锁。
    ///
    /// `reason` 用于审计和诊断，不应改变回滚授权范围。实现必须保证失败不会留下一个
    /// 被标记为已回滚、资源却仍被占用或可继续执行的半状态。
    async fn abort_spawn(&self, plan_id: SpawnPlanId, reason: &str) -> Result<(), PortError>;

    /// 在 Cell 到达允许退休的终态后回收资源，并生成退休记录。
    ///
    /// 有能力调用仍在途时应拒绝退休；返回值列出实际释放的授权、预算和路径范围，供
    /// 事件账本与收据投影使用。
    async fn retire_cell(
        &self,
        cell_id: CellId,
        reason: &str,
    ) -> Result<RetirementRecord, PortError>;

    /// 查询某次 Run 绑定的 Cell。
    ///
    /// `Ok(None)` 表示没有登记绑定，不等同于存储故障。一个实现若保留多个历史 Cell，
    /// 应优先返回仍活动的绑定，并保持选择规则稳定。
    async fn cell_for_run(&self, run_id: RunId) -> Result<Option<CellId>, PortError>;

    /// 读取某个 Cell 对应的完整预留快照。
    ///
    /// 控制面用它把权威 Grant 与 Budget ID 写入能力请求。默认实现拒绝查询，而不是
    /// 返回 `None`，从而区分“此 Cell 不存在”和“适配器根本不支持该安全检查”。
    async fn reservation_for_cell(
        &self,
        cell_id: CellId,
    ) -> Result<Option<SpawnReservation>, PortError> {
        let _ = cell_id;
        Err(PortError::Failed(
            "cell_registry_lookup_unsupported".to_owned(),
        ))
    }
}

/// Service-side construction boundary for correlation contexts.
///
/// This port is deliberately synchronous and side-effect free: the caller must supply an
/// already-authenticated `RequestContext`, resolved scope and current epochs. Implementations
/// must not accept actor, project, authority or policy values from traceparent or model/UI data.
pub trait CorrelationContextPort: Send + Sync {
    fn root(
        &self,
        request: &RequestContext,
        scope: CorrelationScope,
        authority_epoch: u64,
        data_epoch: u64,
        traceparent: Option<&str>,
    ) -> Result<CorrelationContext, PortError>;

    fn child_span(&self, parent: &CorrelationContext) -> Result<CorrelationContext, PortError>;

    fn linked_child(
        &self,
        parent: &CorrelationContext,
        relationship: SpanLinkKind,
    ) -> Result<CorrelationContext, PortError>;
}

/// Default in-process adapter; it only delegates to the domain invariants.
#[derive(Clone, Copy, Debug, Default)]
pub struct DomainCorrelationContextPort;

impl CorrelationContextPort for DomainCorrelationContextPort {
    fn root(
        &self,
        request: &RequestContext,
        scope: CorrelationScope,
        authority_epoch: u64,
        data_epoch: u64,
        traceparent: Option<&str>,
    ) -> Result<CorrelationContext, PortError> {
        CorrelationContext::from_request(request, scope, authority_epoch, data_epoch, traceparent)
            .map_err(|error| PortError::Failed(format!("correlation_context:{error}")))
    }

    fn child_span(&self, parent: &CorrelationContext) -> Result<CorrelationContext, PortError> {
        parent
            .child_span()
            .map_err(|error| PortError::Failed(format!("correlation_context:{error}")))
    }

    fn linked_child(
        &self,
        parent: &CorrelationContext,
        relationship: SpanLinkKind,
    ) -> Result<CorrelationContext, PortError> {
        parent
            .linked_child(relationship)
            .map_err(|error| PortError::Failed(format!("correlation_context:{error}")))
    }
}

/// Immutable notification describing one newly committed transition.
///
/// The notification is constructed only after `EventStorePort::commit_transition` returns
/// `CommitOutcome::Committed`. It carries the exact batch and receipt together so observers can
/// rebuild a projection from the committed cursor without treating a callback as a fact source.
#[derive(Clone, Debug, PartialEq)]
pub struct CommittedTransition {
    pub batch: kiana_domain::TransitionBatch,
    pub receipt: kiana_domain::CommandReceipt,
}

impl CommittedTransition {
    pub fn new(
        batch: kiana_domain::TransitionBatch,
        receipt: kiana_domain::CommandReceipt,
    ) -> Result<Self, PortError> {
        let notification = Self { batch, receipt };
        notification.validate()?;
        Ok(notification)
    }

    pub fn source_cursor(&self) -> kiana_domain::EventCursor {
        self.receipt.cursor
    }

    pub fn source_event_ids(&self) -> &[kiana_domain::EventId] {
        &self.receipt.event_ids
    }

    pub fn validate(&self) -> Result<(), PortError> {
        self.batch
            .validate()
            .map_err(|error| PortError::Failed(format!("commit_notification_batch:{error}")))?;
        if self.receipt.command_id != self.batch.command_id
            || self.receipt.command_digest != self.batch.command_digest
        {
            return Err(PortError::Conflict(
                "commit_notification_receipt_identity_mismatch".to_owned(),
            ));
        }
        if self.receipt.first_cursor == 0
            || self.receipt.cursor < self.receipt.first_cursor
            || self.receipt.event_ids.len() != self.batch.events.len()
        {
            return Err(PortError::Failed(
                "commit_notification_cursor_invalid".to_owned(),
            ));
        }
        let expected_cursor = self
            .receipt
            .first_cursor
            .checked_add(self.batch.events.len() as u64 - 1)
            .ok_or_else(|| PortError::Failed("commit_notification_cursor_overflow".to_owned()))?;
        if self.receipt.cursor != expected_cursor {
            return Err(PortError::Conflict(
                "commit_notification_cursor_not_contiguous".to_owned(),
            ));
        }
        let expected_ids = self
            .batch
            .events
            .iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>();
        if self.receipt.event_ids != expected_ids {
            return Err(PortError::Conflict(
                "commit_notification_event_ids_mismatch".to_owned(),
            ));
        }
        let unique = self
            .receipt
            .event_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if unique.len() != self.receipt.event_ids.len() {
            return Err(PortError::Conflict(
                "commit_notification_event_ids_duplicate".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Observer invoked sequentially after a fresh transition commit.
///
/// Observers receive a clone of committed facts and cannot participate in authorization or call
/// the Broker through this port. An observer error is diagnostic: it must not turn an already
/// committed transition into a false rejection or trigger a second commit attempt.
#[async_trait]
pub trait EventStoreCommitObserver: Send + Sync {
    async fn on_committed(&self, transition: CommittedTransition) -> Result<(), PortError>;
}

/// Compatibility spelling for callers that use the shorter observer name.
pub use EventStoreCommitObserver as CommitObserver;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitObserverFailure {
    pub command_id: kiana_domain::RequestId,
    pub commit_id: kiana_domain::EventId,
    pub source_cursor: kiana_domain::EventCursor,
    pub reason: String,
}

#[async_trait]
/// 追加式运行事件事实源的最小端口。
///
/// 事件顺序、聚合版本和幂等键共同决定重试与恢复是否可信。基础方法用于兼容简单
/// 适配器；授权、审批消费、预算和许可等关联权威变更必须检查事务能力并使用
/// [`Self::commit_transition`]。单事件追加不能替代多聚合事务。trait 的存在不自动保证
/// 落盘、`fsync`、跨进程锁或损坏恢复，这些由具体适配器声明并提供证据。
pub trait EventStorePort: Send + Sync {
    /// Capability negotiation is mandatory before a caller relies on atomic authority changes.
    fn supports_atomic_transitions(&self) -> bool {
        false
    }

    fn capabilities(&self) -> kiana_domain::EventStoreCapabilities {
        kiana_domain::EventStoreCapabilities::default()
    }

    /// Atomically checks every read dependency and commits all events or none. Unknown outcomes
    /// must be confirmed with read_command; they never permit execution.
    async fn commit_transition(
        &self,
        _batch: kiana_domain::TransitionBatch,
    ) -> Result<kiana_domain::CommitOutcome, PortError> {
        Err(PortError::Unavailable(
            "event_store_atomic_transitions_unsupported".to_owned(),
        ))
    }

    async fn read_command(
        &self,
        _command_id: &RequestId,
    ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
        Err(PortError::Unavailable(
            "event_store_command_receipts_unsupported".to_owned(),
        ))
    }

    /// Reads after a logical cursor. A page never splits a committed transaction; a first frame
    /// larger than limit is returned whole within the store's maximum batch bound.
    async fn read_from(
        &self,
        _cursor: kiana_domain::EventCursor,
        _limit: usize,
    ) -> Result<kiana_domain::JournalPage, PortError> {
        Err(PortError::Unavailable(
            "event_store_cursor_reads_unsupported".to_owned(),
        ))
    }

    /// 无条件追加一条事件。
    ///
    /// 此方法不表达 CAS 或幂等语义，重试可能产生重复记录；仅在调用方能接受该边界或
    /// 适配器另有保证时使用。
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError>;

    /// 在聚合流版本符合预期时追加事件。
    ///
    /// `Some(version)` 表示比较并交换：当前流版本不一致时必须返回冲突且不能写入。
    /// `None` 表示调用方没有提出版本前置条件。默认实现只支持 `None`，对任何版本要求
    /// fail-closed，防止不具备 CAS 的旧适配器静默接受并发写入。
    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        if expected_version.is_some() {
            return Err(PortError::Failed(
                "event_store_expected_version_unsupported".to_owned(),
            ));
        }
        self.append(event).await
    }

    /// 按事件携带的幂等键追加，或返回此前已经接受的同一事件。
    ///
    /// 相同键但内容冲突时应报错，不能把不同事实当成重放。默认实现显式不支持，因此
    /// 需要幂等保证的调用不会退化为普通 [`Self::append`]。
    async fn append_idempotent(
        &self,
        _event: RuntimeEvent,
    ) -> Result<EventAppendResult, PortError> {
        Err(PortError::Failed(
            "event_store_idempotency_unsupported".to_owned(),
        ))
    }

    /// 同时施加幂等键和聚合流期望版本约束。
    ///
    /// 生产适配器应覆盖此方法，在同一个原子边界内完成两项检查。兼容默认实现的能力
    /// 较弱：无版本条件时委托 [`Self::append_idempotent`]；有版本条件时仅调用
    /// [`Self::append_expected`] 并报告为新写入。它不会识别成功写入后的重放，因此不能
    /// 作为 durable 幂等证明。
    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        if expected_version.is_none() {
            return self.append_idempotent(event).await;
        }
        self.append_expected(event.clone(), expected_version)
            .await?;
        Ok(EventAppendResult {
            event,
            replayed: false,
        })
    }

    /// 按请求 ID 读取与一次入口请求关联的全部事件。
    ///
    /// 返回顺序应与适配器的事实顺序一致，不能用 UI 排序或 transcript 覆盖事实源顺序。
    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError>;

    /// 读取存储中的全部事件。
    ///
    /// 默认实现不支持全量扫描。调用方必须区分“不支持/读取失败”和“事件集合为空”，
    /// 以免故障时生成一张看似没有副作用的空收据。
    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Err(PortError::Failed(
            "event_store_read_all_unsupported".to_owned(),
        ))
    }

    /// 读取指定聚合类型与聚合 ID 的事件流。
    ///
    /// 默认实现通过 [`Self::read_all`] 后过滤得到结果，适合小型或兼容适配器，但不提供
    /// 独立流的存储隔离与性能保证。无法全量读取时会原样失败，不回退成空流。
    async fn read_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self
            .read_all()
            .await?
            .into_iter()
            .filter(|event| {
                event.aggregate_type.as_deref() == Some(aggregate_type)
                    && event.aggregate_id.as_deref() == Some(aggregate_id)
            })
            .collect())
    }
}

#[async_trait]
/// 唯一接收“已授权能力请求”的执行 Broker 端口。
///
/// 该端口位于授权决策之后：它只接受 [`AuthorizedCapabilityRequest`]，不接受模型产生的
/// 原始 [`CapabilityRequest`]。实现负责在授权快照和 sandbox 边界内派发真实执行并返回
/// 结构化结果，但不能自行增加工具、路径、网络或权限。
pub trait CapabilityBrokerPort: Send + Sync {
    /// 执行一项已经由控制面授权并封装的能力请求。
    ///
    /// 传输错误应返回 [`PortError`]；能力本身的成功、拒绝或结果未知则由
    /// [`CapabilityResult`] 表达，调用方据此记录事件和结算能力租约。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError>;

    /// Cancellation must report uncertainty unless the adapter confirms its effects stopped.
    async fn execute_cancellable(
        &self,
        request: AuthorizedCapabilityRequest,
        mut cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        tokio::select! {
            biased;
            _ = wait_for_cancellation(&mut cancellation) => Err(PortError::Failed("result_unknown:cancel_stop_unconfirmed".to_owned())),
            result = self.execute(request) => result,
        }
    }
}

pub async fn wait_for_cancellation(cancellation: &mut tokio::sync::watch::Receiver<bool>) {
    loop {
        if *cancellation.borrow() {
            return;
        }
        if cancellation.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[async_trait]
/// 审批挑战从暂存到一次性消费的状态存储端口。
///
/// 审批必须绑定请求内容和授权上下文，并遵循 `staged -> active -> consumed`（或过期、
/// 取消）的单向状态变化。审批 ID 不是可转借的通行证；适配器应核对主体、会话、项目、
/// 角色、部门、路径范围以及 proof，且不能重复消费。
pub trait ApprovalStorePort: Send + Sync {
    /// Authenticated execution material for policy/gate/hook revalidation; never a UI preview.
    async fn pending_with_proof(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
        _request_hash: Option<&str>,
        _nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "approval_execution_material_unsupported".to_owned(),
        ))
    }

    /// Read the durable decision without creating execution authority or changing its identity.
    async fn read_decision(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<kiana_domain::ApprovalDecisionRecord, PortError> {
        Err(PortError::Unavailable(
            "approval_decision_read_unsupported".to_owned(),
        ))
    }

    /// Prepare Active in the same journal transaction as the Run pause/pending facts.
    async fn prepare_activation(
        &self,
        _approval_id: ApprovalId,
        _command_id: RequestId,
    ) -> Result<kiana_domain::PreparedApprovalConsumption, PortError> {
        Err(PortError::Unavailable(
            "approval_atomic_activation_unsupported".to_owned(),
        ))
    }

    /// Prepare Approved -> Consumed without writing. Core must atomically commit this read set
    /// and event with the exact dispatch permit, then compare pending.request to the permit.
    async fn prepare_consumption(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
        _dispatch_command_id: RequestId,
    ) -> Result<kiana_domain::PreparedApprovalConsumption, PortError> {
        Err(PortError::Unavailable(
            "approval_atomic_consumption_unsupported".to_owned(),
        ))
    }

    /// Recover only this challenge's frozen scope after authenticating its principal.
    async fn context_for_pending(
        &self,
        context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<RequestContext, PortError> {
        Ok(context.clone())
    }

    async fn invalidate_project(
        &self,
        _project_root: &str,
        _reason: &str,
    ) -> Result<Vec<ApprovalId>, PortError> {
        Err(PortError::Unavailable(
            "approval_project_invalidation_unsupported".to_owned(),
        ))
    }
    /// Read pending challenges bound to this authenticated session and scope.
    async fn list_pending(
        &self,
        _context: &RequestContext,
    ) -> Result<Vec<PendingApproval>, PortError> {
        Err(PortError::Unavailable(
            "approval_listing_unsupported".to_owned(),
        ))
    }

    /// Record the actual decision, including a denied terminal state.
    async fn decide_with_proof(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: kiana_domain::ApprovalDecision,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        if decision == kiana_domain::ApprovalDecision::Approve {
            self.consume_with_proof(context, approval_id, request_hash, nonce)
                .await
        } else {
            Err(PortError::Unavailable(
                "approval_denial_unsupported".to_owned(),
            ))
        }
    }

    /// 为待审批能力请求创建暂存记录和发给审批者的 challenge。
    ///
    /// 暂存成功不代表审批已经生效。实现应校验 `context.request_id` 与请求一致，并将
    /// challenge 的摘要、nonce、有效期和上下文绑定保存为同一条状态记录。
    async fn stage(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        reason: &str,
    ) -> Result<ApprovalChallenge, PortError>;

    /// 激活一条已成功暂存并已进入审计链的审批挑战。
    ///
    /// 过期、不存在、已消费或状态不符都应拒绝。将暂存与激活拆开，可以避免事件记录
    /// 失败时留下一个没有可核验挑战却可被消费的审批。
    async fn activate(&self, approval_id: ApprovalId) -> Result<(), PortError>;

    /// 使用当前请求上下文一次性消费审批，并取回原始待审批请求。
    ///
    /// 这是保留给旧进程内调用方的兼容入口，无法在签名中携带 challenge proof。需要
    /// proof 的入口应调用 [`Self::consume_with_proof`]；实现仍须校验上下文绑定与状态。
    async fn consume(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError>;

    /// 携带请求摘要和 nonce，一次性消费审批。
    ///
    /// 安全敏感适配器应覆盖此方法并将两个 proof 与暂存 challenge 做精确比较。默认实现
    /// 为兼容旧适配器而忽略 proof、退回 [`Self::consume`]；因此“只实现 trait”不能作为
    /// wire proof 已强制的证据。
    async fn consume_with_proof(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        _request_hash: Option<&str>,
        _nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        self.consume(context, approval_id).await
    }

    /// Validate an approval proof without changing its lifecycle state.
    ///
    /// A control plane may need to report that a Run-bound approval cannot yet
    /// be continued after a restart. That decision must still authenticate the
    /// caller and enforce expiry, digest, nonce, and context binding without
    /// consuming the approval that a later recovery process may need. Adapters
    /// that support this recovery boundary should override the method; the
    /// default fails closed so an older adapter cannot silently weaken it.
    async fn validate_with_proof(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
        _request_hash: Option<&str>,
        _nonce: Option<&str>,
    ) -> Result<(), PortError> {
        Err(PortError::Failed(
            "approval_proof_validation_unsupported".to_owned(),
        ))
    }

    /// 使尚未消费的审批失效，例如 Run 取消或授权上下文撤销。
    ///
    /// 失效后必须阻止后续消费。默认实现选择拒绝“不支持失效”，让控制面可以 fail-closed，
    /// 而不是在取消路径上继续保留一张可用审批。
    async fn invalidate(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
        _reason: &str,
    ) -> Result<(), PortError> {
        Err(PortError::Failed(
            "approval_invalidation_unsupported".to_owned(),
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 工具真正进入策略、审批和 Broker 前，项目钩子给出的受限决策。
///
/// 钩子只能进一步收紧请求，不能授予控制面尚未授予的能力。即使返回 [`Self::Allow`]，
/// 请求仍需通过后续政策、Gate、审批和 sandbox 检查。
pub enum PreToolHookDecision {
    /// 钩子不增加额外限制；不是最终授权。
    Allow,
    /// 以稳定、可审计的原因拒绝本次请求。
    Block(String),
    /// 暂停执行并要求显式审批；原因会进入 challenge 和事件记录。
    Ask { reason: String },
}

#[async_trait]
/// 在能力请求执行前调用项目钩子的适配端口。
///
/// 实现可能读取受信项目配置，因此组合根必须先完成 `ProjectTrust` 判断。端口只返回
/// `Allow`、`Block` 或 `Ask`，不能直接执行副作用，也不能改写请求扩大其能力范围。
pub trait PreToolHookPort: Send + Sync {
    /// Pure preparation may pin trusted descriptors/configuration. It never executes a process
    /// or performs discovery; those effects remain separate brokered capabilities.
    async fn prepare_action(
        &self,
        _context: &RequestContext,
        request: &CapabilityRequest,
        cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<CapabilityRequest, PortError> {
        if *cancellation.borrow() {
            return Err(PortError::Failed("cancelled:before_prepare".to_owned()));
        }
        Ok(request.clone())
    }

    /// Process-owning hooks must propagate cancellation and wait for their supervisor to finish.
    async fn decide_cancellable(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<PreToolHookDecision, PortError> {
        if *cancellation.borrow() {
            return Ok(PreToolHookDecision::Block(
                "cancelled:before_hook".to_owned(),
            ));
        }
        let decision = self.decide(context, request).await?;
        if *cancellation.borrow() {
            return Ok(PreToolHookDecision::Block(
                "cancelled:hook_stopped".to_owned(),
            ));
        }
        Ok(decision)
    }

    /// 根据当前授权上下文和能力请求计算钩子决策。
    ///
    /// 配置损坏、钩子执行失败或决策无法解释时应返回错误，由控制面按拒绝处理。
    async fn decide(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<PreToolHookDecision, PortError>;
}

#[derive(Debug, Default)]
/// 不施加额外钩子限制的空适配器。
///
/// 它用于没有配置项目钩子的组合或测试基线。名称中的 “AllowAll” 仅表示本端口不拦截；
/// 控制面的权限、政策、审批与 Broker 校验仍然生效，不能把它当成全局放行开关。
pub struct AllowAllPreToolHooks;

#[async_trait]
impl PreToolHookPort for AllowAllPreToolHooks {
    async fn decide(
        &self,
        _context: &RequestContext,
        _request: &CapabilityRequest,
    ) -> Result<PreToolHookDecision, PortError> {
        Ok(PreToolHookDecision::Allow)
    }
}

#[async_trait]
/// 控制面向规范 Agent Runner 发送命令的端口。
///
/// Runner 负责模型循环和产生能力申请，不拥有直接执行工具的权限。返回的事件是本次命令
/// 产生的有序协议事件；其中的工具请求仍必须回到控制面，经授权后交给 Broker。
pub trait RunnerPort: Send + Sync {
    /// Trusted runtime assignment; model text has no access to this setter.
    fn bind_model_assignment(
        &self,
        _run_id: RunId,
        _assignment: kiana_domain::ModelAssignment,
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "runner_model_assignment_unsupported".to_owned(),
        ))
    }
    /// Exact history derived from committed control-plane events, not wire-supplied role strings.
    fn bind_model_history(
        &self,
        _run_id: RunId,
        history: Vec<kiana_domain::ModelMessage>,
    ) -> Result<(), PortError> {
        if history.is_empty() {
            Ok(())
        } else {
            Err(PortError::Unavailable(
                "runner_protocol_history_unsupported".to_owned(),
            ))
        }
    }
    /// Installed once by the trusted daemon; custom runners must explicitly support admission.
    fn install_model_budget(
        &self,
        _budget: std::sync::Arc<dyn ModelBudgetPort>,
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "runner_model_budget_unsupported".to_owned(),
        ))
    }
    /// Export only paused state; never perform a model call or filesystem operation.
    async fn checkpoint(&self, _run_id: RunId) -> Result<serde_json::Value, PortError> {
        Err(PortError::Unavailable(
            "runner_checkpoint_unsupported".to_owned(),
        ))
    }

    /// Install a checkpoint supplied by ControlPlane, without executing it.
    async fn restore(
        &self,
        _run_id: RunId,
        _checkpoint: serde_json::Value,
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "runner_restore_unsupported".to_owned(),
        ))
    }

    /// 发送一条版本化 Runner 命令，并等待本轮产生的协议事件。
    ///
    /// 适配器不可在协议之外另起第二个执行循环。运行时不可用应使用
    /// [`PortError::Unavailable`]，协议状态冲突使用 [`PortError::Conflict`]。
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError>;

    /// 发送一条版本化 Runner 命令，并在事件产生时交给 `on_event`。
    ///
    /// 默认实现为保持现有适配器零改动，先调用 [`RunnerPort::send`]，再按返回顺序重放事件。
    /// 支持实时增量的实现可以覆写本方法；无论实现方式如何，成功返回的事件必须与
    /// `on_event` 收到的序列一致。回调返回错误表示下游取消或背压，Runner 必须停止并
    /// fail-closed，不能静默丢弃该错误。默认重放发生在 `send` 完成后，无法撤回已经结束的
    /// run；需要中途停止能力的适配器必须覆写本方法。
    async fn send_with_events(
        &self,
        command: RunnerCommand,
        on_event: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, PortError> {
        let events = self.send(command).await?;
        for event in &events {
            on_event(event.clone())
                .map_err(|error| PortError::Failed(format!("runner_event_sink_failed:{error}")))?;
        }
        Ok(events)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
/// 跨核心与适配器边界传播的稳定错误分类。
///
/// 字符串负载保存机器可识别的具体原因码或经过控制的诊断信息。分类本身用于决定是否
/// 可重试、是否属于并发冲突以及是否应 fail-closed；入口层可以添加上下文，但不应把
/// 策略拒绝或结果未知统一塌缩成普通 I/O 错误。
pub enum PortError {
    /// 所需适配器或外部依赖当前不可用，例如 Runner 尚未配置。
    #[error("port_unavailable:{0}")]
    Unavailable(String),
    /// 请求与当前权威状态冲突，例如版本过期、重复消费或租约错配。
    #[error("port_conflict:{0}")]
    Conflict(String),
    /// 端口执行失败或输入不满足契约，且不属于可明确识别的状态冲突。
    #[error("port_failed:{0}")]
    Failed(String),
}

impl PortError {
    /// Map adapter errors to the storage taxonomy without collapsing Unknown or corruption into
    /// success. The mapping is deliberately conservative for unrecognized failure text.
    pub fn storage_class(&self) -> StorageErrorClass {
        match self {
            Self::Unavailable(_) => StorageErrorClass::Unavailable,
            Self::Conflict(_) => StorageErrorClass::Conflict,
            Self::Failed(reason) => {
                let lower = reason.to_ascii_lowercase();
                if lower.contains("result_unknown") || lower.contains("unknown") {
                    StorageErrorClass::ResultUnknown
                } else if lower.contains("corrupt")
                    || lower.contains("checksum")
                    || lower.contains("integrity")
                    || lower.contains("tamper")
                {
                    StorageErrorClass::Corrupt
                } else if lower.contains("empty") {
                    StorageErrorClass::Empty
                } else {
                    StorageErrorClass::Unknown
                }
            }
        }
    }

    pub fn into_storage_error(
        &self,
        code: impl Into<String>,
        source_cursor: Option<u64>,
    ) -> Result<StorageError, PortError> {
        StorageError::new(self.storage_class(), code, self.to_string(), source_cursor)
            .map_err(PortError::Failed)
    }
}

/// The product Broker consumes a committed, single-use permit before entering a handler.
#[async_trait]
pub trait ExecutionPermitVerifierPort: Send + Sync {
    async fn verify_and_consume(
        &self,
        request: &AuthorizedCapabilityRequest,
    ) -> Result<(), PortError>;
}

/// Model requests reserve capacity before contacting a provider. Unknown usage is never refunded.
#[async_trait]
pub trait ModelBudgetPort: Send + Sync {
    async fn reserve_prepared(
        &self,
        _prepared: &kiana_domain::PreparedModelCall,
    ) -> Result<kiana_domain::ModelCallPermit, PortError> {
        Err(PortError::Unavailable(
            "model_prepared_admission_unsupported".to_owned(),
        ))
    }
    async fn consume_prepared(
        &self,
        _prepared: &kiana_domain::PreparedModelCall,
        _permit: &kiana_domain::ModelCallPermit,
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "model_prepared_admission_unsupported".to_owned(),
        ))
    }
    async fn reserve(
        &self,
        run_id: RunId,
        request_id: kiana_domain::RequestId,
        tokens: u64,
    ) -> Result<(), PortError>;
    async fn settle(
        &self,
        run_id: RunId,
        request_id: kiana_domain::RequestId,
        tokens: Option<u64>,
    ) -> Result<(), PortError>;
}

mod model;
pub use model::*;

/// One validated signal accepted by an observability adapter.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "signal", content = "record", rename_all = "snake_case")]
pub enum ObservabilitySignalRecord {
    Log(ObservabilityRecord),
    Metric(MetricPoint),
    Trace(TraceSummary),
    Audit(AuditRecord),
    Health(HealthSnapshot),
}

impl ObservabilitySignalRecord {
    pub fn signal_kind(&self) -> SignalKind {
        match self {
            Self::Log(_) => SignalKind::Log,
            Self::Metric(_) => SignalKind::Metric,
            Self::Trace(_) => SignalKind::Trace,
            Self::Audit(_) => SignalKind::Audit,
            Self::Health(_) => SignalKind::Health,
        }
    }

    pub fn source_cursor(&self) -> u64 {
        match self {
            Self::Log(record) => record.source_cursor,
            Self::Metric(record) => record.source_cursor,
            Self::Trace(record) => record.source_cursor,
            Self::Audit(record) => record.source_cursor,
            Self::Health(record) => record.source_cursor,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Log(record) => {
                if record.signal != SignalKind::Log {
                    return Err("observability_signal_mismatch".to_owned());
                }
                record.validate()
            }
            Self::Metric(record) => record.validate(),
            Self::Trace(record) => record.validate(),
            Self::Audit(record) => record.validate(),
            Self::Health(record) => record.validate(),
        }
    }
}

/// Capabilities an adapter must advertise before core can depend on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ObservabilityCapabilities {
    pub durable: bool,
    pub flush: bool,
    pub cancellation: bool,
    pub max_records: usize,
}

impl Default for ObservabilityCapabilities {
    fn default() -> Self {
        Self {
            durable: false,
            flush: false,
            cancellation: false,
            max_records: 0,
        }
    }
}

/// Requirements for a caller that needs a particular observability guarantee.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservabilityRequirements {
    pub durable: bool,
    pub flush: bool,
    pub cancellation: bool,
    pub min_records: usize,
}

impl ObservabilityRequirements {
    pub const fn none() -> Self {
        Self {
            durable: false,
            flush: false,
            cancellation: false,
            min_records: 0,
        }
    }
}

/// Validate capability negotiation without silently degrading to an in-memory or best-effort
/// adapter.
pub fn require_observability_capabilities(
    actual: ObservabilityCapabilities,
    required: ObservabilityRequirements,
) -> Result<(), PortError> {
    if required.durable && !actual.durable {
        return Err(PortError::Unavailable(
            "observability_durable_unsupported".to_owned(),
        ));
    }
    if required.flush && !actual.flush {
        return Err(PortError::Unavailable(
            "observability_flush_unsupported".to_owned(),
        ));
    }
    if required.cancellation && !actual.cancellation {
        return Err(PortError::Unavailable(
            "observability_cancellation_unsupported".to_owned(),
        ));
    }
    if actual.max_records < required.min_records {
        return Err(PortError::Unavailable(
            "observability_capacity_unsupported".to_owned(),
        ));
    }
    Ok(())
}

/// Acknowledge a signal append/flush boundary. Acknowledgement is not an authority decision.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ObservabilityReceipt {
    pub signal: SignalKind,
    pub sequence: u64,
    pub source_cursor: u64,
}

/// A flush acknowledgement with no implication that an external backend accepted the data.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ObservabilityFlushAck {
    pub flushed_records: usize,
    pub flush_sequence: u64,
}

pub type FlushAck = ObservabilityFlushAck;

/// Aggregate signal port. It only stores/forwards already validated projections; it cannot grant
/// capability authority or invoke a Broker.
#[async_trait]
pub trait ObservabilityPort: Send + Sync {
    fn capabilities(&self) -> ObservabilityCapabilities;

    async fn append(
        &self,
        signal: ObservabilitySignalRecord,
    ) -> Result<ObservabilityReceipt, PortError>;

    async fn flush(&self) -> Result<ObservabilityFlushAck, PortError> {
        Err(PortError::Unavailable(
            "observability_flush_unsupported".to_owned(),
        ))
    }

    /// Cancel future writes. Existing committed facts remain untouched.
    async fn cancel(&self) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "observability_cancellation_unsupported".to_owned(),
        ))
    }

    async fn append_cancellable(
        &self,
        signal: ObservabilitySignalRecord,
        cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<ObservabilityReceipt, PortError> {
        if *cancellation.borrow() {
            return Err(PortError::Failed("observability_cancelled".to_owned()));
        }
        self.append(signal).await
    }
}

/// Dedicated trace boundary. Trace writes are projections and cannot change authorization.
#[async_trait]
pub trait TraceSink: Send + Sync {
    fn capabilities(&self) -> ObservabilityCapabilities;
    async fn record_trace(&self, trace: TraceSummary) -> Result<ObservabilityReceipt, PortError>;
    async fn flush_trace(&self) -> Result<ObservabilityFlushAck, PortError> {
        Err(PortError::Unavailable("trace_flush_unsupported".to_owned()))
    }
}

/// Dedicated metric boundary. Metric labels and catalog membership are validated before append.
#[async_trait]
pub trait MetricSink: Send + Sync {
    fn capabilities(&self) -> ObservabilityCapabilities;
    async fn record_metric(&self, metric: MetricPoint) -> Result<ObservabilityReceipt, PortError>;
    async fn flush_metric(&self) -> Result<ObservabilityFlushAck, PortError> {
        Err(PortError::Unavailable(
            "metric_flush_unsupported".to_owned(),
        ))
    }
}

/// Bounded, server-authenticated filters for a read-only audit projection.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditQueryRequest {
    pub source_cursor: kiana_domain::EventCursor,
    #[serde(default)]
    pub after_cursor: kiana_domain::EventCursor,
    pub limit: usize,
    #[serde(default)]
    pub action_kind: Option<AuditActionKind>,
    #[serde(default)]
    pub decision: Option<AuditDecision>,
    #[serde(default)]
    pub actor_ref: Option<String>,
    #[serde(default)]
    pub target_kind: Option<String>,
}

impl AuditQueryRequest {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.source_cursor == 0 {
            return Err(PortError::Failed(
                "audit_query_source_cursor_required".to_owned(),
            ));
        }
        if self.limit == 0 || self.limit > 1_000 {
            return Err(PortError::Failed("audit_query_limit_invalid".to_owned()));
        }
        if self.after_cursor > self.source_cursor {
            return Err(PortError::Conflict(
                "audit_query_cursor_out_of_range".to_owned(),
            ));
        }
        for (value, field, max) in [
            (self.actor_ref.as_deref(), "audit_query_actor", 256),
            (self.target_kind.as_deref(), "audit_query_target_kind", 128),
        ] {
            if let Some(value) = value {
                if value.trim().is_empty() || value.len() > max {
                    return Err(PortError::Failed(format!("{field}_invalid")));
                }
            }
        }
        Ok(())
    }
}

pub type AuditQuery = AuditQueryRequest;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditQueryPage {
    pub records: Vec<AuditRecord>,
    #[serde(default)]
    pub next_cursor: Option<kiana_domain::EventCursor>,
    pub source_cursor: kiana_domain::EventCursor,
    pub projection_version: u64,
}

impl AuditQueryPage {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.source_cursor == 0 || self.projection_version == 0 {
            return Err(PortError::Failed(
                "audit_query_page_metadata_required".to_owned(),
            ));
        }
        if self.records.len() > 1_000 {
            return Err(PortError::Failed("audit_query_page_limit".to_owned()));
        }
        for record in &self.records {
            record
                .validate()
                .map_err(|error| PortError::Failed(format!("audit_query_record:{error}")))?;
        }
        if self
            .next_cursor
            .is_some_and(|cursor| cursor > self.source_cursor)
        {
            return Err(PortError::Conflict(
                "audit_query_next_cursor_out_of_range".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Read-only audit query port. It does not expose raw RuntimeEvent data.
#[async_trait]
pub trait AuditQueryPort: Send + Sync {
    async fn query(&self, request: AuditQueryRequest) -> Result<AuditQueryPage, PortError>;
}

/// Health/readiness probe port. Probe output is a bounded projection, never an authority grant.
#[async_trait]
pub trait HealthProbePort: Send + Sync {
    async fn probe(&self) -> Result<HealthSnapshot, PortError>;
}

#[derive(Default)]
struct MemoryObservabilityState {
    records: Vec<ObservabilitySignalRecord>,
    capacity: usize,
    failure: Option<String>,
    cancelled: bool,
    flush_sequence: u64,
    health: Option<HealthSnapshot>,
}

/// In-memory fake used by remote CI fixtures. It is explicitly non-durable.
#[derive(Clone)]
pub struct MemoryObservabilitySink {
    state: Arc<Mutex<MemoryObservabilityState>>,
    capabilities: ObservabilityCapabilities,
}

impl std::fmt::Debug for MemoryObservabilitySink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryObservabilitySink")
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl MemoryObservabilitySink {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryObservabilityState {
                capacity,
                ..MemoryObservabilityState::default()
            })),
            capabilities: ObservabilityCapabilities {
                durable: false,
                flush: true,
                cancellation: true,
                max_records: capacity,
            },
        }
    }

    pub fn with_capabilities(capacity: usize, capabilities: ObservabilityCapabilities) -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryObservabilityState {
                capacity,
                ..MemoryObservabilityState::default()
            })),
            capabilities,
        }
    }

    pub fn inject_failure(&self, reason: impl Into<String>) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .failure = Some(reason.into());
    }

    pub fn fail_next(&self, reason: impl Into<String>) {
        self.inject_failure(reason);
    }

    pub fn records(&self) -> Vec<ObservabilitySignalRecord> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .records
            .clone()
    }

    pub fn set_health(&self, snapshot: HealthSnapshot) -> Result<(), PortError> {
        snapshot
            .validate()
            .map_err(|error| PortError::Failed(format!("health_snapshot:{error}")))?;
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .health = Some(snapshot);
        Ok(())
    }

    fn append_record(
        &self,
        signal: ObservabilitySignalRecord,
    ) -> Result<ObservabilityReceipt, PortError> {
        signal
            .validate()
            .map_err(|error| PortError::Failed(format!("observability_record:{error}")))?;
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(reason) = state.failure.take() {
            return Err(PortError::Failed(reason));
        }
        if state.cancelled {
            return Err(PortError::Failed("observability_cancelled".to_owned()));
        }
        if state.records.len() >= state.capacity {
            return Err(PortError::Conflict(
                "observability_capacity_exceeded".to_owned(),
            ));
        }
        state.records.push(signal.clone());
        Ok(ObservabilityReceipt {
            signal: signal.signal_kind(),
            sequence: state.records.len() as u64,
            source_cursor: signal.source_cursor(),
        })
    }

    fn flush_records(&self) -> Result<ObservabilityFlushAck, PortError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.capabilities.flush {
            return Err(PortError::Unavailable(
                "observability_flush_unsupported".to_owned(),
            ));
        }
        if let Some(reason) = state.failure.take() {
            return Err(PortError::Failed(reason));
        }
        state.flush_sequence = state.flush_sequence.saturating_add(1);
        Ok(ObservabilityFlushAck {
            flushed_records: state.records.len(),
            flush_sequence: state.flush_sequence,
        })
    }

    fn query_records(&self, request: &AuditQueryRequest) -> Result<AuditQueryPage, PortError> {
        request.validate()?;
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut records = state
            .records
            .iter()
            .filter_map(|signal| match signal {
                ObservabilitySignalRecord::Audit(record) => Some(record),
                _ => None,
            })
            .filter(|record| {
                record.source_cursor > request.after_cursor
                    && record.source_cursor <= request.source_cursor
                    && request
                        .action_kind
                        .is_none_or(|kind| record.action_kind == kind)
                    && request
                        .decision
                        .is_none_or(|decision| record.decision == decision)
                    && request
                        .actor_ref
                        .as_deref()
                        .is_none_or(|actor| record.actor_ref == actor)
                    && request
                        .target_kind
                        .as_deref()
                        .is_none_or(|target| record.target_kind == target)
            })
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = if records.len() > request.limit {
            records.truncate(request.limit);
            records.last().map(|record| record.source_cursor)
        } else {
            None
        };
        let page = AuditQueryPage {
            records,
            next_cursor,
            source_cursor: request.source_cursor,
            projection_version: state.records.len() as u64,
        };
        page.validate()?;
        Ok(page)
    }
}

#[async_trait]
impl ObservabilityPort for MemoryObservabilitySink {
    fn capabilities(&self) -> ObservabilityCapabilities {
        self.capabilities
    }

    async fn append(
        &self,
        signal: ObservabilitySignalRecord,
    ) -> Result<ObservabilityReceipt, PortError> {
        self.append_record(signal)
    }

    async fn flush(&self) -> Result<ObservabilityFlushAck, PortError> {
        self.flush_records()
    }

    async fn cancel(&self) -> Result<(), PortError> {
        if !self.capabilities.cancellation {
            return Err(PortError::Unavailable(
                "observability_cancellation_unsupported".to_owned(),
            ));
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancelled = true;
        Ok(())
    }
}

#[async_trait]
impl TraceSink for MemoryObservabilitySink {
    fn capabilities(&self) -> ObservabilityCapabilities {
        self.capabilities
    }

    async fn record_trace(&self, trace: TraceSummary) -> Result<ObservabilityReceipt, PortError> {
        self.append_record(ObservabilitySignalRecord::Trace(trace))
    }

    async fn flush_trace(&self) -> Result<ObservabilityFlushAck, PortError> {
        self.flush_records()
    }
}

#[async_trait]
impl MetricSink for MemoryObservabilitySink {
    fn capabilities(&self) -> ObservabilityCapabilities {
        self.capabilities
    }

    async fn record_metric(&self, metric: MetricPoint) -> Result<ObservabilityReceipt, PortError> {
        self.append_record(ObservabilitySignalRecord::Metric(metric))
    }

    async fn flush_metric(&self) -> Result<ObservabilityFlushAck, PortError> {
        self.flush_records()
    }
}

#[async_trait]
impl AuditQueryPort for MemoryObservabilitySink {
    async fn query(&self, request: AuditQueryRequest) -> Result<AuditQueryPage, PortError> {
        self.query_records(&request)
    }
}

#[async_trait]
impl HealthProbePort for MemoryObservabilitySink {
    async fn probe(&self) -> Result<HealthSnapshot, PortError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .health
            .clone()
            .ok_or_else(|| PortError::Unavailable("health_snapshot_unavailable".to_owned()))
    }
}

#[derive(Default)]
struct JsonlObservabilityState {
    lines: Vec<String>,
    records: Vec<ObservabilitySignalRecord>,
    capacity: usize,
    failure: Option<String>,
    cancelled: bool,
    flush_sequence: u64,
    health: Option<HealthSnapshot>,
}

/// JSONL fake that serializes every signal as one bounded line. It is non-durable until an
/// adapter explicitly proves its fsync/rotation contract; this fake never makes that claim.
#[derive(Clone)]
pub struct JsonlObservabilitySink {
    state: Arc<Mutex<JsonlObservabilityState>>,
    capabilities: ObservabilityCapabilities,
}

impl std::fmt::Debug for JsonlObservabilitySink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JsonlObservabilitySink")
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl JsonlObservabilitySink {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(JsonlObservabilityState {
                capacity,
                ..JsonlObservabilityState::default()
            })),
            capabilities: ObservabilityCapabilities {
                durable: false,
                flush: true,
                cancellation: true,
                max_records: capacity,
            },
        }
    }

    pub fn with_capabilities(capacity: usize, capabilities: ObservabilityCapabilities) -> Self {
        Self {
            state: Arc::new(Mutex::new(JsonlObservabilityState {
                capacity,
                ..JsonlObservabilityState::default()
            })),
            capabilities,
        }
    }

    pub fn inject_failure(&self, reason: impl Into<String>) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .failure = Some(reason.into());
    }

    pub fn fail_next(&self, reason: impl Into<String>) {
        self.inject_failure(reason);
    }

    pub fn lines(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .lines
            .clone()
    }

    pub fn records(&self) -> Vec<ObservabilitySignalRecord> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .records
            .clone()
    }

    pub fn set_health(&self, snapshot: HealthSnapshot) -> Result<(), PortError> {
        snapshot
            .validate()
            .map_err(|error| PortError::Failed(format!("health_snapshot:{error}")))?;
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .health = Some(snapshot);
        Ok(())
    }

    fn append_record(
        &self,
        signal: ObservabilitySignalRecord,
    ) -> Result<ObservabilityReceipt, PortError> {
        signal
            .validate()
            .map_err(|error| PortError::Failed(format!("observability_record:{error}")))?;
        let encoded = serde_json::to_string(&signal)
            .map_err(|_| PortError::Failed("observability_json_encode_failed".to_owned()))?;
        if encoded.len() > 256 * 1024 {
            return Err(PortError::Failed(
                "observability_json_line_too_large".to_owned(),
            ));
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(reason) = state.failure.take() {
            return Err(PortError::Failed(reason));
        }
        if state.cancelled {
            return Err(PortError::Failed("observability_cancelled".to_owned()));
        }
        if state.records.len() >= state.capacity {
            return Err(PortError::Conflict(
                "observability_capacity_exceeded".to_owned(),
            ));
        }
        state.lines.push(encoded);
        state.records.push(signal.clone());
        Ok(ObservabilityReceipt {
            signal: signal.signal_kind(),
            sequence: state.records.len() as u64,
            source_cursor: signal.source_cursor(),
        })
    }

    fn flush_records(&self) -> Result<ObservabilityFlushAck, PortError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.capabilities.flush {
            return Err(PortError::Unavailable(
                "observability_flush_unsupported".to_owned(),
            ));
        }
        if let Some(reason) = state.failure.take() {
            return Err(PortError::Failed(reason));
        }
        state.flush_sequence = state.flush_sequence.saturating_add(1);
        Ok(ObservabilityFlushAck {
            flushed_records: state.records.len(),
            flush_sequence: state.flush_sequence,
        })
    }

    fn query_records(&self, request: &AuditQueryRequest) -> Result<AuditQueryPage, PortError> {
        request.validate()?;
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut records = state
            .records
            .iter()
            .filter_map(|signal| match signal {
                ObservabilitySignalRecord::Audit(record) => Some(record),
                _ => None,
            })
            .filter(|record| {
                record.source_cursor > request.after_cursor
                    && record.source_cursor <= request.source_cursor
                    && request
                        .action_kind
                        .is_none_or(|kind| record.action_kind == kind)
                    && request
                        .decision
                        .is_none_or(|decision| record.decision == decision)
                    && request
                        .actor_ref
                        .as_deref()
                        .is_none_or(|actor| record.actor_ref == actor)
                    && request
                        .target_kind
                        .as_deref()
                        .is_none_or(|target| record.target_kind == target)
            })
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = if records.len() > request.limit {
            records.truncate(request.limit);
            records.last().map(|record| record.source_cursor)
        } else {
            None
        };
        let page = AuditQueryPage {
            records,
            next_cursor,
            source_cursor: request.source_cursor,
            projection_version: state.records.len() as u64,
        };
        page.validate()?;
        Ok(page)
    }
}

#[async_trait]
impl ObservabilityPort for JsonlObservabilitySink {
    fn capabilities(&self) -> ObservabilityCapabilities {
        self.capabilities
    }

    async fn append(
        &self,
        signal: ObservabilitySignalRecord,
    ) -> Result<ObservabilityReceipt, PortError> {
        self.append_record(signal)
    }

    async fn flush(&self) -> Result<ObservabilityFlushAck, PortError> {
        self.flush_records()
    }

    async fn cancel(&self) -> Result<(), PortError> {
        if !self.capabilities.cancellation {
            return Err(PortError::Unavailable(
                "observability_cancellation_unsupported".to_owned(),
            ));
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancelled = true;
        Ok(())
    }
}

#[async_trait]
impl TraceSink for JsonlObservabilitySink {
    fn capabilities(&self) -> ObservabilityCapabilities {
        self.capabilities
    }

    async fn record_trace(&self, trace: TraceSummary) -> Result<ObservabilityReceipt, PortError> {
        self.append_record(ObservabilitySignalRecord::Trace(trace))
    }

    async fn flush_trace(&self) -> Result<ObservabilityFlushAck, PortError> {
        self.flush_records()
    }
}

#[async_trait]
impl MetricSink for JsonlObservabilitySink {
    fn capabilities(&self) -> ObservabilityCapabilities {
        self.capabilities
    }

    async fn record_metric(&self, metric: MetricPoint) -> Result<ObservabilityReceipt, PortError> {
        self.append_record(ObservabilitySignalRecord::Metric(metric))
    }

    async fn flush_metric(&self) -> Result<ObservabilityFlushAck, PortError> {
        self.flush_records()
    }
}

#[async_trait]
impl AuditQueryPort for JsonlObservabilitySink {
    async fn query(&self, request: AuditQueryRequest) -> Result<AuditQueryPage, PortError> {
        self.query_records(&request)
    }
}

#[async_trait]
impl HealthProbePort for JsonlObservabilitySink {
    async fn probe(&self) -> Result<HealthSnapshot, PortError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .health
            .clone()
            .ok_or_else(|| PortError::Unavailable("health_snapshot_unavailable".to_owned()))
    }
}
