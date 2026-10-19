//! 本文件定义 AUT-20 补偿计划的单条领域数据契约和结构校验器。
//!
//! # 在系统中的位置
//!
//! ```text
//! 调用方构造/反序列化 AutomationCompensationPlan
//!     -> validate() 按固定顺序拒绝：结构漂移 -> 身份碰撞 -> 原 permit 复用
//!        -> 补偿动作未变化 -> 授权代次不新鲜
//!     -> 返回 Ok 或稳定拒绝码
//! ```
//!
//! Core 提供只读转发 facade；`kiana-workflow::plan_compensation_admission` 在 durable planner 上
//! 证明准入合同（源实例终态、原 attempt 存在、补偿实例独立）。当前生产 Rust 源码没有调用者：
//! 本类型不会启动补偿 workflow、重新批准 capability、写入事件、调用执行器或合并子结果。
//! 字段通过校验不能证明原执行确实存在，或所谓 fresh authorization 是由权威系统新签发的。
//!
//! `plan_digest` 是无密钥 JSON 内容摘要，仅用于检测字段与摘要是否同步；不能代替审批凭证。

use crate::json_digest;
use serde::{Deserialize, Serialize};

/// 补偿计划 DTO 的固定 wire schema 名称。
///
/// v2 新增 `original_instance_id`/`compensation_instance_id`/`original_authorization_epoch`/
/// `original_authorization_digest`：v1 只比较布尔声明，无法区分“原 permit 换了一份摘要”与
/// “同一份授权被再次使用”，也无法判断补偿代次是否真的在原代次之后。
pub const AUTOMATION_COMPENSATION_SCHEMA: &str = "kiana.automation-compensation.v2";

/// 字段形状、schema 或自摘要不自洽时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_PLAN_INVALID: &str = "automation_compensation_plan_invalid";
/// 补偿实例/执行身份与源实例/执行重合时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_IDENTITY_COLLISION: &str =
    "automation_compensation_identity_collision";
/// `reused_original_permit` 为真，或 fresh/原授权摘要规范化后相等时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_ORIGINAL_PERMIT_REUSED: &str =
    "automation_compensation_original_permit_reused";
/// 两个动作摘要规范化后相等（含仅大小写不同）时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_ACTION_UNCHANGED: &str =
    "automation_compensation_action_unchanged";
/// 补偿 authority epoch 不严格大于原授权 epoch 时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_AUTHORIZATION_NOT_FRESH: &str =
    "automation_compensation_authorization_not_fresh";

/// 补偿计划声称的生命周期值；此枚举不实施 Planned→Authorized→Completed 状态机。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCompensationState {
    /// 补偿计划已形成，尚未声称获得新授权。
    Planned,
    /// 计划声称获得授权；本 validator 不读取授权事件核实该声明。
    Authorized,
    /// 计划声称补偿已完成；本 validator 不检查执行回执。
    Completed,
    /// 补偿运行的结果不明确；枚举值不自动启动 Reconcile。
    Unknown,
}

/// 把原执行身份/动作摘要和拟执行的补偿身份/授权摘要绑定在一起的计划 DTO。
///
/// `deny_unknown_fields` 使 Serde 拒绝多余 JSON 字段。`validate()` 检查两个实例/执行 ID 不同、
/// attempt/epoch 非零、摘要形状有效、原 permit 未被复用、补偿动作确实不同、授权代次严格更晚，
/// 以及计划摘要匹配；它不查询原执行事实、当前 authority epoch、审批事件或实际补偿动作内容，
/// 也不检查 state 是否按顺序变化。真实存在性/终态由 `kiana-workflow` 的准入规划器对照状态证明。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCompensationPlan {
    /// 必须等于 `AUTOMATION_COMPENSATION_SCHEMA`。
    pub schema: String,
    /// 被补偿的原 workflow instance ID；本 validator 不在状态中查找它。
    pub original_instance_id: String,
    /// 被补偿的原执行 ID；本 validator 不在执行账本中查找它。
    pub original_execution_id: String,
    /// 原执行的 attempt 序号；这里只要求大于 0。
    pub original_attempt: u32,
    /// 原授权所属的 authority epoch；必须严格小于补偿 epoch。
    pub original_authorization_epoch: u64,
    /// 原动作内容的 SHA-256 文本摘要；这里没有原始 action 供重新计算。
    pub original_action_digest: String,
    /// 原授权的摘要；与 fresh 摘要规范化后相等即视为复用原 permit。
    pub original_authorization_digest: String,
    /// 新补偿实例的 ID；必须非空且不能与原 instance ID 相同。
    pub compensation_instance_id: String,
    /// 新补偿执行的关联 ID；必须非空且不能与原 execution ID 相同。
    pub compensation_execution_id: String,
    /// 补偿动作内容的 SHA-256 文本摘要；规范化后必须与原摘要不同。
    pub compensation_action_digest: String,
    /// 补偿使用的 authority epoch；必须严格大于 `original_authorization_epoch`。
    pub authority_epoch: u64,
    /// 调用方声称新授权的摘要；仅检查格式，未核实其审批来源或与补偿动作的绑定。
    pub fresh_authorization_digest: String,
    /// 必须为 false，表示不复用原执行的 permit；这仍是载荷声明，不会实际消费授权。
    pub reused_original_permit: bool,
    /// 计划声称的生命周期状态；只被纳入 digest，不校验转移历史。
    pub state: AutomationCompensationState,
    /// 对本计划其余字段的 SHA-256 内容摘要，不是签名或执行回执。
    pub plan_digest: String,
}

