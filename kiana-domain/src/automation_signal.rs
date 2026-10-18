//! 本文件定义 AUT-18 的暂停/恢复 signal、审批、checkpoint 元数据与单次消费契约。
//!
//! # 在系统中的位置
//!
//! ```text
//! 当前可见调用边界：
//! 调用方准备 AutomationSignalRequest / AutomationSignalApproval / AutomationCheckpoint
//!     -> 本文件校验 schema / 文本 / 摘要格式 / 字段组合 / 状态转移
//!     -> plan_automation_signal() 按固定拒绝顺序判定下一步事实（纯函数，不写库）
//!     -> AutomationSignalLedger::apply() 单次落库：signal_id dedupe、sequence 连续、
//!        审批单次消费、终态不可复活
//!     -> Core 的 automation_signal admission 分类稳定错误码
//! ```
//!
//! 当前 workspace 的生产 Rust 源码中没有找到这个 validator、计划函数或 ledger 的实际调用者；
//! 仓库状态账本也把 AUT-18 限定为 source-level、partial 合约。因此本文件不是现行 worker
//! pause/resume 执行链：它不验证审批人真实身份、不查 EventStore、不阻止别的写入路径创建同名
//! signal、不暂停/恢复线程，也不创建 checkpoint。字段可通过校验不等于事实已被授权、持久化或消费。
//!
//! 摘要使用无密钥 SHA-256，只能检查字段在同一结构中的摘要是否同步；提供者可以自行重算，不能
//! 由此认证 owner、approver、action、path、authority epoch 或 checkpoint 的真实性。
//!
//! # 本切片的拒绝语义（先拒绝，后成功）
//!
//! * 审批人越权：`OwnerAgent`（执行方）与 `Auditor`（只读）角色不能签发 pause/resume 审批
//!   （`automation_signal_approver_unauthorized`），且审批人与 owner 相同即自批
//!   （`automation_signal_approval_self_approval`）；
//! * 参数/path/epoch 变化：owner、definition、action、path scope、authority epoch 与已记录事实
//!   或审批不符时逐项拒绝；只有“带匹配 checkpoint 的重启恢复”才允许 authority epoch 前进；
//! * 重复 signal / approval：同 `signal_id` 同内容只做幂等重放，不同内容冲突；同一审批摘要只能
//!   支撑一次 resume，第二次消费被拒；
//! * UI 自带 context：请求携带 `ui_context_digest` 一律拒绝
//!   （`automation_signal_ui_context_forbidden`），计划函数不接收任何模型/UI 上下文；
//! * checkpoint：pause 与 resume 都必须携带 checkpoint 元数据，且 owner/definition/action/path/
//!   sequence 必须与请求一致；缺失或漂移都拒绝，因此“重启后仅凭自述恢复”不成立；
//! * 派发：`AutomationSignalPlan::dispatch_permitted` 恒为 `false`，本契约永不派发 effect，
//!   真实执行只能走既有 ControlPlane/Broker 脊柱。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 此类 signal fact 的固定 wire schema 名称。
pub const AUTOMATION_SIGNAL_SCHEMA: &str = "kiana.automation-signal.v1";

/// pause/resume signal 请求的固定 wire schema 名称。
pub const AUTOMATION_SIGNAL_REQUEST_SCHEMA: &str = "kiana.automation-signal-request.v1";

/// pause/resume 审批的固定 wire schema 名称。
pub const AUTOMATION_SIGNAL_APPROVAL_SCHEMA: &str = "kiana.automation-signal-approval.v1";

/// checkpoint 元数据的固定 wire schema 名称。
pub const AUTOMATION_CHECKPOINT_SCHEMA: &str = "kiana.automation-checkpoint.v1";

/// 一条 signal fact 声称的状态；枚举只限制可用值，跨记录顺序由计划函数与 ledger 强制。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSignalState {
    /// execution 暂停的事实声明。
    Paused,
    /// execution 恢复的事实声明；必须绑定已消费的审批摘要。
    Resumed,
    /// signal 已消费的终态声明；必须与 terminal state 一致，不能再次被恢复。
    Consumed,
}

/// 请求方想对 execution 做的事；它决定下一步事实的目标状态。
///
/// 枚举只描述意图，不证明请求方有权做这件事：`Pause` 与 `Consume` 复用既有权限，`Resume` 必须
/// 携带一条未过期、未消费、绑定同一 pause signal 的审批。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSignalKind {
    /// 请求暂停；必须携带 checkpoint，且不得携带审批。
    Pause,
    /// 请求恢复；必须携带审批与 checkpoint，且已有事实必须是 `Paused`。
    Resume,
    /// 请求把已恢复的 signal 消费掉；必须引用恢复时使用的同一审批摘要。
    Consume,
}

/// 审批人角色；只有 `Operator` 与 `Supervisor` 可以签发 signal 审批。
///
/// `OwnerAgent` 是执行方的角色，给自己签发恢复审批等于自批；`Auditor` 只读，不能授予执行权限。
/// 这个分类是拒绝面的一部分，不证明角色分配本身正确（角色来源由 ControlPlane 负责）。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationApproverRole {
    /// 运维/操作角色；有权限签发 pause/resume 审批。
    Operator,
    /// 主管角色；有权限签发 pause/resume 审批。
    Supervisor,
    /// 执行方角色；无权限签发自己的恢复审批。
    OwnerAgent,
    /// 审计角色；只读，无权限授予执行权限。
    Auditor,
}

impl AutomationApproverRole {
    /// 该角色是否有权签发 pause/resume 审批；`OwnerAgent`/`Auditor` 返回 false。
    pub fn may_authorize_signals(self) -> bool {
        matches!(self, Self::Operator | Self::Supervisor)
    }
}

