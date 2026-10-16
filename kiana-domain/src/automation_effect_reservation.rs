//! AUT-14 效果预留事实、版本围栏和进程内幂等账本。
//!
//! # 系统位置与数据流
//!
//! ```text
//! Core / 调用方构造 AutomationEffectReservation
//!     -> validate() 检查字段格式及 reservation_digest 自洽
//!     -> AutomationReservationLedger::reserve()
//!     -> 比较 authority/config/policy 快照
//!     -> 用 idempotency_key 查 BTreeMap
//!     -> 返回旧记录，或规范为 Reserved 后写入内存 map 并返回
//! ```
//!
//! 本文件属于底层领域契约，定义一个“某个执行准备进行某个动作”的可序列化事实，以及一个仅在当前
//! 进程内有效的内存 ledger。AUT-14 baseline 将它描述为 bounded ledger，但当前 `BTreeMap` 没有容量、
//! 过期清理或删除机制，因此本实现本身并不限制记录总数。当前未找到生产代码调用该账本，也没有通往
//! Broker、EventLog、worker 或外部副作用的调用链；`Reserved` 不是 permit 消耗或动作成功的证据。
//!
//! 摘要是无密钥的 SHA-256 内容指纹，作用是发现载荷不一致，不是签名、身份认证或授权证明。账本的
//! authority/config/policy 值由构造者提供并在每次 reserve 时与 fact 对照；这里不主动读取权威状态。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// AUT-14 预留事实使用的固定序列化 schema 名称。
pub const AUTOMATION_RESERVATION_SCHEMA: &str = "kiana.automation-effect-reservation.v1";

/// 预留事实记录的声明状态。
///
/// 当前 ledger 只会在新建记录时把状态规范成 `Reserved`；本文件没有提交、消耗、未知结果恢复等状态
/// 转移方法。枚举中的其他值可由反序列化输入提供，但不能据此推断对应生命周期已实现。
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
    /// 用于将重复 reserve 请求映射到同一记录的幂等键；ledger 以它作为 map key。
    pub idempotency_key: String,
    /// 拥有该动作意图的执行 ID；不查询执行记录是否存在或仍有效。
    pub execution_id: String,
    /// 待绑定动作内容的 `sha256:` 摘要；本模块不重建或执行动作。
    pub action_digest: String,
    /// 动作申请的 capability 名称；只是被绑定的声明，不会在这里授权。
    pub capability: String,
    /// 动作所依据的授权语义 epoch；必须非零，并且 reserve 时与 ledger epoch 相同。
    pub authority_epoch: u64,
    /// 配置版本标识；reserve 时与 ledger 记录的配置快照逐字比较。
    pub config_revision: String,
    /// 策略版本标识；reserve 时与 ledger 记录的策略快照逐字比较。
    pub policy_revision: String,
    /// budget 事实摘要；不读取额度或占用预算。
    pub budget_digest: String,
    /// 可写路径/scope 声明摘要；不在本模块打开路径或检验 sandbox。
    pub path_scope_digest: String,
    /// 可选审批摘要；None 表示此 fact 未绑定审批摘要，并不表示系统已判定无需审批。
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

/// 针对一份固定 authority/config/policy 快照的进程内 reservation 幂等表。
///
/// `entries` 按幂等键排序保存，以便给定键获得确定查找行为。结构体没有锁、持久化端口或容量参数；
/// `&mut self` 只让单次 Rust 调用通过独占借用顺序更新这个对象，并不能协调不同 ledger 副本/进程。
/// 当前没有清理、加载、提交、consume 或未知结果 reconcile API，条目会一直留在 map 中。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationReservationLedger {
    /// 此 ledger 接受的授权 epoch；调用者负责保证它是当前可信快照。
    pub authority_epoch: u64,
    /// 此 ledger 接受的配置 revision。
    pub config_revision: String,
    /// 此 ledger 接受的策略 revision。
    pub policy_revision: String,
    /// 以 idempotency_key 为 key 的内存记录；私有字段阻止模块外直接绕过 reserve 写入。
    entries: BTreeMap<String, AutomationEffectReservation>,
}

impl AutomationReservationLedger {
    /// 创建一张空的进程内幂等表，并绑定调用者提供的三项快照版本。
    ///
    /// 构造时没有加载已有记录、检查版本是否当前或校验 revision 字符串；因此新建 ledger 的正确性和
    /// 生命周期由调用方负责。ledger 丢弃或进程退出后，记录没有本模块提供的恢复来源。
    pub fn new(authority_epoch: u64, config_revision: String, policy_revision: String) -> Self {
        Self {
            authority_epoch,
            config_revision,
            policy_revision,
            entries: BTreeMap::new(),
        }
    }

    /// 校验并登记一个 reservation；相同幂等键的重放只有摘要相同才返回原记录。
    ///
    /// 【调用者/输入】Core 的 `reserve_automation_effect()` facade 只转调此方法；fact 的版本和摘要由
    /// 调用者构造，ledger 自身的版本在 `new()` 时固定。
    ///
    /// 【核心流程】
    /// 1. 先运行 fact 的形状及摘要校验，非法输入立即返回 `automation_effect_reservation_invalid`；
    /// 2. 对比 authority epoch、config revision、policy revision，任何漂移都返回 fence drift；
    /// 3. 用 idempotency_key 查 map。已有记录的 reservation_digest 相同则返回已保存副本；同 key 不同
    ///    digest 则返回 idempotency conflict；
    /// 4. 新 key 才把输入 state 规范成 `Reserved`，重算含新状态的摘要并再次验证；
    /// 5. 克隆事实后插入内存 map，并返回该事实。
    ///
    /// 【副作用/失败】仅在当前对象中插入一条记录。校验、版本漂移或幂等冲突均不会更改 map，也不会触发
    /// 自动 retry、日志、EventLog、Broker 或外部动作；调用方需显式处理领域错误。
    ///
    /// 【为什么先检查版本和幂等键】相同动作意图不能在配置/权限快照变化后被静默复用；完全相同的请求
    /// 可以重放并取回原记录，避免重复创建 reservation。保证仅限当前 ledger 对象，不跨进程。
    ///
    /// ⚠ 注意：baseline 称其为 bounded ledger，但这里的 BTreeMap 没有容量上限或过期清理；新增 key 会
    /// 持续增长。方法也没有状态迁移 API，不能把 Committed/Consumed/Unknown 当成已实现生命周期。
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
        if let Some(existing) = self.entries.get(&reservation.idempotency_key) {
            if existing.reservation_digest == reservation.reservation_digest {
                return Ok(existing.clone());
            }
            return Err("automation_effect_reservation_idempotency_conflict");
        }
        reservation.state = AutomationReservationState::Reserved;
        reservation.reservation_digest = reservation.digest();
        reservation.validate()?;
        self.entries
            .insert(reservation.idempotency_key.clone(), reservation.clone());
        Ok(reservation)
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
