//! AUT-14 效果预留事实、执行绑定、permit、attempt、版本围栏和进程内幂等账本。
//!
//! # 系统位置与数据流
//!
//! ```text
//! Core / 调用方登记已知 execution 的 AutomationExecutionBinding
//!     -> 构造 AutomationEffectReservation（绑定 action/capability/budget/path/approval）
//!     -> AutomationReservationLedger::reserve()
//!     -> 检查 authority/config/policy 快照、已知 execution、scope 摘要、幂等键
//!     -> issue_permit() 为该 reservation 签发一次性 permit
//!     -> begin_attempt() 消耗 permit 并建立恰好一个 attempt
//!     -> fetch_*() 供崩溃后安全重取已提交事实（不重新执行）
//! ```
//!
//! 本文件属于底层领域契约，定义一组可序列化事实：某个执行准备进行某个动作（reservation）、
//! 该动作在一份固定 scope 下的权威快照（execution binding）、服务端签发的短时凭证（permit）
//! 与唯一一次有效尝试（attempt）。ledger 仍只是当前进程内的 `BTreeMap`，没有容量、过期清理、
//! 持久化或跨进程协调，因此它不会自己产生 durable 语义；`Reserved`/`Issued`/`Started` 都不是
//! 外部动作已执行或已成功的证据。当前未找到生产代码调用该账本，也没有通往 Broker、EventLog、
//! worker 或外部副作用的调用链。
//!
//! 摘要是无密钥的 SHA-256 内容指纹，作用是发现载荷不一致，不是签名、身份认证或授权证明。账本的
//! authority/config/policy 值由构造者提供并在每次 reserve 时与 fact 对照；这里不主动读取权威状态。
//! execution binding 由调用方登记，登记本身不代表该 execution 真实存在或曾获授权。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// AUT-14 预留事实使用的固定序列化 schema 名称。
pub const AUTOMATION_RESERVATION_SCHEMA: &str = "kiana.automation-effect-reservation.v1";
/// execution 权威 scope 快照使用的固定序列化 schema 名称。
pub const AUTOMATION_EXECUTION_BINDING_SCHEMA: &str = "kiana.automation-execution-binding.v1";
/// 服务端 permit 使用的固定序列化 schema 名称。
pub const AUTOMATION_EFFECT_PERMIT_SCHEMA: &str = "kiana.automation-effect-permit.v1";
/// 唯一有效 attempt 使用的固定序列化 schema 名称。
pub const AUTOMATION_EFFECT_ATTEMPT_SCHEMA: &str = "kiana.automation-effect-attempt.v1";

/// 预留事实记录的声明状态。
///
/// 当前 ledger 只会在新建记录时把状态规范成 `Reserved`；本文件没有提交、消耗、未知结果恢复等
/// 状态转移方法。枚举中的其他值可由反序列化输入提供，但不能据此推断对应生命周期已实现。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationReservationState {
    /// 资源/动作意图已登记在当前 ledger；不代表外部效果已执行。
    Reserved,
    /// 载荷可表达已提交状态，但当前 ledger 没有将记录转移到此状态的 API。
    Committed,
    /// 载荷可表达已消耗状态，但当前 ledger 没有 permit 消耗实现。
    Consumed,
    /// 结果未知的载荷状态；当前没有在此模块中进行 reconcile 的方法。
    Unknown,
}

/// 将某次动作意图绑定到执行身份和授权/资源快照上的可序列化事实。
///
/// 字段由上游构造并随事实一起计算 digest。validator 只校验必需文本、摘要格式及自身摘要一致性，不
/// 解析真实执行、动作、capability、budget、路径、approval 或 policy 内容；各 digest 是调用方声明的
/// 引用。`deny_unknown_fields` 使 Serde 反序列化时拒绝 schema 外的额外字段，避免静默忽略版本差异。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationEffectReservation {
    /// 必须与本模块固定 schema 常量完全相等。
    pub schema: String,
    /// 预留记录 ID；此模块只验证格式，不要求它全局唯一。
    pub reservation_id: String,
    /// 用于将重复 reserve 请求映射到同一记录的幂等键；ledger 以它建立幂等索引。
    pub idempotency_key: String,
    /// 拥有该动作意图的执行 ID；ledger 要求它已登记 execution binding。
    pub execution_id: String,
    /// 待绑定动作内容的 `sha256:` 摘要；本模块不重建或执行动作。
    pub action_digest: String,
    /// 动作申请的 capability 名称；只是被绑定的声明，不会在这里授权。
    pub capability: String,
    /// 动作所依据的授权语义 epoch；必须非零，并且 reserve 时与 ledger 和 binding 快照一致。
    pub authority_epoch: u64,
    /// 配置版本标识；reserve 时与 ledger 记录的配置快照逐字比较。
    pub config_revision: String,
    /// 策略版本标识；reserve 时与 ledger 记录的策略快照逐字比较。
    pub policy_revision: String,
    /// budget 事实摘要；ledger 要求它与 execution binding 中的 budget 摘要一致，但不读取额度。
    pub budget_digest: String,
    /// 可写路径/scope 声明摘要；ledger 要求它与 binding 一致，但不在本模块打开路径或检验 sandbox。
    pub path_scope_digest: String,
    /// 可选审批摘要；None 表示此 fact 未绑定审批摘要，方法与 binding 的该字段逐位比较。
    pub approval_digest: Option<String>,
    /// 事实当前声明的生命周期状态；ledger 对新 key 会覆盖为 Reserved。
    pub state: AutomationReservationState,
    /// 除本字段外所有事实字段的内容摘要；validate 会重算并比较。
    pub reservation_digest: String,
}

