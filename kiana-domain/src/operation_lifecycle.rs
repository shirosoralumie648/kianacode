//! 部署操作（operation）生命周期合同和纯重放折叠器（reducer），属于最低层的 `kiana-domain`。
//!
//! 【模块位置与数据流】
//! ```text
//! 事件存储（EventStore）/ JournalFrame（事件日志帧）中的已提交操作事实（由上游读取并转换）
//!     -> OperationTransition（操作转换：绑定操作 ID、修订版（revision）、序号、源游标和证据引用）
//!     -> OperationJournal::append() / OperationJournal::replay()
//!     -> validate() 重算状态投影与摘要
//!     -> Core 的只读 facade 或后续部署模块
//! ```
//!
//! 本文件只定义版本化值对象、固定状态迁移和内存中的确定性折叠；它不读取 EventStore，
//! 不写入 JournalFrame，不取得操作租约/写入隔离（lease/fencing），不启动进程监督器
//! （supervisor）或操作系统进程（process），也不消费审批（approval）或派发能力（capability）。
//! 持久事实仍应来自上游 EventStore；journal 是可重建的当前状态投影，不能反过来当作事实来源。
//! 当前 Core 门面（facade）尚无生产调用者，DEP-05 的部署（deployment）接入仍为 partial；
//! Domain 测试夹具（fixture）证明合同样例，不代表启动或操作路径已经接线。
//!
//! 状态之间有先后约束：必须先完成 preflight，才能 drain 或 execute；drain 超时进入终态
//! `Unknown`，因为超时只证明等待窗口已过，不能证明底层进程确实停止。所有 digest 都是
//! 无密钥 SHA-256 内容指纹，不是身份签名、授权凭证或外部对象真实性证明。
//!
//! Serde 的 `Serialize` 把结构体/枚举写成 JSON 等 wire 数据，`Deserialize` 执行反向读取；
//! 枚举上的 `rename_all = "snake_case"` 固定协议中的变体名称，结构体上的
//! `deny_unknown_fields` 则拒绝当前版本未认识的输入字段。

use crate::{json_digest, EventCursor, OperationId, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// operation lifecycle 事实和 journal 数据在 wire 协议中的固定 schema 标识。
pub const OPERATION_LIFECYCLE_SCHEMA: &str = "kiana.operation-lifecycle.v1";
/// 本文件各生命周期数据传输对象（DTO）共用的当前主/次版本号（major/minor）。
pub const OPERATION_LIFECYCLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一个操作状态投影（projection）最多保存的转换数量；这是数据边界，不是重试次数。
pub const MAX_OPERATION_TRANSITIONS: usize = 256;
/// 原因和修订版标识等单行文本的 UTF-8 字节上限；多字节字符占用多个字节。
pub const MAX_OPERATION_TEXT_BYTES: usize = 512;
/// 单条状态转换（transition）最多引用的证据（evidence）项数，避免列表无界增长。
pub const MAX_OPERATION_EVIDENCE_REFS: usize = 32;

/// 当前操作所处的大阶段；它用于描述工作阶段，不代替更细的 `OperationState`。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    /// 创建后检查 revision、权限和前置条件的阶段。
    Preflight,
    /// 请求既有运行退出或转入安全状态的阶段；受 drain deadline 限制。
    Draining,
    /// 实际部署动作正在执行的阶段。
    Executing,
    /// 生命周期已结束；其中 `Unknown` 也属于终态，不能自动续跑。
    Terminal,
}

/// 单个部署 operation 在事件序列中的详细状态。
///
/// `Created` 是新建后的起点；`Completed`、`Failed`、`Cancelled` 和 `Unknown` 都是不可再追加
/// transition 的终态。`Unknown` 特意与 `Failed` 分开：系统无法确认结果时，不能假装失败后
/// 就可以安全重试，因为外部效果可能已经发生。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// operation 已登记，但尚未通过前置检查。
    Created,
    /// 前置检查已通过；可按规则开始 drain 或直接进入执行。
    Preflight,
    /// 正在等待既有运行退出或资源释放。
    Draining,
    /// 部署动作已由上游报告开始。
    Executing,
    /// 上游报告执行成功；本 enum 本身不验证外部目标状态。
    Completed,
    /// 上游报告可确定的失败结果。
    Failed,
    /// 上游报告操作已取消；不自动表示外部清理已完成。
    Cancelled,
    /// 结果无法确定；必须保留不确定性，不自动重试或恢复执行。
    Unknown,
}

impl OperationState {
    /// 判断此状态是否禁止继续追加 lifecycle transition。
    ///
    /// `OperationJournal::append()` 用它阻止 Completed/Failed/Cancelled/Unknown 被复活；
    /// 此处只作纯状态判断，不会写日志、取消进程或释放 lease。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }

    /// 将细粒度状态归入大阶段，供 journal 的 `current_phase` 保持一致。
    ///
    /// Created 和 Preflight 同属前置检查；所有结果状态归入 Terminal。调整映射会影响对外
    /// journal 投影和摘要一致性，因此不能把 `Unknown` 映射回可执行阶段。
    const fn phase(self) -> OperationPhase {
        match self {
            Self::Created | Self::Preflight => OperationPhase::Preflight,
            Self::Draining => OperationPhase::Draining,
            Self::Executing => OperationPhase::Executing,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unknown => {
                OperationPhase::Terminal
            }
        }
    }
}

/// 允许折叠器（reducer）尝试的状态转换（transition）种类；是否允许取决于当前状态和前置条件。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationTransitionKind {
    /// 前置检查完成，Created → Preflight。
    PreflightComplete,
    /// 开始排空，Preflight → Draining，并要求有未来的 drain deadline。
    DrainStart,
    /// 排空 deadline 已到但未确认完成，Draining → Unknown。
    DrainTimeout,
    /// 开始部署执行；只有 preflight 已完成后才能通过 journal 上下文校验。
    ExecuteStart,
    /// 上游报告执行完成，Executing → Completed；本 reducer 不核验外部目标状态。
    Complete,
    /// 上游报告可确定失败，可从 Preflight/Draining/Executing 进入 Failed。
    Fail,
    /// 操作被取消；此事实本身不等同于已完成资源清理。
    Cancel,
    /// 结果无法确认；用于保留不确定性并阻止自动复跑。
    ResultUnknown,
}

/// 一个阶段的绝对截止时间（deadline）声明。
///
/// 它只携带 Unix epoch 毫秒时间和阶段标签；不会启动计时器或读取系统时钟。drain/execute
/// 的转换校验会把它与 transition 自带的 `observed_at_unix_ms` 比较。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseDeadline {
    /// 必须等于当前 lifecycle schema。
    pub schema: String,
    /// deadline DTO 的版本。
    pub version: SchemaVersion,
    /// 截止时间适用于哪一个非终态阶段。
    pub phase: OperationPhase,
    /// Unix epoch 起算的毫秒数；0 被保留为无效/未设置值。
    pub deadline_unix_ms: u64,
    /// 对 schema/version/phase/time 字段计算的自摘要。
    pub deadline_digest: String,
}

impl PhaseDeadline {
    /// 为一个非终态阶段构造并校验 deadline。
    ///
    /// 调用方提供绝对时间；函数只检查非零、阶段和摘要格式，不读取当前时钟，也不判断时间
    /// 是否仍在未来。具体的“未来”约束要在 `validate_against()` 中与观察时间比较。
    pub fn new(phase: OperationPhase, deadline_unix_ms: u64) -> Result<Self, String> {
        let mut deadline = Self {
            schema: OPERATION_LIFECYCLE_SCHEMA.to_owned(),
            version: OPERATION_LIFECYCLE_VERSION,
            phase,
            deadline_unix_ms,
            deadline_digest: String::new(),
        };
        deadline.deadline_digest = deadline.digest();
        deadline.validate()?;
        Ok(deadline)
    }

    /// 校验 deadline 的 schema、版本、阶段和值摘要。
    ///
    /// Terminal 不允许 active deadline；失败返回稳定错误字符串，由 transition/journal 校验
    /// 继续向调用者传播，不触发超时动作。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LIFECYCLE_SCHEMA
            || self.version != OPERATION_LIFECYCLE_VERSION
            || self.deadline_unix_ms == 0
            || self.phase == OperationPhase::Terminal
        {
            return Err("operation_deadline_header_invalid".to_owned());
        }
        valid_digest(&self.deadline_digest, "operation_deadline_digest")?;
        if self.deadline_digest != self.digest() {
            return Err("operation_deadline_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算 deadline 内容指纹；摘要字段本身不参与，以避免自引用。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "phase": self.phase,
            "deadline_unix_ms": self.deadline_unix_ms,
        }))
    }
}

