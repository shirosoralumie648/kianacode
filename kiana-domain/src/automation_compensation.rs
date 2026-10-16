//! 本文件定义 AUT-20 补偿计划的单条领域数据契约和结构校验器。
//!
//! # 在系统中的位置
//!
//! 调用方构造/反序列化 AutomationCompensationPlan
//!     -> validate() 检查字段边界、原/补偿身份差异和 plan_digest
//!     -> 返回 Ok 或稳定错误
//!
//! Core 仅提供薄转发 facade，当前生产 Rust 源码没有找到实际调用者。AUT-20 目前是 source-level
//! 合约：本类型不会启动补偿 workflow、重新批准 capability、写入事件、调用执行器或合并子结果。
//! durable workflow 的 `AutomationCommand::Compensate` 是另一条已存在的 planner 路径，也不消费
//! `AutomationCompensationPlan`。字段通过校验不能证明原执行确实存在，或所谓 fresh authorization
//! 是由权威系统新签发的。
//!
//! `plan_digest` 是无密钥 JSON 内容摘要，仅用于检测字段与摘要是否同步；不能代替审批凭证。

use crate::json_digest;
use serde::{Deserialize, Serialize};

/// 补偿计划 DTO 的固定 wire schema 名称。
pub const AUTOMATION_COMPENSATION_SCHEMA: &str = "kiana.automation-compensation.v1";

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
/// `deny_unknown_fields` 使 Serde 拒绝多余 JSON 字段。`validate()` 检查两个 execution ID 不同、
/// attempt/epoch 非零、摘要形状有效、原 permit 未复用，以及计划摘要匹配；它不查询原执行事实、
/// 当前 authority epoch、审批事件或实际补偿动作内容，也不检查 state 是否按顺序变化。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCompensationPlan {
    /// 必须等于 `AUTOMATION_COMPENSATION_SCHEMA`。
    pub schema: String,
    /// 被补偿的原执行 ID；本 validator 不在执行账本中查找它。
    pub original_execution_id: String,
    /// 原执行的 attempt 序号；这里只要求大于 0。
    pub original_attempt: u32,
    /// 原动作内容的 SHA-256 文本摘要；这里没有原始 action 供重新计算。
    pub original_action_digest: String,
    /// 补偿动作内容的 SHA-256 文本摘要；只按文本比较是否与原摘要相同。
    pub compensation_action_digest: String,
    /// 新补偿执行的关联 ID；必须非空且不能与原 execution ID 相同。
    pub compensation_execution_id: String,
    /// 计划使用的 authority epoch；只要求非零，不与当前控制面代次比较。
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
    /// 校验计划本身的格式和若干静态绑定约束，不执行补偿，也不认证其外部来源。
    ///
    /// 【检查】schema、原/补偿 execution ID 的非空与不同、attempt/epoch 非零、动作/授权/计划
    /// digest 格式、两个 action digest 文本不相等、原 permit 未复用，以及 plan_digest 自洽。
    /// ID 长度上限为 512 UTF-8 字节，且拒绝 NUL/CR/LF，但不拒绝所有内部空白。
    /// 【失败】任一条件不满足返回 `automation_compensation_plan_invalid`；成功仅代表 DTO 自洽。
    /// 【副作用】无事件、授权查询、预算、文件、worker 或网络操作。
    ///
    /// 注意：digest 格式接受大小写十六进制，但两个动作摘要用原始字符串直接比较；同一摘要以
    /// 不同大小写表示时会被当成不同值。是否影响 AUT-20 的“补偿动作必须不同”契约需另行判断。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_COMPENSATION_SCHEMA
            || !valid_text(&self.original_execution_id)
            || self.original_attempt == 0
            || !valid_digest(&self.original_action_digest)
            || !valid_digest(&self.compensation_action_digest)
            || self.original_action_digest == self.compensation_action_digest
            || !valid_text(&self.compensation_execution_id)
            || self.compensation_execution_id == self.original_execution_id
            || self.authority_epoch == 0
            || !valid_digest(&self.fresh_authorization_digest)
            || self.reused_original_permit
            || !valid_digest(&self.plan_digest)
            || self.plan_digest != self.digest()
        {
            return Err("automation_compensation_plan_invalid");
        }
        Ok(())
    }

    /// 对除 plan_digest 外的字段计算内容摘要，摘要提供者可自行重算，因此不是授权证明。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "original_execution_id": self.original_execution_id,
            "original_attempt": self.original_attempt,
            "original_action_digest": self.original_action_digest,
            "compensation_action_digest": self.compensation_action_digest,
            "compensation_execution_id": self.compensation_execution_id,
            "authority_epoch": self.authority_epoch,
            "fresh_authorization_digest": self.fresh_authorization_digest,
            "reused_original_permit": self.reused_original_permit,
            "state": self.state,
        }))
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
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