impl AutomationEffectReservation {
    /// 检查 fact 的 schema、字段形状及自带摘要；不查询 authority，也不 reserve 资源。
    ///
    /// 【调用者/输入】通常由 `AutomationReservationLedger::reserve()` 在任何 map 查找前调用；单独使用时，
    /// 调用方传入一份已经组装好的 reservation。
    ///
    /// 【检查过程】依次要求固定 schema；ID/capability/revision 文本非空、不含任何 Unicode 空白且不超过 512
    /// UTF-8 字节；action/budget/path/可选 approval/reservation digest 都符合 `sha256:` 加 64 位十六进制
    /// 文本；authority_epoch 非零；最后将除 reservation_digest 外的字段重新序列化并比较摘要。
    ///
    /// 【输出/失败】有效返回 `Ok(())`，任一条件不满足返回 `automation_effect_reservation_invalid`。
    /// 这是静态输入错误，validator 自身不记录日志、不重试、不禁用主体、不向用户直接显示消息；上层决定
    /// 如何映射这个领域错误码。
    ///
    /// 【重要边界】digest 无密钥且其余 digest 只检查格式，所以验证通过仅证明“载荷与自带指纹一致”，
    /// 不证明 fact 来自可信调用者、所引用内容存在或动作已获准。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_RESERVATION_SCHEMA
            || !valid_text(&self.reservation_id)
            || !valid_text(&self.idempotency_key)
            || !valid_text(&self.execution_id)
            || !valid_digest(&self.action_digest)
            || !valid_text(&self.capability)
            || self.authority_epoch == 0
            || !valid_text(&self.config_revision)
            || !valid_text(&self.policy_revision)
            || !valid_digest(&self.budget_digest)
            || !valid_digest(&self.path_scope_digest)
            || self
                .approval_digest
                .as_deref()
                .is_some_and(|v| !valid_digest(v))
            || !valid_digest(&self.reservation_digest)
            || self.reservation_digest != self.digest()
        {
            return Err("automation_effect_reservation_invalid");
        }
        Ok(())
    }

    /// 以稳定 JSON 对象覆盖除 reservation_digest 外的字段，生成内容指纹。
    ///
    /// state 也包含在摘要内，因此状态改变必须重新计算摘要。对象键在这里显式列出，避免遗漏身份、版本
    /// 围栏或 budget/scope 声明后，仍把旧摘要误当作同一份完整事实。摘要不证明数据来源可信。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "reservation_id": self.reservation_id,
            "idempotency_key": self.idempotency_key,
            "execution_id": self.execution_id,
            "action_digest": self.action_digest,
            "capability": self.capability,
            "authority_epoch": self.authority_epoch,
            "config_revision": self.config_revision,
            "policy_revision": self.policy_revision,
            "budget_digest": self.budget_digest,
            "path_scope_digest": self.path_scope_digest,
            "approval_digest": self.approval_digest,
            "state": self.state,
        }))
    }
}

/// 单个 execution 的权威 scope 快照：action、capability、budget、path 和 approval 摘要。
///
/// 登记这份快照的调用方声明“这个 execution 被允许执行的动作形状就是这些摘要”。`reserve()` 只做
/// 逐字摘要比较，因此 binding 变化会立即让携带旧摘要的 reservation 以
/// `automation_effect_reservation_scope_drift` 失败，而不是静默复用旧意图。摘要无密钥，
/// 登记与比较都不构成真实性证明：真正的授权、审批和预算判定仍由 ControlPlane 与 Broker 负责。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationExecutionBinding {
    /// 必须与本模块固定 schema 常量完全相等。
    pub schema: String,
    /// 该快照对应的 execution 身份；ledger 以它作为唯一索引键。
    pub execution_id: String,
    /// 允许的动作内容摘要；与 reservation 的 action_digest 逐字比较。
    pub action_digest: String,
    /// 允许的 capability 名称；与 reservation 的 capability 逐字比较。
    pub capability: String,
    /// 绑定的 budget 事实摘要；与 reservation 的 budget_digest 逐字比较。
    pub budget_digest: String,
    /// 绑定的路径/scope 摘要；与 reservation 的 path_scope_digest 逐字比较。
    pub path_scope_digest: String,
    /// 绑定的审批摘要；None 表示该 execution 明确没有绑定审批摘要。
    pub approval_digest: Option<String>,
    /// 除本字段外所有 binding 字段的内容摘要；validate 会重算并比较。
    pub binding_digest: String,
}