/// 一次带来源和前后状态的 operation 事实。
///
/// 本值对象把事件序号、EventStore source cursor、部署 revision、状态变化、原因、证据引用、
/// 观察时间和可选 deadline 绑定在同一份摘要中。构造时可验证字段与映射关系，但前置状态
/// 是否真的完成、cursor 是否来自已提交 EventLog、证据引用是否真实存在，要由 journal 上下文
/// 与上游事实读取者负责；本类型不是 EventStore event，也不会自行持久化。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationTransition {
    /// 固定 lifecycle schema 名称。
    pub schema: String,
    /// transition wire 版本。
    pub version: SchemaVersion,
    /// 这条事实所属 operation 的稳定 ID，不是 revision 或 EventStore cursor。
    pub operation_id: OperationId,
    /// 产生该操作时选定的 deployment revision 标识。
    pub revision_id: String,
    /// revision 内容摘要；同一 operation 后续 transition 不得换 revision。
    pub revision_digest: String,
    /// operation 内从 1 开始连续递增的 transition 序号；新 journal 的初始值为 0。
    pub sequence: u64,
    /// 对应源事件在 EventStore 中的位置；必须非零并严格向前推进。
    pub source_cursor: EventCursor,
    /// reducer 接受这条 transition 前的状态，用于防止在过期状态上套用事件。
    pub from_state: OperationState,
    /// 按 `kind` 与 `from_state` 推导出的目标状态。
    pub state: OperationState,
    /// `state` 所属的大阶段，必须与目标状态的映射一致。
    pub phase: OperationPhase,
    /// 这次迁移的原因类别，决定允许的状态边和 deadline 规则。
    pub kind: OperationTransitionKind,
    /// 面向诊断和上层展示的单行原因码/文本。
    pub reason: String,
    /// 上游提供的证据引用列表；当前校验允许空列表，非空引用也只校验格式/去重。
    pub evidence_refs: Vec<String>,
    /// 对 `evidence_refs` 列表计算的摘要，不证明引用目标存在。
    pub evidence_digest: String,
    /// 上游声称的观察时间（Unix epoch 毫秒）；本模块不自行读取时钟。
    pub observed_at_unix_ms: u64,
    /// 当前阶段的可选截止时间；DrainStart/DrainTimeout 必须携带对应 deadline。
    pub deadline: Option<PhaseDeadline>,
    /// 对 transition 其余字段计算的摘要；不是签名或 EventStore 提交回执。
    pub transition_digest: String,
}

impl OperationTransition {
    /// 按迁移种类和来源状态生成目标状态、计算摘要并校验 transition 自身。
    ///
    /// 【调用关系】上游事实适配器应先构造 transition，再交给 `OperationJournal::append()`；
    /// 当前仓库中生产路径未调用此构造器，fixture 用它生成可重复样例。
    ///
    /// 【输入】
    /// - `operation_id`、`revision_id`、`revision_digest`：固定操作和部署修订版身份。
    /// - `sequence`、`source_cursor`：本操作内的序号和上游事件存储游标，分别用于检查连续性/前进。
    /// - `from_state`、`kind`：当前状态声明和请求记录的迁移种类，由白名单推导目标状态。
    /// - `reason`、`evidence_refs`：上游给出的原因与证据引用；当前合同允许引用列表为空。
    /// - `observed_at_unix_ms`、`deadline`：上游观察时间和可选阶段截止时间，用于超时关系校验。
    ///
    /// 【输出】成功返回带当前 schema/version、推导后 `state`/`phase`、evidence digest 和
    /// transition digest 的值。
    ///
    /// 【失败情况】不支持的 kind/from-state 组合、空/越界 header、坏摘要、deadline 不一致或
    /// reason 不合规时返回稳定字符串。构造器不读取 journal，因此完整 preflight、序号连续性、
    /// cursor 递增等上下文条件留给 `append()` 检查；它也不写 EventStore 或执行部署动作。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        revision_id: impl Into<String>,
        revision_digest: impl Into<String>,
        sequence: u64,
        source_cursor: EventCursor,
        from_state: OperationState,
        kind: OperationTransitionKind,
        reason: impl Into<String>,
        evidence_refs: Vec<String>,
        observed_at_unix_ms: u64,
        deadline: Option<PhaseDeadline>,
    ) -> Result<Self, String> {
        let (state, phase) = expected_transition(kind, from_state)?;
        let mut transition = Self {
            schema: OPERATION_LIFECYCLE_SCHEMA.to_owned(),
            version: OPERATION_LIFECYCLE_VERSION,
            operation_id,
            revision_id: revision_id.into(),
            revision_digest: revision_digest.into(),
            sequence,
            source_cursor,
            from_state,
            state,
            phase,
            kind,
            reason: reason.into(),
            evidence_digest: evidence_digest(&evidence_refs),
            evidence_refs,
            observed_at_unix_ms,
            deadline,
            transition_digest: String::new(),
        };
        transition.transition_digest = transition.digest();
        transition.validate()?;
        Ok(transition)
    }

    /// 校验 transition 的头部、单条迁移映射、deadline、evidence 和自摘要。
    ///
    /// 这是单条记录校验，不知道此前 journal 的 preflight 标记或 active deadline；通过并不
    /// 单独表示可将其应用到某个 operation。上游通过 `append()` 后遇到错误时应按原错误处理，
    /// 本方法不会重试、记录日志或补偿外部效果。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LIFECYCLE_SCHEMA
            || self.version != OPERATION_LIFECYCLE_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.sequence == 0
            || self.source_cursor == 0
            || self.observed_at_unix_ms == 0
        {
            return Err("operation_transition_header_invalid".to_owned());
        }
        bounded(&self.revision_id, "operation_revision_id")?;
        valid_digest(&self.revision_digest, "operation_revision_digest")?;
        bounded(&self.reason, "operation_reason")?;
        validate_evidence(&self.evidence_refs, &self.evidence_digest)?;
        if let Some(deadline) = &self.deadline {
            deadline.validate()?;
            // Timeout 必须引用同一个 drain 窗口，不能借此伪造另一个阶段的 deadline。
            let deadline_phase_matches = deadline.phase == self.phase
                || (self.kind == OperationTransitionKind::DrainTimeout
                    && deadline.phase == OperationPhase::Draining);
            if !deadline_phase_matches {
                return Err("operation_deadline_phase_mismatch".to_owned());
            }
        }
        let (expected_state, expected_phase) = expected_transition(self.kind, self.from_state)?;
        if self.state != expected_state || self.phase != expected_phase {
            return Err("operation_transition_state_mismatch".to_owned());
        }
        if self.kind == OperationTransitionKind::DrainStart && self.deadline.is_none() {
            return Err("operation_drain_deadline_required".to_owned());
        }
        if self.kind == OperationTransitionKind::DrainTimeout && self.deadline.is_none() {
            return Err("operation_drain_timeout_deadline_required".to_owned());
        }
        valid_digest(&self.transition_digest, "operation_transition_digest")?;
        if self.transition_digest != self.digest() {
            return Err("operation_transition_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算并绑定 transition 的所有业务字段；`transition_digest` 本身不进入输入。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "revision_id": self.revision_id,
            "revision_digest": self.revision_digest,
            "sequence": self.sequence,
            "source_cursor": self.source_cursor,
            "from_state": self.from_state,
            "state": self.state,
            "phase": self.phase,
            "kind": self.kind,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
            "evidence_digest": self.evidence_digest,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "deadline": self.deadline,
        }))
    }
}