/// 一个 pause/resume/consume signal 的单条绑定事实。
///
/// Serde 的 `deny_unknown_fields` 会拒绝未声明 JSON 字段，避免不同版本对同一载荷作不同解释。
/// `validate()` 只检查本结构本身；它不读取旧 fact，因此不能保证 signal_id 从未出现过，也不能
/// 验证 `signal_sequence` 连续、当前 epoch 有效、owner 对应真实 approver 或 action/path 摘要
/// 对应实际请求。跨记录的顺序、dedupe 与单次消费由 `plan_automation_signal()` 与
/// [`AutomationSignalLedger`] 强制。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSignalFact {
    /// 必须精确等于 `AUTOMATION_SIGNAL_SCHEMA`。
    pub schema: String,
    /// 关联的执行 ID；这里只检查文本格式，不在运行 registry 中查询该 execution。
    pub execution_id: String,
    /// 这条 signal 的标识；跨记录的唯一性由 ledger 的 signal_id dedupe 保证。
    pub signal_id: String,
    /// 同一 execution 内严格递增的 signal 序号；只要求非零，连续性由 ledger 检查。
    pub signal_sequence: u64,
    /// 声称拥有此 signal 的 actor ID；只检查文本，不确认实际身份或批准关系。
    pub owner_id: String,
    /// 工作流定义摘要；重启恢复时用它证明“还是同一个定义”，本文件不校验定义内容。
    pub definition_digest: String,
    /// 受控 action 内容的 SHA-256 格式摘要；此处不接收 action 本体来重新计算。
    pub action_digest: String,
    /// 允许写入的路径范围摘要；这里只检查摘要格式，不检查实际文件路径。
    pub path_scope_digest: String,
    /// 创建事实时声称的 authority epoch；只要求非零，不与当前 ControlPlane epoch 比较。
    pub authority_epoch: u64,
    /// checkpoint 序号；只要求非零，不读取 checkpoint store 或检查序号连续性。
    pub checkpoint_sequence: u64,
    /// 这条 fact 声称的状态；本 validator 只检查单条字段组合，不限制前后顺序。
    pub state: AutomationSignalState,
    /// 恢复/消费事实绑定的审批摘要；只有 `Paused` 不带审批，其余状态必须携带。
    pub approval_digest: Option<String>,
    /// 冗余的一致性标记；必须且只在 state 为 Consumed 时为 true。
    pub consumed: bool,
    /// 对本结构其余字段的 `sha256:` 摘要；不能认证创建者，也不防止提供者修改后重算。
    pub signal_digest: String,
}

impl AutomationSignalFact {
    /// 构造一条 signal fact，自动计算自摘要并做一次完整校验。
    ///
    /// 构造成功只表示 DTO 自洽；它不查 ledger、不校验 signal 序号连续、不消费审批，也不派发。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: impl Into<String>,
        signal_id: impl Into<String>,
        signal_sequence: u64,
        owner_id: impl Into<String>,
        definition_digest: impl Into<String>,
        action_digest: impl Into<String>,
        path_scope_digest: impl Into<String>,
        authority_epoch: u64,
        checkpoint_sequence: u64,
        state: AutomationSignalState,
        approval_digest: Option<String>,
    ) -> Result<Self, &'static str> {
        let mut fact = Self {
            schema: AUTOMATION_SIGNAL_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            signal_id: signal_id.into(),
            signal_sequence,
            owner_id: owner_id.into(),
            definition_digest: definition_digest.into(),
            action_digest: action_digest.into(),
            path_scope_digest: path_scope_digest.into(),
            authority_epoch,
            checkpoint_sequence,
            state,
            approval_digest,
            consumed: matches!(state, AutomationSignalState::Consumed),
            signal_digest: String::new(),
        };
        fact.recompute_digest();
        fact.validate()?;
        Ok(fact)
    }

    /// 校验一条 signal fact 的 schema、标识格式、非零计数、审批绑定、消费标记和自摘要。
    ///
    /// 成功只表示单条 DTO 自洽。字段文本最长 512 UTF-8 字节且不允许任何空白字符；digest 必须是
    /// `sha256:` 加 64 个十六进制字符。`authority_epoch`/`checkpoint_sequence` 的非零检查没有
    /// 连接当前 authority 或持久 checkpoint 记录。任何条件失败统一返回
    /// `automation_signal_fact_invalid`；函数不写状态、日志或 EventStore，也不会触发 pause/resume。
    ///
    /// `consumed == (state == Consumed)` 与 `approval_digest.is_some() != Paused` 只保证同一 fact
    /// 的字段一致，不能证明同一 signal_id 没有在另一条事实中重复消费；每种状态值作为单条 fact
    /// 都可以通过校验。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_SIGNAL_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_text(&self.signal_id)
            || !valid_text(&self.owner_id)
            || !valid_digest(&self.definition_digest)
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.path_scope_digest)
            || self.signal_sequence == 0
            || self.authority_epoch == 0
            || self.checkpoint_sequence == 0
            || self.approval_digest.is_some() == matches!(self.state, AutomationSignalState::Paused)
            || self
                .approval_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
            || self.consumed != matches!(self.state, AutomationSignalState::Consumed)
            || !valid_digest(&self.signal_digest)
            || self.signal_digest != self.digest()
        {
            return Err("automation_signal_fact_invalid");
        }
        Ok(())
    }

    /// 按固定字段集合重算 `signal_digest`；调用方修改任何可改字段后必须调用它。
    ///
    /// 摘要无密钥，字段提供者可以在修改事实后重新计算，不能证明事实来源可信。
    pub fn recompute_digest(&mut self) {
        self.signal_digest = self.digest();
    }

    /// 对除 `signal_digest` 外全部字段计算 JSON 摘要，避免把摘要字段自身纳入输入形成自引用。
    /// 摘要没有密钥，因此不是 approver 身份证明或授权签名。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "signal_id": self.signal_id,
            "signal_sequence": self.signal_sequence,
            "owner_id": self.owner_id,
            "definition_digest": self.definition_digest,
            "action_digest": self.action_digest,
            "path_scope_digest": self.path_scope_digest,
            "authority_epoch": self.authority_epoch,
            "checkpoint_sequence": self.checkpoint_sequence,
            "state": self.state,
            "approval_digest": self.approval_digest,
            "consumed": self.consumed,
        }))
    }
}