impl AutomationExecutionBinding {
    /// 构造并立即校验一份 execution binding；digest 由构造器写入。
    ///
    /// 【调用者/输入】Core/调用方在登记 execution 时构造；参数都是调用方声明的引用，本函数不查询
    /// 真实执行、审批或预算记录。`approval_digest` 为 None 表示“明确没有绑定审批摘要”，不是
    /// “判定无需审批”。
    ///
    /// 【输出/失败】字段形状或摘要格式不满足时返回 `automation_effect_execution_binding_invalid`，
    /// 不会返回部分构造的对象。
    pub fn new(
        execution_id: impl Into<String>,
        action_digest: impl Into<String>,
        capability: impl Into<String>,
        budget_digest: impl Into<String>,
        path_scope_digest: impl Into<String>,
        approval_digest: Option<String>,
    ) -> Result<Self, &'static str> {
        let mut binding = Self {
            schema: AUTOMATION_EXECUTION_BINDING_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            action_digest: action_digest.into(),
            capability: capability.into(),
            budget_digest: budget_digest.into(),
            path_scope_digest: path_scope_digest.into(),
            approval_digest,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    /// 检查 binding 的 schema、文本边界、摘要格式与自身摘要一致性。
    ///
    /// 不查询 execution 是否存在、审批是否有效或预算是否充足；所有这些判断都在更上层的授权
    /// 边界完成。通过校验只说明这份快照内部自洽。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_EXECUTION_BINDING_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_digest(&self.action_digest)
            || !valid_text(&self.capability)
            || !valid_digest(&self.budget_digest)
            || !valid_digest(&self.path_scope_digest)
            || self
                .approval_digest
                .as_deref()
                .is_some_and(|v| !valid_digest(v))
            || !valid_digest(&self.binding_digest)
            || self.binding_digest != self.digest()
        {
            return Err("automation_effect_execution_binding_invalid");
        }
        Ok(())
    }

    /// 以稳定 JSON 对象覆盖除 binding_digest 外的字段，生成内容指纹。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "action_digest": self.action_digest,
            "capability": self.capability,
            "budget_digest": self.budget_digest,
            "path_scope_digest": self.path_scope_digest,
            "approval_digest": self.approval_digest,
        }))
    }
}

/// permit 的生命周期：签发后可被恰好一个 attempt 消耗。
///
/// 状态转换只能通过 `AutomationReservationLedger::begin_attempt()` 发生：`Issued` 在成功建立
/// attempt 时被改成 `Consumed`。本模块没有“重新签发”“撤销后复用”或“过期后恢复”的 API；重复
/// 消耗、换 attempt 或换 permit 的重试都以稳定错误码拒绝。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPermitState {
    /// 已签发、尚未消耗；此时还没有任何 effect 被执行。
    Issued,
    /// 已被唯一一次 attempt 消耗；同 permit 不能再次建立 attempt。
    Consumed,
}

/// 服务端为一份已登记 reservation 签发的短时凭证，绑定 action、execution 和全部版本快照。
///
/// permit 由 ledger 的 `issue_permit()` 构造，调用方不能自行拼装后当作权威：ledger 查找 permit 时
/// 只认自己保存的副本。permit 的 digest 覆盖 state，因此消耗状态变化会改变摘要；新摘要仍是内容
/// 指纹，不是签名。持有 permit 不代表外部动作已获准或已发生，它只说明该 reservation 在本地账本中
/// 已走到可建立 attempt 的阶段。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationEffectPermit {
    /// 必须与本模块固定 schema 常量完全相等。
    pub schema: String,
    /// permit 记录 ID；ledger 以“一 reservation 至多一份 permit”为约束，不要求全局唯一。
    pub permit_id: String,
    /// 签发此 permit 的 reservation ID。
    pub reservation_id: String,
    /// 签发时 reservation 的完整摘要；reservation 内容变化会与它不一致。
    pub reservation_digest: String,
    /// 绑定的 execution 身份。
    pub execution_id: String,
    /// 绑定的动作摘要。
    pub action_digest: String,
    /// 绑定的 capability 名称。
    pub capability: String,
    /// 签发时的授权 epoch；ledger 在建立 attempt 时要求调用方重报同一值。
    pub authority_epoch: u64,
    /// 签发时的配置 revision。
    pub config_revision: String,
    /// 签发时的策略 revision。
    pub policy_revision: String,
    /// 该 permit 一次性消耗的围栏令牌；0 没有意义。
    pub fence_token: u64,
    /// permit 当前状态；只有 ledger 能通过建立 attempt 将其推进到 Consumed。
    pub state: AutomationPermitState,
    /// 除本字段外所有 permit 字段的内容摘要；validate 会重算并比较。
    pub permit_digest: String,
}