/// 某个 operation 当前状态和完整 transition 前缀的可重建投影。
///
/// `OperationJournal::new()` 从 Created 起步，`append()` 逐条校验并更新投影；`replay()` 用同样
/// 的 append 规则从头折叠给定序列。`transitions` 是计算 `current_state` 等字段的输入，
/// `EventStore` 才是外部持久事实源。本结构目前只在内存中存在，不提供事务、持久化或跨进程锁。
/// `Serialize`/`Deserialize` 供 wire/fixture 表示使用，未知字段由 Serde 拒绝。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationJournal {
    /// 固定 lifecycle schema 名称。
    pub schema: String,
    /// journal DTO 版本。
    pub version: SchemaVersion,
    /// 该 journal 对应的稳定 operation ID。
    pub operation_id: OperationId,
    /// 此 operation 固定绑定的 deployment revision ID。
    pub revision_id: String,
    /// 此操作固定绑定的修订版内容摘要。
    pub revision_digest: String,
    /// 所有已接受 transition 折叠后的当前状态。
    pub current_state: OperationState,
    /// 由 `current_state` 推导出的大阶段；验证时要求两者一致。
    pub current_phase: OperationPhase,
    /// preflight 是否已完成；`ExecuteStart` 的上下文校验仍以此标记为门槛。
    pub preflight_completed: bool,
    /// 已折叠 transition 的数量/当前序号；新 journal 为 0，后续从 1 连续递增。
    pub sequence: u64,
    /// 已折叠事实中最新的 EventStore source cursor；新 journal 尚无事实时为 0。
    pub source_cursor: EventCursor,
    /// 当前状态对应的最近原因文案。
    pub reason: String,
    /// 当前投影保存的 evidence 引用；`validate()` 检查其格式和自摘要。
    pub evidence_refs: Vec<String>,
    /// 当前 evidence 列表的摘要，不认证引用内容或来源。
    pub evidence_digest: String,
    /// 当前未结束阶段的 deadline；终态不应保留 deadline。
    pub active_deadline: Option<PhaseDeadline>,
    /// 从创建后按序接受的 transition 前缀，决定大部分当前投影字段。
    pub transitions: Vec<OperationTransition>,
    /// 对当前 journal 投影和 transition 列表计算的摘要。
    pub journal_digest: String,
}