/// 一次 pause/resume/consume 请求：绑定 execution、signal 序号、owner、definition/action/path、
/// authority epoch、checkpoint 序号与（恢复时的）审批摘要。
///
/// 请求是**意图**而不是事实：它自证摘要，但不代表 pause/resume 已被记录、审批仍有效或
/// checkpoint 仍然对应同一个 run。`ui_context_digest` 只用于拒绝：任何非空值都表示入口试图把
/// UI/模型自带的上下文带进 signal 决策，必须整体拒绝，而不是合并进计划。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSignalRequest {
    /// 必须精确匹配 `AUTOMATION_SIGNAL_REQUEST_SCHEMA`。
    pub schema: String,
    /// 目标执行 ID；必须与已记录事实完全一致，否则拒绝。
    pub execution_id: String,
    /// 本次 signal 标识；同一 execution 内不得复用不同内容。
    pub signal_id: String,
    /// 同一 execution 内严格递增且连续的 signal 序号；第一个 signal 必须是 1。
    pub signal_sequence: u64,
    /// 请求意图：Pause / Resume / Consume。
    pub kind: AutomationSignalKind,
    /// 声称拥有该 execution 的 actor；必须与已记录事实和审批一致。
    pub owner_id: String,
    /// 工作流定义摘要；重启恢复时必须与 checkpoint 和已记录事实一致。
    pub definition_digest: String,
    /// action 内容摘要；参数漂移在这里体现为与已记录事实/审批不符。
    pub action_digest: String,
    /// 路径范围摘要；path 漂移在这里体现为与已记录事实/审批/checkpoint 不符。
    pub path_scope_digest: String,
    /// 本次请求声称的 authority epoch；不得低于已记录事实与 checkpoint 的 epoch。
    pub authority_epoch: u64,
    /// 请求声明要绑定/恢复到的 checkpoint 序号。
    pub checkpoint_sequence: u64,
    /// 恢复/消费请求引用的审批摘要；Pause 必须为 None，Resume/Consume 必须为 Some。
    pub approval_digest: Option<String>,
    /// 入口自带上下文摘要；任何非空值都被拒绝（`automation_signal_ui_context_forbidden`）。
    pub ui_context_digest: Option<String>,
    /// 请求原因文本；不能为空。
    pub reason: String,
    /// 对本请求其他字段的 `sha256:` 摘要；不是签名。
    pub request_digest: String,
}

impl AutomationSignalRequest {
    /// 构造并校验一条 signal 请求；形状不合法时返回稳定错误码。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: impl Into<String>,
        signal_id: impl Into<String>,
        signal_sequence: u64,
        kind: AutomationSignalKind,
        owner_id: impl Into<String>,
        definition_digest: impl Into<String>,
        action_digest: impl Into<String>,
        path_scope_digest: impl Into<String>,
        authority_epoch: u64,
        checkpoint_sequence: u64,
        approval_digest: Option<String>,
        ui_context_digest: Option<String>,
        reason: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let mut request = Self {
            schema: AUTOMATION_SIGNAL_REQUEST_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            signal_id: signal_id.into(),
            signal_sequence,
            kind,
            owner_id: owner_id.into(),
            definition_digest: definition_digest.into(),
            action_digest: action_digest.into(),
            path_scope_digest: path_scope_digest.into(),
            authority_epoch,
            checkpoint_sequence,
            approval_digest,
            ui_context_digest,
            reason: reason.into(),
            request_digest: String::new(),
        };
        request.recompute_digest();
        request.validate()?;
        Ok(request)
    }

    /// 校验 schema、身份字段、非零计数、审批绑定形状与自摘要，并拒绝入口自带上下文。
    ///
    /// 【拒绝顺序】先形状，再 `ui_context_digest`（`automation_signal_ui_context_forbidden`），
    /// 再审批绑定形状（`automation_signal_request_approval_mismatch`），最后自摘要。
    /// 【输出】通过时 `Ok(())`；不读取 ledger、不验证审批真伪、不激活 checkpoint，也没有副作用。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_SIGNAL_REQUEST_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_text(&self.signal_id)
            || !valid_text(&self.owner_id)
            || !valid_digest(&self.definition_digest)
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.path_scope_digest)
            || self.signal_sequence == 0
            || self.authority_epoch == 0
            || self.checkpoint_sequence == 0
            || !valid_text(&self.reason)
            || !valid_digest(&self.request_digest)
            || self.request_digest != self.digest()
        {
            return Err("automation_signal_request_invalid");
        }
        if self.ui_context_digest.is_some() {
            return Err("automation_signal_ui_context_forbidden");
        }
        let approval_expected = !matches!(self.kind, AutomationSignalKind::Pause);
        if self.approval_digest.is_some() != approval_expected
            || self
                .approval_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return Err("automation_signal_request_approval_mismatch");
        }
        Ok(())
    }

    /// 按固定字段集合重算请求摘要；修改字段后必须调用。
    pub fn recompute_digest(&mut self) {
        self.request_digest = self.digest();
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "signal_id": self.signal_id,
            "signal_sequence": self.signal_sequence,
            "kind": self.kind,
            "owner_id": self.owner_id,
            "definition_digest": self.definition_digest,
            "action_digest": self.action_digest,
            "path_scope_digest": self.path_scope_digest,
            "authority_epoch": self.authority_epoch,
            "checkpoint_sequence": self.checkpoint_sequence,
            "approval_digest": self.approval_digest,
            "ui_context_digest": self.ui_context_digest,
            "reason": self.reason,
        }))
    }
}