impl AutomationEffectPermit {
    /// 由 ledger 内部根据 reservation 构造一份 `Issued` permit。
    ///
    /// 私有构造函数防止模块外伪造“已签发”副本；读取方持有的 permit 只有在 ledger 中存在同 permit_id
    /// 的登记时才可能继续推进。函数不读取 authority、budget 或审批状态。
    fn issue(
        reservation: &AutomationEffectReservation,
        permit_id: String,
        fence_token: u64,
    ) -> Result<Self, &'static str> {
        let mut permit = Self {
            schema: AUTOMATION_EFFECT_PERMIT_SCHEMA.to_owned(),
            permit_id,
            reservation_id: reservation.reservation_id.clone(),
            reservation_digest: reservation.reservation_digest.clone(),
            execution_id: reservation.execution_id.clone(),
            action_digest: reservation.action_digest.clone(),
            capability: reservation.capability.clone(),
            authority_epoch: reservation.authority_epoch,
            config_revision: reservation.config_revision.clone(),
            policy_revision: reservation.policy_revision.clone(),
            fence_token,
            state: AutomationPermitState::Issued,
            permit_digest: String::new(),
        };
        permit.permit_digest = permit.digest();
        permit.validate()?;
        Ok(permit)
    }

    /// 检查 permit 的 schema、文本边界、epoch/fence 非零、摘要格式与自身摘要一致性。
    ///
    /// 不查询该 permit 是否已被消耗、所引用 reservation 是否仍存在，也不判断外部动作是否允许执行；
    /// 这些检查由 ledger 的状态转换方法完成。通过校验只说明 permit 载荷内部自洽。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_EFFECT_PERMIT_SCHEMA
            || !valid_text(&self.permit_id)
            || !valid_text(&self.reservation_id)
            || !valid_digest(&self.reservation_digest)
            || !valid_text(&self.execution_id)
            || !valid_digest(&self.action_digest)
            || !valid_text(&self.capability)
            || self.authority_epoch == 0
            || !valid_text(&self.config_revision)
            || !valid_text(&self.policy_revision)
            || self.fence_token == 0
            || !valid_digest(&self.permit_digest)
            || self.permit_digest != self.digest()
        {
            return Err("automation_effect_permit_invalid");
        }
        Ok(())
    }

    /// 以稳定 JSON 对象覆盖除 permit_digest 外的字段，生成内容指纹。state 变化必须重算摘要。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "permit_id": self.permit_id,
            "reservation_id": self.reservation_id,
            "reservation_digest": self.reservation_digest,
            "execution_id": self.execution_id,
            "action_digest": self.action_digest,
            "capability": self.capability,
            "authority_epoch": self.authority_epoch,
            "config_revision": self.config_revision,
            "policy_revision": self.policy_revision,
            "fence_token": self.fence_token,
            "state": self.state,
        }))
    }
}

/// 一份 reservation 的唯一有效尝试记录，绑定 permit 与全部执行身份摘要。
///
/// ledger 对“一 reservation 至多一个 attempt”取交集式约束：第一个 `begin_attempt()` 成功后，
/// 换 attempt_id、换 permit 或二次消耗都返回稳定错误；相同 attempt_id + permit_id 的重放只返回
/// `AlreadyStarted` 的既有事实，供调用方安全重取而不是重新执行。`attempt` 在本切片固定为 1；
/// 失败后的新 attempt 属于 AUT-16 的 retry 切片，不能在这里用同一个 reservation 改写。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationEffectAttempt {
    /// 必须与本模块固定 schema 常量完全相等。
    pub schema: String,
    /// 本次尝试的稳定 ID；同一 reservation 只登记一个。
    pub attempt_id: String,
    /// 所属 reservation ID。
    pub reservation_id: String,
    /// 建立 attempt 时 reservation 的完整摘要。
    pub reservation_digest: String,
    /// 绑定的 execution 身份。
    pub execution_id: String,
    /// 绑定的动作摘要。
    pub action_digest: String,
    /// 绑定的 capability 名称。
    pub capability: String,
    /// 消耗掉的 permit ID。
    pub permit_id: String,
    /// 消耗前（Issued 状态）permit 的内容摘要。
    pub permit_digest: String,
    /// 建立 attempt 时的授权 epoch。
    pub authority_epoch: u64,
    /// 尝试序号；本切片由 ledger 固定为 1，validate 只要求非零。
    pub attempt: u32,
    /// 除本字段外所有 attempt 字段的内容摘要；validate 会重算并比较。
    pub attempt_digest: String,
}