impl OperationJournal {
    /// 创建一个尚未接受 transition 的 operation 投影。
    ///
    /// 【调用者】当前由 Domain fixtures 使用；预期也可由上游 adapter 为新操作建立投影。
    ///
    /// 【输入/输出】输入稳定 operation ID、revision ID 和 revision digest；成功返回
    /// `Created`/`Preflight`、sequence/cursor 为 0、无 active deadline 的初始 journal。
    /// `operation.created` 是创建时写入的初始 evidence ref，不代表 EventStore 已提交对应事件。
    ///
    /// 【失败与副作用】nil operation ID、revision 文本或摘要不合规时返回稳定错误字符串；不
    /// 分配 ID、不写存储、不获取 lease，也不启动任何 operation。
    pub fn new(
        operation_id: OperationId,
        revision_id: impl Into<String>,
        revision_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let evidence_refs = vec!["operation.created".to_owned()];
        let mut journal = Self {
            schema: OPERATION_LIFECYCLE_SCHEMA.to_owned(),
            version: OPERATION_LIFECYCLE_VERSION,
            operation_id,
            revision_id: revision_id.into(),
            revision_digest: revision_digest.into(),
            current_state: OperationState::Created,
            current_phase: OperationPhase::Preflight,
            preflight_completed: false,
            sequence: 0,
            source_cursor: 0,
            reason: "created".to_owned(),
            evidence_digest: evidence_digest(&evidence_refs),
            evidence_refs,
            active_deadline: None,
            transitions: Vec::new(),
            journal_digest: String::new(),
        };
        journal.journal_digest = journal.digest();
        journal.validate()?;
        Ok(journal)
    }

    /// 从初始 Created 投影开始，按输入迭代器顺序重放所有 transition。
    ///
    /// 【调用关系】Domain fixture 或上游 replay adapter 可提供 operation/revision 身份及有序
    /// transition 列表；每项都经 `append()`，因此共享相同的身份、连续序号、cursor、状态机和
    /// deadline 校验。成功返回重放后的内存投影。
    ///
    /// 【失败与限制】第一条不合法 transition 会停止并返回其稳定错误字符串。输入是一个从头
    /// 开始的 transition iterator，不是 expected persisted snapshot；此方法不与某个外部 journal
    /// 比较，也不读取 EventStore、补齐缺失事实或持久化重建结果。
    pub fn replay(
        operation_id: OperationId,
        revision_id: impl Into<String>,
        revision_digest: impl Into<String>,
        transitions: impl IntoIterator<Item = OperationTransition>,
    ) -> Result<Self, String> {
        let mut journal = Self::new(operation_id, revision_id, revision_digest)?;
        for transition in transitions {
            journal.append(transition)?;
        }
        Ok(journal)
    }

    /// 将一条新 transition 原地折叠进当前 journal。
    ///
    /// 【核心顺序】先校验现有 journal 和终态 fence，再校验单条 transition、operation/revision
    /// 身份、连续 sequence、严格递增 source cursor，最后由 `validate_against()` 检查状态机、
    /// preflight 和 deadline 条件，更新投影字段/transition 列表并重算 journal 摘要。
    ///
    /// 【失败与副作用】正常拒绝以稳定 `String` 返回；不重试、记日志、写 EventStore 或调用
    /// supervisor。此方法会修改传入的内存对象，不是事务或持久化提交。
    /// `Ok(())` 表示 transition 已被折叠到这个内存对象；它不表示外部部署效果成功或已持久提交。
    ///
    /// ⚠ 当前容量上限由最后的 `self.validate()` 检查，而不是在 append 前拒绝。正常状态机路径
    /// 不能累积到 256 条；但当前 `validate()` 对嵌套 transition 的检查不完整，畸形 journal
    /// 可能先被接受并到达该边界。此时追加第 257 条会在投影字段和列表已修改后返回
    /// `operation_transition_limit_or_sequence_invalid`，没有自动回滚；`Err` 不能被当作原状态未变。
    pub fn append(&mut self, transition: OperationTransition) -> Result<(), String> {
        self.validate()?;
        if self.current_state.is_terminal() {
            return Err("operation_terminal_immutable".to_owned());
        }
        transition.validate()?;
        if transition.operation_id != self.operation_id {
            return Err("operation_id_mismatch".to_owned());
        }
        if transition.revision_id != self.revision_id
            || transition.revision_digest != self.revision_digest
        {
            return Err("operation_revision_late_event".to_owned());
        }
        if transition.sequence != self.sequence.saturating_add(1) {
            return Err("operation_sequence_not_contiguous".to_owned());
        }
        if transition.source_cursor <= self.source_cursor {
            return Err("operation_source_cursor_regressed".to_owned());
        }
        let (state, preflight_completed, active_deadline) = validate_against(
            self.current_state,
            self.preflight_completed,
            self.active_deadline.as_ref(),
            &transition,
        )?;
        self.current_state = state;
        self.current_phase = state.phase();
        self.preflight_completed = preflight_completed;
        self.sequence = transition.sequence;
        self.source_cursor = transition.source_cursor;
        self.reason = transition.reason.clone();
        self.evidence_refs = transition.evidence_refs.clone();
        self.evidence_digest = transition.evidence_digest.clone();
        self.active_deadline = active_deadline;
        self.transitions.push(transition);
        self.journal_digest = self.digest();
        // 这是更新后的完整投影自检。若这里失败，前面的原地更新已经发生，不会自动恢复旧值。
        self.validate()
    }