/// 一条绑定到具体 pause signal 的 resume 审批。
///
/// 审批只说明“谁在什么时间窗口内授权恢复哪次暂停”，不携带执行权限本身：它必须与请求的
/// owner/definition/action/path/authority epoch 逐项一致，并且在 ledger 中只能被消费一次。
/// 审批人不得是 owner（自批拒绝），角色必须是 `Operator`/`Supervisor`。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSignalApproval {
    /// 必须精确匹配 `AUTOMATION_SIGNAL_APPROVAL_SCHEMA`。
    pub schema: String,
    /// 审批标识；只检查文本，不查审批存储。
    pub approval_id: String,
    /// 被恢复的 execution；必须与请求和已记录 pause 一致。
    pub execution_id: String,
    /// 执行方 owner；审批人不得与之相同。
    pub owner_id: String,
    /// 审批人标识；这里只检查文本与“不等于 owner”。
    pub approver_id: String,
    /// 审批人角色；只有 Operator/Supervisor 有权签发。
    pub approver_role: AutomationApproverRole,
    /// 审批类型；目前只允许 `Resume`（`Consume` 复用恢复时的同一审批）。
    pub kind: AutomationSignalKind,
    /// 被恢复的 definition 摘要；必须与请求一致。
    pub definition_digest: String,
    /// 被恢复的 action 摘要；必须与请求一致。
    pub action_digest: String,
    /// 被恢复的 path scope 摘要；必须与请求一致。
    pub path_scope_digest: String,
    /// 这条审批针对的 pause signal；必须与已记录的暂停事实一致。
    pub pause_signal_id: String,
    /// 审批绑定的 authority epoch；必须与恢复请求的 epoch 相同（重启后必须重新签发）。
    pub authority_epoch: u64,
    /// 审批版本号；只要求非零。
    pub approval_epoch: u64,
    /// 审批签发 tick；只要求非零且早于过期点。
    pub issued_tick: u64,
    /// 审批过期 tick；必须严格大于 `issued_tick`，过期后不能恢复。
    pub expires_at_tick: u64,
    /// 对本审批其他字段的 `sha256:` 摘要；不是签名。
    pub approval_digest: String,
}

impl AutomationSignalApproval {
    /// 构造并校验一条恢复审批；形状不合法时返回稳定错误码。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        approval_id: impl Into<String>,
        execution_id: impl Into<String>,
        owner_id: impl Into<String>,
        approver_id: impl Into<String>,
        approver_role: AutomationApproverRole,
        kind: AutomationSignalKind,
        definition_digest: impl Into<String>,
        action_digest: impl Into<String>,
        path_scope_digest: impl Into<String>,
        pause_signal_id: impl Into<String>,
        authority_epoch: u64,
        approval_epoch: u64,
        issued_tick: u64,
        expires_at_tick: u64,
    ) -> Result<Self, &'static str> {
        let mut approval = Self {
            schema: AUTOMATION_SIGNAL_APPROVAL_SCHEMA.to_owned(),
            approval_id: approval_id.into(),
            execution_id: execution_id.into(),
            owner_id: owner_id.into(),
            approver_id: approver_id.into(),
            approver_role,
            kind,
            definition_digest: definition_digest.into(),
            action_digest: action_digest.into(),
            path_scope_digest: path_scope_digest.into(),
            pause_signal_id: pause_signal_id.into(),
            authority_epoch,
            approval_epoch,
            issued_tick,
            expires_at_tick,
            approval_digest: String::new(),
        };
        approval.recompute_digest();
        approval.validate()?;
        Ok(approval)
    }

    /// 校验 schema、身份/摘要形状、非零计数、有效期窗口、类型与拒绝面。
    ///
    /// 【拒绝顺序】形状 → 自批（`automation_signal_approval_self_approval`）→ 角色越权
    /// （`automation_signal_approver_unauthorized`）→ 有效期窗口（`automation_signal_approval_window_invalid`）
    /// → 类型（`automation_signal_approval_kind_drift`）→ 自摘要。
    /// 【输出】通过时 `Ok(())`；本函数不读取当前时间、不查审批存储、不消费审批，也不授予执行权限。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_SIGNAL_APPROVAL_SCHEMA
            || !valid_text(&self.approval_id)
            || !valid_text(&self.execution_id)
            || !valid_text(&self.owner_id)
            || !valid_text(&self.approver_id)
            || !valid_text(&self.pause_signal_id)
            || !valid_digest(&self.definition_digest)
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.path_scope_digest)
            || self.authority_epoch == 0
            || self.approval_epoch == 0
            || self.issued_tick == 0
            || self.expires_at_tick == 0
            || !valid_digest(&self.approval_digest)
            || self.approval_digest != self.digest()
        {
            return Err("automation_signal_approval_invalid");
        }
        if self.approver_id == self.owner_id {
            return Err("automation_signal_approval_self_approval");
        }
        if !self.approver_role.may_authorize_signals() {
            return Err("automation_signal_approver_unauthorized");
        }
        if self.expires_at_tick <= self.issued_tick {
            return Err("automation_signal_approval_window_invalid");
        }
        if !matches!(self.kind, AutomationSignalKind::Resume) {
            return Err("automation_signal_approval_kind_drift");
        }
        Ok(())
    }

    /// 按固定字段集合重算审批摘要；修改字段后必须调用。
    pub fn recompute_digest(&mut self) {
        self.approval_digest = self.digest();
    }

    /// 该审批在 `now_tick` 是否仍在有效窗口内；`now_tick` 由调用方提供且不被本函数认证。
    pub fn is_live_at(&self, now_tick: u64) -> bool {
        now_tick >= self.issued_tick && now_tick < self.expires_at_tick
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "approval_id": self.approval_id,
            "execution_id": self.execution_id,
            "owner_id": self.owner_id,
            "approver_id": self.approver_id,
            "approver_role": self.approver_role,
            "kind": self.kind,
            "definition_digest": self.definition_digest,
            "action_digest": self.action_digest,
            "path_scope_digest": self.path_scope_digest,
            "pause_signal_id": self.pause_signal_id,
            "authority_epoch": self.authority_epoch,
            "approval_epoch": self.approval_epoch,
            "issued_tick": self.issued_tick,
            "expires_at_tick": self.expires_at_tick,
        }))
    }
}

/// resume/consume 使用的 checkpoint 元数据。
///
/// checkpoint 是“暂停时把哪一次 run 记在哪里”的元数据声明：它绑定 execution、owner、definition、
/// action、path、authority epoch、checkpoint 序号与 source cursor。重启后恢复必须让请求与 checkpoint
/// 的这些身份字段逐项一致，才允许把 authority epoch 前推；本文件不读取真实 checkpoint 存储，
/// 因此通过校验不等于该 checkpoint 真的落盘或被当前进程认领。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCheckpoint {
    /// 必须精确匹配 `AUTOMATION_CHECKPOINT_SCHEMA`。
    pub schema: String,
    /// checkpoint 所属 execution；必须与请求一致。
    pub execution_id: String,
    /// checkpoint 所属 owner；必须与请求一致。
    pub owner_id: String,
    /// 生成 checkpoint 时的 definition 摘要；必须与请求一致。
    pub definition_digest: String,
    /// 生成 checkpoint 时的 action 摘要；必须与请求一致。
    pub action_digest: String,
    /// 生成 checkpoint 时的 path scope 摘要；必须与请求一致。
    pub path_scope_digest: String,
    /// 生成 checkpoint 时的 authority epoch；不得高于恢复请求的 epoch。
    pub authority_epoch: u64,
    /// checkpoint 序号；同一 execution 内不得回退。
    pub checkpoint_sequence: u64,
    /// checkpoint 对应的 source cursor；只检查非负，不推进或回退真实 cursor。
    pub source_cursor: u64,
    /// checkpoint 创建 tick；只要求非零。
    pub created_tick: u64,
    /// 对本 checkpoint 其他字段的 `sha256:` 摘要；不是签名。
    pub checkpoint_digest: String,
}