impl AutomationEffectAttempt {
    /// 由 ledger 内部根据 reservation 与 permit 构造唯一 attempt。
    ///
    /// 私有构造函数防止模块外伪造“已开始”记录；同时 fail-closed 地核对 permit 与 reservation
    /// 的 reservation_id/digest/epoch 绑定，任何漂移都返回 `automation_effect_attempt_permit_mismatch`。
    fn issue(
        reservation: &AutomationEffectReservation,
        permit: &AutomationEffectPermit,
        attempt_id: String,
    ) -> Result<Self, &'static str> {
        if permit.reservation_id != reservation.reservation_id
            || permit.reservation_digest != reservation.reservation_digest
            || permit.authority_epoch != reservation.authority_epoch
        {
            return Err("automation_effect_attempt_permit_mismatch");
        }
        let mut attempt = Self {
            schema: AUTOMATION_EFFECT_ATTEMPT_SCHEMA.to_owned(),
            attempt_id,
            reservation_id: reservation.reservation_id.clone(),
            reservation_digest: reservation.reservation_digest.clone(),
            execution_id: reservation.execution_id.clone(),
            action_digest: reservation.action_digest.clone(),
            capability: reservation.capability.clone(),
            permit_id: permit.permit_id.clone(),
            permit_digest: permit.permit_digest.clone(),
            authority_epoch: reservation.authority_epoch,
            attempt: 1,
            attempt_digest: String::new(),
        };
        attempt.attempt_digest = attempt.digest();
        attempt.validate()?;
        Ok(attempt)
    }

    /// 检查 attempt 的 schema、文本边界、摘要格式、序号与自身摘要一致性。
    ///
    /// 不判断外部动作是否进行、permit 是否仍然有效或结果是否已知；这些属于后续 observation/
    /// reconcile 边界。通过校验只说明这份尝试记录内部自洽。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_EFFECT_ATTEMPT_SCHEMA
            || !valid_text(&self.attempt_id)
            || !valid_text(&self.reservation_id)
            || !valid_digest(&self.reservation_digest)
            || !valid_text(&self.execution_id)
            || !valid_digest(&self.action_digest)
            || !valid_text(&self.capability)
            || !valid_text(&self.permit_id)
            || !valid_digest(&self.permit_digest)
            || self.authority_epoch == 0
            || self.attempt == 0
            || !valid_digest(&self.attempt_digest)
            || self.attempt_digest != self.digest()
        {
            return Err("automation_effect_attempt_invalid");
        }
        Ok(())
    }

    /// 以稳定 JSON 对象覆盖除 attempt_digest 外的字段，生成内容指纹。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "attempt_id": self.attempt_id,
            "reservation_id": self.reservation_id,
            "reservation_digest": self.reservation_digest,
            "execution_id": self.execution_id,
            "action_digest": self.action_digest,
            "capability": self.capability,
            "permit_id": self.permit_id,
            "permit_digest": self.permit_digest,
            "authority_epoch": self.authority_epoch,
            "attempt": self.attempt,
        }))
    }
}

/// `begin_attempt()` 的结果：区分“本次新建”与“此前已开始的重放”。
///
/// 调用方必须只对 `Started` 执行 effect；`AlreadyStarted` 表示同一 permit/attempt 早已登记，
/// 应改为读取既有事实并对账，而不是再次调用 Broker 或 handler。这个区分是本模块防止重复副作用的
/// 最小机制。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationEffectAttemptBegin {
    /// 本次调用新建了唯一 attempt；调用方可在授权边界内执行一次 effect。
    Started(AutomationEffectAttempt),
    /// 相同 permit_id + attempt_id 的重放；返回既有事实，不新建 attempt，也不消耗 permit。
    AlreadyStarted(AutomationEffectAttempt),
}

impl AutomationEffectAttemptBegin {
    /// 读取结果中的 attempt 事实，供调用方记录或对账。
    pub fn attempt(&self) -> &AutomationEffectAttempt {
        match self {
            Self::Started(attempt) | Self::AlreadyStarted(attempt) => attempt,
        }
    }

    /// 本次调用是否新建了 attempt；重放返回 false。
    pub fn started(&self) -> bool {
        matches!(self, Self::Started(_))
    }
}