impl AutomationCompensationPlan {
    /// 按固定顺序校验计划自身的格式和静态绑定约束，不执行补偿，也不认证其外部来源。
    ///
    /// 【拒绝顺序】结构/schema/摘要 → 身份碰撞 → 原 permit 复用 → 补偿动作未变化 →
    /// 授权代次不新鲜。固定顺序让同一份坏载荷总是得到同一个拒绝码，便于审计。
    /// 【成功】仅代表 DTO 在领域层自洽；原实例/执行是否真实存在、源实例是否可补偿由
    /// `kiana_workflow::plan_compensation_admission` 对照 `AutomationState` 证明。
    /// 【副作用】无事件、授权查询、预算、文件、worker 或网络操作。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_COMPENSATION_SCHEMA
            || !valid_text(&self.original_instance_id)
            || !valid_text(&self.original_execution_id)
            || self.original_attempt == 0
            || self.original_authorization_epoch == 0
            || !valid_digest(&self.original_action_digest)
            || !valid_digest(&self.original_authorization_digest)
            || !valid_text(&self.compensation_instance_id)
            || !valid_text(&self.compensation_execution_id)
            || !valid_digest(&self.compensation_action_digest)
            || self.authority_epoch == 0
            || !valid_digest(&self.fresh_authorization_digest)
            || !valid_digest(&self.plan_digest)
            || self.plan_digest != self.digest()
        {
            return Err(AUTOMATION_COMPENSATION_PLAN_INVALID);
        }
        // 1. 补偿必须是独立实例/执行，不能把原身份当成补偿身份重复提交。
        if self.compensation_instance_id == self.original_instance_id
            || self.compensation_execution_id == self.original_execution_id
        {
            return Err(AUTOMATION_COMPENSATION_IDENTITY_COLLISION);
        }
        // 2. 原 permit 复用：布尔声明或摘要相等都拒绝。摘要比较走规范化，避免伪造大小写绕过。
        if self.reused_original_permit
            || canonical_digest(&self.fresh_authorization_digest).as_deref()
                == canonical_digest(&self.original_authorization_digest).as_deref()
        {
            return Err(AUTOMATION_COMPENSATION_ORIGINAL_PERMIT_REUSED);
        }
        // 3. 补偿动作必须与原动作不同；仅大小写不同的同一摘要也要拒绝。
        if canonical_digest(&self.compensation_action_digest).as_deref()
            == canonical_digest(&self.original_action_digest).as_deref()
        {
            return Err(AUTOMATION_COMPENSATION_ACTION_UNCHANGED);
        }
        // 4. 补偿必须有自己的、严格更晚的授权代次，而不是继承原执行。
        if self.authority_epoch <= self.original_authorization_epoch {
            return Err(AUTOMATION_COMPENSATION_AUTHORIZATION_NOT_FRESH);
        }
        Ok(())
    }

    /// 对除 plan_digest 外的字段计算内容摘要，摘要提供者可自行重算，因此不是授权证明。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "original_instance_id": self.original_instance_id,
            "original_execution_id": self.original_execution_id,
            "original_attempt": self.original_attempt,
            "original_authorization_epoch": self.original_authorization_epoch,
            "original_action_digest": self.original_action_digest,
            "original_authorization_digest": self.original_authorization_digest,
            "compensation_instance_id": self.compensation_instance_id,
            "compensation_execution_id": self.compensation_execution_id,
            "compensation_action_digest": self.compensation_action_digest,
            "authority_epoch": self.authority_epoch,
            "fresh_authorization_digest": self.fresh_authorization_digest,
            "reused_original_permit": self.reused_original_permit,
            "state": self.state,
        }))
    }

    /// 用当前字段重算 `plan_digest` 并校验，返回可直接提交的计划快照。
    ///
    /// 摘要不提供机密性/签名；它只保证字段与摘要同步，调用方仍须通过真实 authority 链。
    pub fn seal(mut self) -> Result<Self, &'static str> {
        self.plan_digest = self.digest();
        self.validate()?;
        Ok(self)
    }
}

/// 对外领域 validator，仅转调计划自身校验；当前没有生产执行路径调用它。
pub fn validate_automation_compensation(
    plan: &AutomationCompensationPlan,
) -> Result<(), &'static str> {
    plan.validate()
}

/// 检查文本非空、最长 512 UTF-8 字节且不包含 NUL/换行；并不验证 ID 是否真实存在。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\n', '\r'])
}

/// 检查 SHA-256 摘要的文本外形，接受大小写十六进制字符，不核验原始内容。
fn valid_digest(value: &str) -> bool {
    canonical_digest(value).is_some()
}

/// 返回小写十六进制摘要；形状非法时返回 `None`。仅用于规范化比较，不改变原始载荷。
fn canonical_digest(value: &str) -> Option<String> {
    value
        .strip_prefix("sha256:")
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(|hex| format!("sha256:{}", hex.to_ascii_lowercase()))
}
