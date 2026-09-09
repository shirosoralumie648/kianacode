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
    AgentTemplate, ApprovalChallenge, ApprovalId, AuthorizedCapabilityRequest, BudgetLease,
    BudgetLeaseId, CapabilityGrant, CapabilityGrantId, CapabilityRequest, CapabilityResult, CellId,
    CellLifecycle, CellSpec, PendingApproval, RequestContext, RequestId, RetirementRecord, RunId,
    RuntimeEvent, SpawnPlan, SpawnPlanId, SupervisionLease, WorkFingerprint,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};

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

#[derive(Clone, Debug, PartialEq)]
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

#[async_trait]
/// 追加式运行事件事实源的最小端口。
///
/// 事件顺序、聚合版本和幂等键共同决定重试与恢复是否可信。基础方法用于兼容简单
/// 适配器；控制面的关键写入应优先使用同时带幂等与期望版本的
/// [`Self::append_idempotent_expected`]。trait 的存在不自动保证落盘、`fsync`、跨进程锁
/// 或损坏恢复，这些需要由具体适配器和测试证明。
pub trait EventStorePort: Send + Sync {
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
}

#[async_trait]
/// 审批挑战从暂存到一次性消费的状态存储端口。
///
/// 审批必须绑定请求内容和授权上下文，并遵循 `staged -> active -> consumed`（或过期、
/// 取消）的单向状态变化。审批 ID 不是可转借的通行证；适配器应核对主体、会话、项目、
/// 角色、部门、路径范围以及 proof，且不能重复消费。
pub trait ApprovalStorePort: Send + Sync {
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