/// 针对一份固定 authority/config/policy 快照的进程内 reservation/permit/attempt 账本。
///
/// `reservations`/`permits`/`attempts` 都按稳定键排序保存，以便给定键获得确定查找行为。结构体没有
/// 锁、持久化端口、容量参数或过期清理；`&mut self` 只让单次 Rust 调用通过独占借用顺序更新这个
/// 对象，并不能协调不同 ledger 副本/进程。`kiana-eventlog` 的 CAS 语义存储适配器是这一状态在
/// 事件存储边界上的对应物，但它同样是 CI-语义实现，不构成 durable 证明。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationReservationLedger {
    /// 此 ledger 接受的授权 epoch；调用者负责保证它是当前可信快照。
    pub authority_epoch: u64,
    /// 此 ledger 接受的配置 revision。
    pub config_revision: String,
    /// 此 ledger 接受的策略 revision。
    pub policy_revision: String,
    /// 已登记 execution 的权威 scope 快照，key = execution_id。
    executions: BTreeMap<String, AutomationExecutionBinding>,
    /// 已登记 reservation，key = reservation_id；私有字段阻止模块外绕过方法写入。
    reservations: BTreeMap<String, AutomationEffectReservation>,
    /// 幂等索引：idempotency_key -> reservation_id，用于重放同一请求。
    idempotency: BTreeMap<String, String>,
    /// 每份 reservation 至多一份 permit，key = reservation_id。
    permits: BTreeMap<String, AutomationEffectPermit>,
    /// 每份 reservation 至多一个 attempt，key = reservation_id。
    attempts: BTreeMap<String, AutomationEffectAttempt>,
}

impl AutomationReservationLedger {
    /// 创建一张空的进程内账本，并绑定调用者提供的三项快照版本。
    ///
    /// 构造时没有加载已有记录、检查版本是否当前或校验 revision 字符串；因此新建 ledger 的正确性和
    /// 生命周期由调用方负责。ledger 丢弃或进程退出后，记录没有本模块提供的恢复来源。
    pub fn new(authority_epoch: u64, config_revision: String, policy_revision: String) -> Self {
        Self {
            authority_epoch,
            config_revision,
            policy_revision,
            executions: BTreeMap::new(),
            reservations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            permits: BTreeMap::new(),
            attempts: BTreeMap::new(),
        }
    }