impl AutomationCheckpoint {
    /// 构造并校验一份 checkpoint 元数据；形状不合法时返回稳定错误码。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: impl Into<String>,
        owner_id: impl Into<String>,
        definition_digest: impl Into<String>,
        action_digest: impl Into<String>,
        path_scope_digest: impl Into<String>,
        authority_epoch: u64,
        checkpoint_sequence: u64,
        source_cursor: u64,
        created_tick: u64,
    ) -> Result<Self, &'static str> {
        let mut checkpoint = Self {
            schema: AUTOMATION_CHECKPOINT_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            owner_id: owner_id.into(),
            definition_digest: definition_digest.into(),
            action_digest: action_digest.into(),
            path_scope_digest: path_scope_digest.into(),
            authority_epoch,
            checkpoint_sequence,
            source_cursor,
            created_tick,
            checkpoint_digest: String::new(),
        };
        checkpoint.recompute_digest();
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    /// 校验 schema、身份/摘要形状、非零序号与自摘要；`source_cursor` 允许为 0。
    ///
    /// 通过时 `Ok(())` 只表示这条 checkpoint 元数据自洽：不查存储、不校验它与真实 run 的一致性，
    /// 也不创建或激活 checkpoint。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_CHECKPOINT_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_text(&self.owner_id)
            || !valid_digest(&self.definition_digest)
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.path_scope_digest)
            || self.authority_epoch == 0
            || self.checkpoint_sequence == 0
            || self.created_tick == 0
            || !valid_digest(&self.checkpoint_digest)
            || self.checkpoint_digest != self.digest()
        {
            return Err("automation_signal_checkpoint_invalid");
        }
        Ok(())
    }

    /// 按固定字段集合重算 checkpoint 摘要；修改字段后必须调用。
    pub fn recompute_digest(&mut self) {
        self.checkpoint_digest = self.digest();
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "owner_id": self.owner_id,
            "definition_digest": self.definition_digest,
            "action_digest": self.action_digest,
            "path_scope_digest": self.path_scope_digest,
            "authority_epoch": self.authority_epoch,
            "checkpoint_sequence": self.checkpoint_sequence,
            "source_cursor": self.source_cursor,
            "created_tick": self.created_tick,
        }))
    }
}

/// 计划函数判定的下一步：要提交（或已经存在）的事实，以及是否命中幂等重放。
///
/// `dispatch_permitted` 恒为 `false`：signal 契约只表达暂停/恢复/消费授权，永不派发 effect。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSignalPlan {
    /// 下一步应提交（或已经存在）的 signal 事实。
    pub fact: AutomationSignalFact,
    /// 是否命中同 signal_id 同内容的安全重放（重放时调用方不写新事实）。
    pub replayed: bool,
    /// 是否允许因此派发 effect；本契约永远为 false，真实执行只能走既有 ControlPlane/Broker 脊柱。
    pub dispatch_permitted: bool,
}

impl AutomationSignalPlan {
    /// 校验计划对象自身：事实自洽，且 `dispatch_permitted` 必须为 false。
    ///
    /// 任何声称允许派发的计划都返回 `automation_signal_dispatch_forbidden`，避免后续改动把
    /// signal 决策变成第二条执行入口。
    pub fn validate(&self) -> Result<(), &'static str> {
        self.fact.validate()?;
        if self.dispatch_permitted {
            return Err("automation_signal_dispatch_forbidden");
        }
        Ok(())
    }
}

/// CAS 提交结果：`replayed=true` 表示没有新建事实，`fact` 是当前权威副本。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSignalCommit {
    /// 提交后（或重放命中的）当前事实副本。
    pub fact: AutomationSignalFact,
    /// 是否命中同一内容并重放了既有事实。
    pub replayed: bool,
}

/// 只追加的 signal 账本：按 signal_id dedupe、按 execution 强制连续序号、单次消费审批。
///
/// 这是**进程内**账本：没有容量上限、没有持久化与跨进程围栏，因此记录一条事实不等于它已经 durable。
/// 它的用途是把“同一 signal 只能消费一次、审批只能支撑一次恢复、序号稳定、终态不可复活”变成
/// 可测试的单点规则。重启场景由调用方用 [`AutomationSignalLedger::record_recovered`] 重放已提交事实，
/// 但本函数不校验那些事实的真实来源。
#[derive(Debug, Default, Clone)]
pub struct AutomationSignalLedger {
    /// signal_id → 已记录事实。
    records: BTreeMap<String, AutomationSignalFact>,
    /// 追加顺序（signal_id），用于稳定、可重放的遍历。
    order: Vec<String>,
    /// 已消费过的审批摘要（恢复/消费事实引用过即算消费）。
    consumed_approvals: BTreeSet<String>,
}

impl AutomationSignalLedger {
    /// 新建空账本。
    pub fn new() -> Self {
        Self::default()
    }

    /// 按追加顺序遍历所有记录；顺序稳定，便于对账与夹具断言。
    pub fn facts(&self) -> impl Iterator<Item = &AutomationSignalFact> {
        self.order
            .iter()
            .filter_map(|signal_id| self.records.get(signal_id))
    }