    /// 从头重算 transition 链，并验证当前投影字段和 journal 摘要是否与该链自洽。
    ///
    /// 【调用者】由 `new()`、`append()` 的前后自检和 Core `validate_operation_journal()` 调用；
    /// 外层直接从 wire 解码的调用方也可以显式调用它。
    ///
    /// 校验顺序为：header/文本/evidence → 数量与序号 → active deadline → 每条 transition 的
    /// operation/revision/sequence/cursor 连续性与状态折叠 → 当前 state/phase/preflight/cursor/
    /// deadline 和重算投影比较 → 最终摘要。第一个不满足条件的环节返回专属稳定字符串。
    /// 这是结构和内部一致性检查，不会访问 EventStore、验证 source cursor 的提交存在性或认证 digest。
    ///
    /// ⚠ 当前代码的重放循环调用 `validate_against()`，但没有逐条调用
    /// `OperationTransition::validate()`；因此 wire 解码后的 journal 直接进入此方法时，嵌套
    /// transition 的 schema/version、reason/evidence 形状、自摘要及 expected state/phase 不会
    /// 全部重新校验。外层 digest 又是无密钥摘要，调用方可以重算它，不能把 `Ok(())` 当成
    /// 每条内层事实都经过完整验证。
    ///
    /// 当前代码还会独立验证顶层 reason/evidence_digest，却未将顶层 reason/evidence_refs 与最后
    /// 一条 transition 对照。因此自摘要正确仍不足以证明这两项来自同一条已提交 transition。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LIFECYCLE_SCHEMA
            || self.version != OPERATION_LIFECYCLE_VERSION
            || self.operation_id.as_uuid().is_nil()
        {
            return Err("operation_journal_header_invalid".to_owned());
        }
        bounded(&self.revision_id, "operation_revision_id")?;
        valid_digest(&self.revision_digest, "operation_revision_digest")?;
        bounded(&self.reason, "operation_reason")?;
        validate_evidence(&self.evidence_refs, &self.evidence_digest)?;
        if self.transitions.len() > MAX_OPERATION_TRANSITIONS
            || self.sequence != self.transitions.len() as u64
        {
            return Err("operation_transition_limit_or_sequence_invalid".to_owned());
        }
        if let Some(deadline) = &self.active_deadline {
            deadline.validate()?;
            if deadline.phase != self.current_phase || self.current_state.is_terminal() {
                return Err("operation_active_deadline_invalid".to_owned());
            }
        }
        let mut state = OperationState::Created;
        let mut preflight_completed = false;
        let mut active_deadline = None;
        // 新 journal 没有 transition；cursor 从 0 起，第一条有效 transition 必须严格大于 0。
        let mut cursor = 0;
        let mut expected_sequence = 1;
        for transition in &self.transitions {
            // sequence 必须从 1 开始无间隙；source cursor 可以跳号，但不允许重复或倒退。
            if transition.operation_id != self.operation_id
                || transition.revision_id != self.revision_id
                || transition.revision_digest != self.revision_digest
                || transition.sequence != expected_sequence
                || transition.source_cursor <= cursor
            {
                return Err("operation_transition_chain_invalid".to_owned());
            }
            let (next_state, next_preflight, next_deadline) = validate_against(
                state,
                preflight_completed,
                active_deadline.as_ref(),
                transition,
            )?;
            state = next_state;
            preflight_completed = next_preflight;
            active_deadline = next_deadline;
            cursor = transition.source_cursor;
            expected_sequence = expected_sequence.saturating_add(1);
        }
        if state != self.current_state
            || state.phase() != self.current_phase
            || preflight_completed != self.preflight_completed
            || cursor != self.source_cursor
            || active_deadline != self.active_deadline
        {
            return Err("operation_journal_projection_mismatch".to_owned());
        }
        // journal 的摘要包括最新投影、deadline 和完整 transition 列表；它只能发现非一致修改，
        // 不能证明 transition 曾写入 EventStore 或由可信 signer 生成。
        valid_digest(&self.journal_digest, "operation_journal_digest")?;
        if self.journal_digest != self.digest() {
            return Err("operation_journal_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算绑定 journal 投影和完整 transition 前缀的 SHA-256 指纹。
    ///
    /// `journal_digest` 自身不参与输入以避免自引用；无密钥摘要用于发现载荷漂移，不是签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "revision_id": self.revision_id,
            "revision_digest": self.revision_digest,
            "current_state": self.current_state,
            "current_phase": self.current_phase,
            "preflight_completed": self.preflight_completed,
            "sequence": self.sequence,
            "source_cursor": self.source_cursor,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
            "evidence_digest": self.evidence_digest,
            "active_deadline": self.active_deadline,
            "transitions": self.transitions,
        }))
    }
}