    /// 登记（或确认）一个 execution 的权威 scope 快照。
    ///
    /// 【调用者】Core/调用方在 reserve 之前登记；同一 execution_id 重复登记相同摘要幂等返回 Ok，
    /// 不同摘要返回 `automation_effect_execution_binding_conflict`，避免同一个 execution 被两个
    /// 互相矛盾的 scope 声明同时使用。
    /// 【副作用】仅写入当前 ledger；不查询真实执行、审批或预算记录，也不产生任何事件。
    pub fn register_execution(
        &mut self,
        binding: AutomationExecutionBinding,
    ) -> Result<(), &'static str> {
        binding.validate()?;
        if let Some(existing) = self.executions.get(&binding.execution_id) {
            if existing.binding_digest == binding.binding_digest {
                return Ok(());
            }
            return Err("automation_effect_execution_binding_conflict");
        }
        self.executions
            .insert(binding.execution_id.clone(), binding);
        Ok(())
    }

    /// 读取已登记的 execution binding；未登记返回 `automation_effect_reservation_execution_unknown`。
    pub fn fetch_execution(
        &self,
        execution_id: &str,
    ) -> Result<AutomationExecutionBinding, &'static str> {
        self.executions
            .get(execution_id)
            .cloned()
            .ok_or("automation_effect_reservation_execution_unknown")
    }

    /// 校验并登记一个 reservation；相同幂等键的重放只有摘要相同才返回原记录。
    ///
    /// 【调用者/输入】Core 的 `reserve_automation_effect()` facade 只转调此方法；fact 的版本和摘要由
    /// 调用者构造，ledger 自身的版本在 `new()` 时固定。
    ///
    /// 【核心流程】
    /// 1. 先运行 fact 的形状及摘要校验，非法输入立即返回 `automation_effect_reservation_invalid`；
    /// 2. 对比 authority epoch、config revision、policy revision，任何漂移都返回 fence drift；
    /// 3. 按幂等键查索引。已有记录的 reservation_digest 相同则返回已保存副本；同 key 不同 digest 则
    ///    返回 idempotency conflict；
    /// 4. 新 key 要求 execution 已登记，否则返回 `automation_effect_reservation_execution_unknown`；
    /// 5. 将 action/capability/budget/path/approval 摘要与 execution binding 逐字比较，任何一项不同
    ///    返回 `automation_effect_reservation_scope_drift`；
    /// 6. 才把输入 state 规范成 `Reserved`，重算摘要、再次验证并插入账本。
    ///
    /// 【副作用/失败】仅在当前对象中插入一条记录。校验、版本漂移、未知 execution、scope drift 或
    /// 幂等冲突均不会更改账本，也不会触发自动 retry、日志、EventLog、Broker 或外部动作。
    ///
    /// ⚠ 账本条目没有容量上限或过期清理，也没有持久化；`&mut self` 只提供进程内串行修改。
    pub fn reserve(
        &mut self,
        mut reservation: AutomationEffectReservation,
    ) -> Result<AutomationEffectReservation, &'static str> {
        reservation.validate()?;
        if reservation.authority_epoch != self.authority_epoch
            || reservation.config_revision != self.config_revision
            || reservation.policy_revision != self.policy_revision
        {
            return Err("automation_effect_reservation_fence_drift");
        }
        if let Some(reservation_id) = self.idempotency.get(&reservation.idempotency_key) {
            let existing = self
                .reservations
                .get(reservation_id)
                .ok_or("automation_effect_reservation_store_corrupt")?;
            if existing.reservation_digest == reservation.reservation_digest {
                return Ok(existing.clone());
            }
            return Err("automation_effect_reservation_idempotency_conflict");
        }
        if self.reservations.contains_key(&reservation.reservation_id) {
            return Err("automation_effect_reservation_conflict");
        }
        let binding = self
            .executions
            .get(&reservation.execution_id)
            .ok_or("automation_effect_reservation_execution_unknown")?;
        if binding.action_digest != reservation.action_digest
            || binding.capability != reservation.capability
            || binding.budget_digest != reservation.budget_digest
            || binding.path_scope_digest != reservation.path_scope_digest
            || binding.approval_digest != reservation.approval_digest
        {
            return Err("automation_effect_reservation_scope_drift");
        }
        reservation.state = AutomationReservationState::Reserved;
        reservation.reservation_digest = reservation.digest();
        reservation.validate()?;
        self.idempotency.insert(
            reservation.idempotency_key.clone(),
            reservation.reservation_id.clone(),
        );
        self.reservations
            .insert(reservation.reservation_id.clone(), reservation.clone());
        Ok(reservation)
    }

    /// 按 reservation_id 重取已登记事实；未登记返回 `automation_effect_reservation_unknown`。
    ///
    /// 这是崩溃/重试后的安全读取路径：只克隆既有事实，不重新 reserve、不签发 permit、不建立 attempt。
    pub fn fetch_reservation(
        &self,
        reservation_id: &str,
    ) -> Result<AutomationEffectReservation, &'static str> {
        self.reservations
            .get(reservation_id)
            .cloned()
            .ok_or("automation_effect_reservation_unknown")
    }

    /// 按幂等键重取已登记 reservation；未登记返回 `automation_effect_reservation_unknown`。
    ///
    /// 调用方在提交结果不确定时可以安全重取，而不需要再次执行 reserve 的副作用面。
    pub fn fetch_reservation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<AutomationEffectReservation, &'static str> {
        let reservation_id = self
            .idempotency
            .get(idempotency_key)
            .ok_or("automation_effect_reservation_unknown")?;
        self.fetch_reservation(reservation_id)
    }

    /// 为一份已登记 reservation 签发至多一份 permit。
    ///
    /// 【调用者】Core 的 `issue_automation_effect_permit()` facade 转调；调用方提供 permit_id 和
    /// 非零 fence_token，本方法不查询 authority、budget 或审批状态。
    ///
    /// 【核心流程】
    /// 1. 按 reservation_id 查 reservation，缺失返回 `automation_effect_permit_reservation_unknown`；
    /// 2. permit_id 非法或 fence_token == 0 返回 `automation_effect_permit_invalid`；
    /// 3. 已有 permit 且 permit_id/fence_token 相同：`Issued` 幂等返回原副本，`Consumed` 返回
    ///    `automation_effect_permit_already_consumed`；
    /// 4. 已有 permit 但标识不同返回 `automation_effect_permit_conflict`（一 reservation 一 permit）；
    /// 5. 新建 `Issued` permit 并写入账本。
    ///
    /// 【副作用/失败】所有拒绝路径都不改变账本。permit 只是本地账本事实，不触发任何外部动作。
    pub fn issue_permit(
        &mut self,
        reservation_id: &str,
        permit_id: &str,
        fence_token: u64,
    ) -> Result<AutomationEffectPermit, &'static str> {
        let reservation = self
            .reservations
            .get(reservation_id)
            .cloned()
            .ok_or("automation_effect_permit_reservation_unknown")?;
        if !valid_text(permit_id) || fence_token == 0 {
            return Err("automation_effect_permit_invalid");
        }
        if let Some(existing) = self.permits.get(reservation_id) {
            if existing.permit_id == permit_id && existing.fence_token == fence_token {
                return match existing.state {
                    AutomationPermitState::Issued => Ok(existing.clone()),
                    AutomationPermitState::Consumed => {
                        Err("automation_effect_permit_already_consumed")
                    }
                };
            }
            return Err("automation_effect_permit_conflict");
        }
        let permit =
            AutomationEffectPermit::issue(&reservation, permit_id.to_owned(), fence_token)?;
        self.permits
            .insert(reservation_id.to_owned(), permit.clone());
        Ok(permit)
    }

    /// 按 permit_id 读取账本中的 permit；未找到返回 `automation_effect_permit_unknown`。
    pub fn fetch_permit(&self, permit_id: &str) -> Result<AutomationEffectPermit, &'static str> {
        self.permits
            .values()
            .find(|permit| permit.permit_id == permit_id)
            .cloned()
            .ok_or("automation_effect_permit_unknown")
    }

    /// 消耗 permit 并为该 reservation 建立恰好一个有效 attempt。
    ///
    /// 【调用者】Core 的 `dispatch_reserved_automation_effect()` 在调用任何 Brokered effect 之前
    /// 调用本方法；调用方必须重报 `expected_authority_epoch` 供账本核对，不允许“没有 reservation
    /// 也执行”。
    ///
    /// 【核心流程】
    /// 1. attempt_id 非法返回 `automation_effect_attempt_invalid`；
    /// 2. 按 permit_id 在账本中查找（不信任调用方传入的 permit 对象），未找到返回
    ///    `automation_effect_permit_unknown`；
    /// 3. 重报 epoch 与 permit/账本不一致返回 `automation_effect_permit_fence_drift`；
    /// 4. 已有 attempt：相同 permit_id + attempt_id 返回 `AlreadyStarted` 既有事实；否则返回
    ///    `automation_effect_attempt_conflict`；
    /// 5. permit 已消耗返回 `automation_effect_permit_already_consumed`；
    /// 6. 构造 attempt、把 permit 置为 `Consumed`、重算 permit 摘要并写入两处状态。
    ///
    /// 【副作用/失败】仅修改当前 ledger；拒绝路径不消耗 permit、不建立 attempt，也不产生外部效果。
    /// `Started` 只表示账本允许调用方在授权边界内执行一次 effect，不代表动作已执行。
    pub fn begin_attempt(
        &mut self,
        permit_id: &str,
        attempt_id: &str,
        expected_authority_epoch: u64,
    ) -> Result<AutomationEffectAttemptBegin, &'static str> {
        if !valid_text(attempt_id) {
            return Err("automation_effect_attempt_invalid");
        }
        let (reservation_id, permit) = self
            .permits
            .iter()
            .find(|(_, permit)| permit.permit_id == permit_id)
            .map(|(reservation_id, permit)| (reservation_id.clone(), permit.clone()))
            .ok_or("automation_effect_permit_unknown")?;
        if expected_authority_epoch != permit.authority_epoch
            || expected_authority_epoch != self.authority_epoch
        {
            return Err("automation_effect_permit_fence_drift");
        }
        if let Some(existing) = self.attempts.get(&reservation_id) {
            if existing.attempt_id == attempt_id && existing.permit_id == permit_id {
                return Ok(AutomationEffectAttemptBegin::AlreadyStarted(
                    existing.clone(),
                ));
            }
            return Err("automation_effect_attempt_conflict");
        }
        if permit.state == AutomationPermitState::Consumed {
            return Err("automation_effect_permit_already_consumed");
        }
        let reservation = self
            .reservations
            .get(&reservation_id)
            .cloned()
            .ok_or("automation_effect_reservation_store_corrupt")?;
        let attempt = AutomationEffectAttempt::issue(&reservation, &permit, attempt_id.to_owned())?;
        if let Some(stored) = self.permits.get_mut(&reservation_id) {
            stored.state = AutomationPermitState::Consumed;
            stored.permit_digest = stored.digest();
            stored.validate()?;
        }
        self.attempts.insert(reservation_id, attempt.clone());
        Ok(AutomationEffectAttemptBegin::Started(attempt))
    }

    /// 按 reservation_id 读取已登记 attempt；未登记返回 `automation_effect_attempt_unknown`。
    ///
    /// 这是“已开始但结果未知”时的安全重取入口：调用方用它读取既有 attempt 并对账，而不是用
    /// 相同/不同 attempt_id 再次执行 effect。
    pub fn fetch_attempt(
        &self,
        reservation_id: &str,
    ) -> Result<AutomationEffectAttempt, &'static str> {
        self.attempts
            .get(reservation_id)
            .cloned()
            .ok_or("automation_effect_attempt_unknown")
    }
}

/// 校验 ID、revision 和 capability 文本形状；不验证其业务命名空间或外部记录。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

/// 校验 `sha256:` 前缀及 64 位十六进制文本，不重新计算被引用的外部内容。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