    /// 按追加顺序遍历某个 execution 的记录。
    pub fn facts_for<'a>(
        &'a self,
        execution_id: &'a str,
    ) -> impl Iterator<Item = &'a AutomationSignalFact> + 'a {
        self.facts()
            .filter(move |fact| fact.execution_id == execution_id)
    }

    /// 取 signal_id 对应的已记录事实。
    pub fn fetch(&self, signal_id: &str) -> Option<&AutomationSignalFact> {
        self.records.get(signal_id)
    }

    /// 取某个 execution 当前序号最大的事实；没有记录时返回 `None`。
    pub fn latest(&self, execution_id: &str) -> Option<&AutomationSignalFact> {
        self.records
            .values()
            .filter(|fact| fact.execution_id == execution_id)
            .max_by_key(|fact| fact.signal_sequence)
    }

    /// 某个 execution 的下一个合法 signal 序号；第一个 signal 为 1。
    pub fn next_sequence(&self, execution_id: &str) -> u64 {
        self.latest(execution_id)
            .map_or(1, |fact| fact.signal_sequence + 1)
    }

    /// 审批摘要是否已被消费（恢复或消费事实引用过即算）。
    pub fn approval_consumed(&self, approval_digest: &str) -> bool {
        self.consumed_approvals.contains(approval_digest)
    }

    /// 记录数量。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 账本是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 重放一条已提交事实，用于重启后重新装填账本。
    ///
    /// 【拒绝面】形状不合法 → `automation_signal_fact_invalid`；同一 signal_id 不同摘要 →
    /// `automation_signal_recovery_conflict`；序号回退/跳号 →
    /// `automation_signal_sequence_regression` / `automation_signal_sequence_gap`；状态转移不在
    /// pause→resume→consume 白名单内 → `automation_signal_transition_refused`；审批摘要重复出现 →
    /// `automation_signal_approval_consumed`。
    /// 【输出】返回已记录（或同摘要重放命中）的事实副本。
    /// 【副作用】无 I/O、无日志、不创建 checkpoint、不派发 effect；本函数也不证明事实来源真实。
    pub fn record_recovered(
        &mut self,
        fact: AutomationSignalFact,
    ) -> Result<AutomationSignalFact, &'static str> {
        fact.validate()?;
        if let Some(existing) = self.records.get(&fact.signal_id) {
            if existing.signal_digest == fact.signal_digest {
                return Ok(existing.clone());
            }
            return Err("automation_signal_recovery_conflict");
        }
        let next = self.next_sequence(&fact.execution_id);
        if fact.signal_sequence < next {
            return Err("automation_signal_sequence_regression");
        }
        if fact.signal_sequence > next {
            return Err("automation_signal_sequence_gap");
        }
        let transition_refused = match self.latest(&fact.execution_id) {
            None => fact.state != AutomationSignalState::Paused,
            Some(previous) => !matches!(
                (previous.state, fact.state),
                (
                    AutomationSignalState::Paused,
                    AutomationSignalState::Resumed
                ) | (
                    AutomationSignalState::Resumed,
                    AutomationSignalState::Paused
                ) | (
                    AutomationSignalState::Resumed,
                    AutomationSignalState::Consumed
                )
            ),
        };
        if transition_refused {
            return Err("automation_signal_transition_refused");
        }
        if let Some(digest) = fact.approval_digest.as_deref() {
            if self.consumed_approvals.contains(digest) {
                return Err("automation_signal_approval_consumed");
            }
        }
        self.order.push(fact.signal_id.clone());
        self.records.insert(fact.signal_id.clone(), fact.clone());
        if let Some(digest) = fact.approval_digest.clone() {
            self.consumed_approvals.insert(digest);
        }
        Ok(fact)
    }

    /// 单次提交一个已通过 [`plan_automation_signal`] 的计划。
    ///
    /// 【拒绝面】请求/事实形状不合法、计划声称可以派发、signal_id 冲突、序号回退/跳号、
    /// 状态转移不在白名单、审批已被消费，都返回稳定错误码且不追加任何记录。
    /// 【幂等】同一 signal_id 同一事实摘要返回既有副本并标记 `replayed=true`，不新建记录。
    /// 【副作用】只修改本账本的内存结构；不写 EventLog、不派发 effect、不创建 checkpoint。
    pub fn apply(
        &mut self,
        request: &AutomationSignalRequest,
        plan: AutomationSignalPlan,
    ) -> Result<AutomationSignalCommit, &'static str> {
        request.validate()?;
        plan.validate()?;
        if plan.replayed {
            let existing = self
                .records
                .get(&plan.fact.signal_id)
                .ok_or("automation_signal_signal_conflict")?;
            if existing.signal_digest != plan.fact.signal_digest {
                return Err("automation_signal_signal_conflict");
            }
            return Ok(AutomationSignalCommit {
                fact: existing.clone(),
                replayed: true,
            });
        }
        if let Some(existing) = self.records.get(&request.signal_id) {
            if existing.signal_digest == plan.fact.signal_digest {
                return Ok(AutomationSignalCommit {
                    fact: existing.clone(),
                    replayed: true,
                });
            }
            return Err("automation_signal_signal_conflict");
        }
        if plan.fact.execution_id != request.execution_id
            || plan.fact.signal_sequence != request.signal_sequence
        {
            return Err("automation_signal_fact_drift");
        }
        let next = self.next_sequence(&request.execution_id);
        if request.signal_sequence < next {
            return Err("automation_signal_sequence_regression");
        }
        if request.signal_sequence > next {
            return Err("automation_signal_sequence_gap");
        }
        let transition_refused = match self.latest(&request.execution_id) {
            None => plan.fact.state != AutomationSignalState::Paused,
            Some(previous) => !matches!(
                (previous.state, plan.fact.state),
                (
                    AutomationSignalState::Paused,
                    AutomationSignalState::Resumed
                ) | (
                    AutomationSignalState::Resumed,
                    AutomationSignalState::Paused
                ) | (
                    AutomationSignalState::Resumed,
                    AutomationSignalState::Consumed
                )
            ),
        };
        if transition_refused {
            return Err("automation_signal_transition_refused");
        }
        if let Some(digest) = plan.fact.approval_digest.as_deref() {
            if self.consumed_approvals.contains(digest) {
                return Err("automation_signal_approval_consumed");
            }
        }
        self.order.push(plan.fact.signal_id.clone());
        self.records
            .insert(plan.fact.signal_id.clone(), plan.fact.clone());
        if let Some(digest) = plan.fact.approval_digest.clone() {
            self.consumed_approvals.insert(digest);
        }
        Ok(AutomationSignalCommit {
            fact: plan.fact,
            replayed: false,
        })
    }
}