/// 返回某种 transition 在来源状态下唯一允许的目标状态和大阶段。
///
/// 这是状态机白名单的第一层检查：例如 drain timeout 只能从 Draining 进入 Unknown，
/// 不能声称进程已 Stopped；Complete 只能从 Executing 进入 Completed。需要依赖 journal
/// 上下文的 preflight/deadline 条件由 `validate_against()` 继续检查。非法组合以
/// `operation_transition_not_allowed` 返回，不会尝试修复或猜测调用方意图。它由 transition
/// 构造器和 `OperationTransition::validate()` 调用；它自身不访问 journal 或产生副作用。
fn expected_transition(
    kind: OperationTransitionKind,
    from: OperationState,
) -> Result<(OperationState, OperationPhase), String> {
    let expected = match kind {
        OperationTransitionKind::PreflightComplete if from == OperationState::Created => {
            (OperationState::Preflight, OperationPhase::Preflight)
        }
        OperationTransitionKind::DrainStart if from == OperationState::Preflight => {
            (OperationState::Draining, OperationPhase::Draining)
        }
        OperationTransitionKind::DrainTimeout if from == OperationState::Draining => {
            (OperationState::Unknown, OperationPhase::Terminal)
        }
        OperationTransitionKind::ExecuteStart
            if matches!(from, OperationState::Preflight | OperationState::Draining) =>
        {
            (OperationState::Executing, OperationPhase::Executing)
        }
        OperationTransitionKind::Complete if from == OperationState::Executing => {
            (OperationState::Completed, OperationPhase::Terminal)
        }
        OperationTransitionKind::Fail
            if matches!(
                from,
                OperationState::Preflight | OperationState::Draining | OperationState::Executing
            ) =>
        {
            (OperationState::Failed, OperationPhase::Terminal)
        }
        OperationTransitionKind::Cancel
            if matches!(
                from,
                OperationState::Preflight | OperationState::Draining | OperationState::Executing
            ) =>
        {
            (OperationState::Cancelled, OperationPhase::Terminal)
        }
        OperationTransitionKind::ResultUnknown
            if matches!(from, OperationState::Draining | OperationState::Executing) =>
        {
            (OperationState::Unknown, OperationPhase::Terminal)
        }
        _ => return Err("operation_transition_not_allowed".to_owned()),
    };
    Ok(expected)
}