/// 对外提供的领域校验函数；目前只转调 `AutomationSignalFact::validate()`。
///
/// 由 `kiana-domain` crate 导出，Core facade 也会转调它；当前生产代码搜索未发现实际使用者，
/// 可见使用集中在测试。错误原样返回稳定静态字符串，不产生任何执行或持久化副作用。
pub fn validate_automation_signal(fact: &AutomationSignalFact) -> Result<(), &'static str> {
    fact.validate()
}

/// 同 execution 内 signal 事实的状态转移白名单。
///
/// 同一状态返回 true（内容相同的重放由 ledger 的摘要比较判定）；`Consumed` 是终态，除了自身
/// 重放以外没有任何出边；`Resumed` 可以再次 `Paused`（恢复后再次暂停）或进入 `Consumed`；
/// `Paused` 只能被 `Resumed`。因此“已消费的 signal 复活”“未暂停直接恢复”在契约层被拒绝。
pub fn automation_signal_transition_allowed(
    from: AutomationSignalState,
    to: AutomationSignalState,
) -> bool {
    use AutomationSignalState::*;
    if from == to {
        return true;
    }
    matches!((from, to), (Paused, Resumed) | (Resumed, Paused | Consumed))
}

/// 计划一次 pause/resume/consume：按固定顺序拒绝，然后给出下一步事实。
///
/// 【核心流程（先拒绝）】
/// 1. 请求、审批、checkpoint 各自校验；请求自带 UI context 直接拒绝；
/// 2. `signal_id` 已记录：同内容返回幂等重放，不同内容返回 `automation_signal_signal_conflict`；
/// 3. 序号必须等于该 execution 的下一个连续序号（回退 → `..._sequence_regression`，
///    跳号 → `..._sequence_gap`）；
/// 4. 与当前事实逐项绑定：owner/definition/action/path 漂移分别拒绝；authority epoch 低于当前
///    事实 → `..._authority_epoch_stale`，高于当前事实只允许出现在携带匹配 checkpoint 的
///    `Resume`（重启恢复），其余 → `..._authority_epoch_ahead`；
/// 5. `Pause`：不得携带审批，必须携带 owner/definition/action/path/sequence 全部匹配的
///    checkpoint，且当前事实不得是 `Consumed` 或已 `Paused`；
/// 6. `Resume`：当前事实必须是 `Paused`，必须携带同 epoch、绑定同一 `pause_signal_id`、未过期、
///    未被消费、角色有权的审批，以及身份字段全部匹配的 checkpoint；
/// 7. `Consume`：当前事实必须是 `Resumed`，审批摘要必须等于恢复时使用的摘要，且不得携带新审批或
///    checkpoint；已消费的事实不能再次消费。
///
/// 【输出】`AutomationSignalPlan`：下一步事实 + 是否重放；`dispatch_permitted` 恒为 false，
/// 本函数永不派发 effect、不写账本、不创建 checkpoint。
/// 【失败】任一拒绝条件都返回稳定静态错误码，且不产生任何副作用。
pub fn plan_automation_signal(
    request: &AutomationSignalRequest,
    approval: Option<&AutomationSignalApproval>,
    checkpoint: Option<&AutomationCheckpoint>,
    ledger: &AutomationSignalLedger,
    now_tick: u64,
) -> Result<AutomationSignalPlan, &'static str> {
    request.validate()?;
    if let Some(approval) = approval {
        approval.validate()?;
    }
    if let Some(checkpoint) = checkpoint {
        checkpoint.validate()?;
    }
    if let Some(existing) = ledger.fetch(&request.signal_id) {
        if fact_answers_request(existing, request) {
            return Ok(AutomationSignalPlan {
                fact: existing.clone(),
                replayed: true,
                dispatch_permitted: false,
            });
        }
        return Err("automation_signal_signal_conflict");
    }
    let next = ledger.next_sequence(&request.execution_id);
    if request.signal_sequence < next {
        return Err("automation_signal_sequence_regression");
    }
    if request.signal_sequence > next {
        return Err("automation_signal_sequence_gap");
    }
    let previous = ledger.latest(&request.execution_id);
    if let Some(previous) = previous {
        if previous.owner_id != request.owner_id {
            return Err("automation_signal_owner_drift");
        }
        if previous.definition_digest != request.definition_digest {
            return Err("automation_signal_definition_digest_drift");
        }
        if previous.action_digest != request.action_digest {
            return Err("automation_signal_action_digest_drift");
        }
        if previous.path_scope_digest != request.path_scope_digest {
            return Err("automation_signal_path_scope_drift");
        }
        if request.authority_epoch < previous.authority_epoch {
            return Err("automation_signal_authority_epoch_stale");
        }
        if request.authority_epoch > previous.authority_epoch
            && !matches!(request.kind, AutomationSignalKind::Resume)
        {
            return Err("automation_signal_authority_epoch_ahead");
        }
    }
    let (state, approval_digest) = match request.kind {
        AutomationSignalKind::Pause => {
            if approval.is_some() {
                return Err("automation_signal_approval_unexpected");
            }
            if let Some(previous) = previous {
                if previous.state == AutomationSignalState::Consumed {
                    return Err("automation_signal_already_consumed");
                }
                if previous.state == AutomationSignalState::Paused {
                    return Err("automation_signal_state_conflict");
                }
            }
            let checkpoint = checkpoint.ok_or("automation_signal_checkpoint_required")?;
            bind_checkpoint(request, checkpoint)?;
            if let Some(previous) = previous {
                if checkpoint.checkpoint_sequence < previous.checkpoint_sequence {
                    return Err("automation_signal_checkpoint_sequence_regression");
                }
            }
            (AutomationSignalState::Paused, None)
        }
        AutomationSignalKind::Resume => {
            let previous = previous.ok_or("automation_signal_not_paused")?;
            if previous.state == AutomationSignalState::Consumed {
                return Err("automation_signal_already_consumed");
            }
            if previous.state != AutomationSignalState::Paused {
                return Err("automation_signal_not_paused");
            }
            let approval = approval.ok_or("automation_signal_approval_required")?;
            bind_approval(request, approval)?;
            if approval.pause_signal_id != previous.signal_id {
                return Err("automation_signal_approval_pause_drift");
            }
            if ledger.approval_consumed(&approval.approval_digest) {
                return Err("automation_signal_approval_consumed");
            }
            if approval.expires_at_tick < now_tick {
                return Err("automation_signal_approval_expired");
            }
            let checkpoint = checkpoint.ok_or("automation_signal_checkpoint_required")?;
            bind_checkpoint(request, checkpoint)?;
            if checkpoint.checkpoint_sequence < previous.checkpoint_sequence {
                return Err("automation_signal_checkpoint_sequence_regression");
            }
            (
                AutomationSignalState::Resumed,
                Some(approval.approval_digest.clone()),
            )
        }
        AutomationSignalKind::Consume => {
            let previous = previous.ok_or("automation_signal_not_resumed")?;
            if previous.state == AutomationSignalState::Consumed {
                return Err("automation_signal_already_consumed");
            }
            if previous.state != AutomationSignalState::Resumed {
                return Err("automation_signal_not_resumed");
            }
            if request.approval_digest != previous.approval_digest {
                return Err("automation_signal_approval_digest_drift");
            }
            if approval.is_some() {
                return Err("automation_signal_approval_unexpected");
            }
            if checkpoint.is_some() {
                return Err("automation_signal_checkpoint_unexpected");
            }
            (
                AutomationSignalState::Consumed,
                previous.approval_digest.clone(),
            )
        }
    };
    let mut fact = AutomationSignalFact {
        schema: AUTOMATION_SIGNAL_SCHEMA.to_owned(),
        execution_id: request.execution_id.clone(),
        signal_id: request.signal_id.clone(),
        signal_sequence: request.signal_sequence,
        owner_id: request.owner_id.clone(),
        definition_digest: request.definition_digest.clone(),
        action_digest: request.action_digest.clone(),
        path_scope_digest: request.path_scope_digest.clone(),
        authority_epoch: request.authority_epoch,
        checkpoint_sequence: request.checkpoint_sequence,
        state,
        approval_digest,
        consumed: matches!(state, AutomationSignalState::Consumed),
        signal_digest: String::new(),
    };
    fact.recompute_digest();
    fact.validate()?;
    Ok(AutomationSignalPlan {
        fact,
        replayed: false,
        dispatch_permitted: false,
    })
}