/// 把一条 transition 与 journal 当前上下文比较，并推导下一组投影辅助状态。
///
/// 【调用者】`OperationJournal::append()` 用返回值更新 journal；`OperationJournal::validate()`
/// 从 Created 起重放每一条记录，也调用本函数以复用完全相同的状态规则。
///
/// 【输入/输出】输入当前 state、preflight 完成标记、active deadline 和待应用 transition；
/// 成功返回 `(next_state, next_preflight_completed, next_active_deadline)`。错误由 Core/上层
/// 原样传播，不重试，也不产生进程或存储副作用。
///
/// 【边界规则】from_state 必须等于当前状态；DrainStart 要求 preflight 完成且 deadline 晚于
/// transition 的观察时间；DrainTimeout 必须引用当前 active deadline，且观察时间已到期；
/// ExecuteStart 需要 preflight 完成；终态 transition 不得附带 deadline。这里比较的是调用方
/// 提供的时间字段，不读取当前时钟，也不证明排空或执行的外部动作真实发生。
fn validate_against(
    current_state: OperationState,
    preflight_completed: bool,
    active_deadline: Option<&PhaseDeadline>,
    transition: &OperationTransition,
) -> Result<(OperationState, bool, Option<PhaseDeadline>), String> {
    if transition.from_state != current_state {
        return Err("operation_from_state_mismatch".to_owned());
    }
    // 除下方明确设置/清空的迁移外，deadline 沿用当前阶段的 active deadline。
    let mut next_deadline = active_deadline.cloned();
    let mut next_preflight = preflight_completed;
    match transition.kind {
        OperationTransitionKind::PreflightComplete => {
            if preflight_completed {
                return Err("operation_preflight_repeated".to_owned());
            }
            if transition.deadline.is_some() {
                return Err("operation_preflight_deadline_unexpected".to_owned());
            }
            // preflight 完成只设置生命周期标记，不附带 deadline，也不代表开始了部署动作。
            next_preflight = true;
            next_deadline = None;
        }
        OperationTransitionKind::DrainStart => {
            if !preflight_completed {
                return Err("operation_preflight_required".to_owned());
            }
            let deadline = transition
                .deadline
                .as_ref()
                .ok_or_else(|| "operation_drain_deadline_required".to_owned())?;
            // 避免 drain 事件声称一个已经到期的窗口，否则随后的 timeout 关系无法成立。
            if deadline.phase != OperationPhase::Draining
                || deadline.deadline_unix_ms <= transition.observed_at_unix_ms
            {
                return Err("operation_drain_deadline_invalid".to_owned());
            }
            next_deadline = Some(deadline.clone());
        }
        OperationTransitionKind::DrainTimeout => {
            let active =
                active_deadline.ok_or_else(|| "operation_drain_deadline_missing".to_owned())?;
            let deadline = transition
                .deadline
                .as_ref()
                .ok_or_else(|| "operation_drain_timeout_deadline_required".to_owned())?;
            // 只能在同一个排空 deadline 到期后形成 Unknown；超时不能推导出已停止。
            if deadline != active || transition.observed_at_unix_ms < deadline.deadline_unix_ms {
                return Err("operation_drain_timeout_not_due".to_owned());
            }
            next_deadline = None;
        }
        OperationTransitionKind::ExecuteStart => {
            if !preflight_completed {
                return Err("operation_preflight_required".to_owned());
            }
            // execution deadline 可选；若有则必须属于执行阶段且晚于观测时刻。
            if let Some(deadline) = &transition.deadline {
                if deadline.phase != OperationPhase::Executing
                    || deadline.deadline_unix_ms <= transition.observed_at_unix_ms
                {
                    return Err("operation_execute_deadline_invalid".to_owned());
                }
                next_deadline = Some(deadline.clone());
            } else {
                next_deadline = None;
            }
        }
        OperationTransitionKind::Complete
        | OperationTransitionKind::Fail
        | OperationTransitionKind::Cancel
        | OperationTransitionKind::ResultUnknown => {
            // 结束或不确定后清除活动 deadline，防止终态继续显示为正在计时。
            if transition.deadline.is_some() {
                return Err("operation_terminal_deadline_unexpected".to_owned());
            }
            next_deadline = None;
        }
    }
    Ok((transition.state, next_preflight, next_deadline))
}

/// 检查 evidence 引用列表的数量、单项文本、重复项和列表摘要。
///
/// 失败返回稳定的 evidence 错误码，由 transition/journal 校验传播。当前合同允许 `refs` 为空；
/// 因此成功只说明空列表或所给列表的摘要自洽，不代表每个引用目标存在、可读取或已提交。
fn validate_evidence(refs: &[String], digest: &str) -> Result<(), String> {
    if refs.len() > MAX_OPERATION_EVIDENCE_REFS {
        return Err("operation_evidence_limit".to_owned());
    }
    let mut seen = BTreeSet::new();
    for value in refs {
        bounded(value, "operation_evidence_ref")?;
        if !seen.insert(value) {
            return Err("operation_evidence_duplicate".to_owned());
        }
    }
    valid_digest(digest, "operation_evidence_digest")?;
    if digest != evidence_digest(refs) {
        return Err("operation_evidence_digest_mismatch".to_owned());
    }
    Ok(())
}

/// 对引用数组整体计算确定性摘要；元素顺序属于摘要输入，列表为空时也会产生合法摘要。
fn evidence_digest(refs: &[String]) -> String {
    json_digest(&serde_json::json!(refs))
}

/// 限制原因码、ID 和 evidence 引用为非空单行文本且不超过 512 个 UTF-8 字节。
///
/// NUL/CR/LF 会被拒绝，避免字段含控制字符或额外日志行。错误码以调用方传入的 field 名开头。
fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_OPERATION_TEXT_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 仅检查 `sha256:` 前缀和后续 64 位十六进制外形，不验证摘要引用的外部内容。
///
/// 十六进制允许大小写；失败时以调用方字段名生成稳定错误字符串。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