/// 已记录事实是否完整回答了这条请求（用于同 signal_id 的幂等重放判定）。
fn fact_answers_request(fact: &AutomationSignalFact, request: &AutomationSignalRequest) -> bool {
    let expected_state = match request.kind {
        AutomationSignalKind::Pause => AutomationSignalState::Paused,
        AutomationSignalKind::Resume => AutomationSignalState::Resumed,
        AutomationSignalKind::Consume => AutomationSignalState::Consumed,
    };
    fact.schema == AUTOMATION_SIGNAL_SCHEMA
        && fact.execution_id == request.execution_id
        && fact.signal_id == request.signal_id
        && fact.signal_sequence == request.signal_sequence
        && fact.owner_id == request.owner_id
        && fact.definition_digest == request.definition_digest
        && fact.action_digest == request.action_digest
        && fact.path_scope_digest == request.path_scope_digest
        && fact.authority_epoch == request.authority_epoch
        && fact.checkpoint_sequence == request.checkpoint_sequence
        && fact.approval_digest.as_deref() == request.approval_digest.as_deref()
        && fact.state == expected_state
}

/// 审批与恢复请求的逐项绑定；任一项不符都返回专门错误码，避免“用旧审批恢复新参数”。
fn bind_approval(
    request: &AutomationSignalRequest,
    approval: &AutomationSignalApproval,
) -> Result<(), &'static str> {
    if !matches!(approval.kind, AutomationSignalKind::Resume) {
        return Err("automation_signal_approval_kind_drift");
    }
    if approval.execution_id != request.execution_id {
        return Err("automation_signal_approval_binding_drift");
    }
    if approval.owner_id != request.owner_id {
        return Err("automation_signal_approval_owner_drift");
    }
    if approval.definition_digest != request.definition_digest {
        return Err("automation_signal_approval_definition_drift");
    }
    if approval.action_digest != request.action_digest {
        return Err("automation_signal_approval_action_drift");
    }
    if approval.path_scope_digest != request.path_scope_digest {
        return Err("automation_signal_approval_path_drift");
    }
    if approval.authority_epoch != request.authority_epoch {
        return Err("automation_signal_approval_authority_epoch_stale");
    }
    if request.approval_digest.as_deref() != Some(approval.approval_digest.as_str()) {
        return Err("automation_signal_approval_digest_drift");
    }
    Ok(())
}

/// checkpoint 与请求的逐项绑定；epoch 只允许请求不落后于 checkpoint，身份字段必须完全一致。
fn bind_checkpoint(
    request: &AutomationSignalRequest,
    checkpoint: &AutomationCheckpoint,
) -> Result<(), &'static str> {
    if checkpoint.execution_id != request.execution_id {
        return Err("automation_signal_checkpoint_binding_drift");
    }
    if checkpoint.owner_id != request.owner_id {
        return Err("automation_signal_checkpoint_owner_drift");
    }
    if checkpoint.definition_digest != request.definition_digest {
        return Err("automation_signal_checkpoint_definition_digest_drift");
    }
    if checkpoint.action_digest != request.action_digest {
        return Err("automation_signal_checkpoint_action_digest_drift");
    }
    if checkpoint.path_scope_digest != request.path_scope_digest {
        return Err("automation_signal_checkpoint_path_scope_drift");
    }
    if checkpoint.checkpoint_sequence != request.checkpoint_sequence {
        return Err("automation_signal_checkpoint_sequence_drift");
    }
    if checkpoint.authority_epoch > request.authority_epoch {
        return Err("automation_signal_checkpoint_authority_epoch_ahead");
    }
    Ok(())
}

/// 检查 ID 类字段是否为非空、无空白、无 NUL/换行且不超过 512 UTF-8 字节的文本。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

/// 检查摘要是否为 `sha256:` 前缀加 64 个大小写均可的十六进制字符；不验证摘要来源。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
