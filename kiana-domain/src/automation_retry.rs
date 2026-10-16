//! 本文件定义 AUT-16 的有界重试决策输入和纯分类器，不真正预留或执行新尝试。
//!
//! # 在系统中的位置
//!
//! ```text
//! 调用侧整理一次执行的状态快照
//!     -> AutomationRetryInput::validate 检查 schema / 摘要 / 数值形状
//!     -> classify_automation_retry 返回 RetryNewAttempt / NoRetry / ReconcileUnknown
//!     -> 调用侧自行决定如何预留新 attempt、重读事实或结束
//! ```
//!
//! Core 有公开转发 facade，但当前生产 Rust 源码没有找到调用者；领域测试直接覆盖此分类器。
//! durable workflow 的 `AutomationCommand::Retry` 是另一条路径，不调用本文件。当前 Core facade
//! 返回的决定不会创建 attempt、占用预算、等待 Retry-After、派发 effect、写 EventStore 或重试。
//! 因此它是 AUT-16 的 source contract，而不是现行产品中的运行时重试机制。
//!
//! `AutomationRetryInput` 是调用侧提供的事实快照：本模块只验证部分形状和 self-digest，不查询
//! idempotency、审批、预算、deadline 或 effect 的真实来源。所有策略结论都依赖调用方提交的字段。

use crate::json_digest;
use serde::{Deserialize, Serialize};

/// 此重试决策输入 DTO 的固定版本标识。
pub const AUTOMATION_RETRY_SCHEMA: &str = "kiana.automation-retry.v1";

/// 分类器返回给调用侧的策略建议；它本身不启动重试或 reconciliation。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRetryDecision {
    /// 调用侧可考虑创建一个全新的 attempt；本函数没有执行 reservation。
    RetryNewAttempt,
    /// 当前条件不满足自动重试门槛，调用侧应保留拒绝/失败处理路径。
    NoRetry,
    /// effect 已开始但结果不确定；应先读取权威事实，不能盲目发起新 effect。
    ReconcileUnknown,
}

/// 调用侧为单次重试判断提供的有界快照。
///
/// `deny_unknown_fields` 拒绝 wire JSON 中未声明字段；schema 版本变化需同步 producer/consumer。
/// `decision_digest` 绑定本结构其他字段，但无密钥，因此不能证明这些值来自 Core 或 Provider。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRetryInput {
    /// 必须精确匹配 `AUTOMATION_RETRY_SCHEMA`。
    pub schema: String,
    /// 当前执行身份，不是新的 retry attempt ID；validator 只检查非空，未查询执行 registry。
    pub execution_id: String,
    /// 当前尝试序号，从 1 开始；等于 max_attempts 时不再建议新尝试。
    pub attempt: u32,
    /// 本次执行最多允许的尝试总数，而非还能重试多少次。
    pub max_attempts: u32,
    /// 声称外部 effect 是否已开始；若已开始且结果未知，会优先要求 ReconcileUnknown。
    pub effect_started: bool,
    /// 声称 effect 是否已得到明确结果；值的真实性由调用侧保证，本模块不读取运行证据。
    pub effect_known: bool,
    /// 调用侧声称该 capability 是否允许按幂等语义安全发起新尝试。
    pub capability_idempotent: bool,
    /// 调用侧声称原审批当前仍有效；本函数不查询 approval store 或 authority epoch。
    pub approval_valid: bool,
    /// 可用于新尝试的剩余预算；0 时不建议重试，本函数不预留或扣减它。
    pub budget_remaining: u64,
    /// 调用侧分类为“临时且发生在发送前”的失败；只有这个分类允许进入 RetryNewAttempt。
    pub transient_pre_send: bool,
    /// 上游建议等待时长，单位毫秒；大于 24 小时的值在 validate 阶段拒绝。
    pub retry_after_ms: Option<u64>,
    /// 总 deadline 前可等待/运行的剩余时间，单位毫秒。
    pub deadline_remaining_ms: u64,
    /// 原始输入内容摘要；这里只检查 sha256 格式，不接收输入本体重新计算。
    pub input_digest: String,
    /// 除自身外本结构字段的摘要；只能检查快照自洽，不能认证字段来源。
    pub decision_digest: String,
}

impl AutomationRetryInput {
    /// 检查 schema、attempt 范围、Retry-After 上限、两个 digest 格式和决策摘要一致性。
    ///
    /// 任意条件失败都返回 `automation_retry_input_invalid`。这不验证字段之间的现实一致性：例如
    /// effect_started/effect_known/transient_pre_send 的组合仍由调用侧负责保证可信且不矛盾。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_RETRY_SCHEMA
            || self.execution_id.trim().is_empty()
            || self.attempt == 0
            || self.max_attempts == 0
            || self.attempt > self.max_attempts
            || self.retry_after_ms.is_some_and(|v| v > 86_400_000)
            || !valid_digest(&self.input_digest)
            || !valid_digest(&self.decision_digest)
            || self.decision_digest != self.digest()
        {
            return Err("automation_retry_input_invalid");
        }
        Ok(())
    }

    /// 对除 decision_digest 外的所有输入字段计算内容摘要，避免把摘要本身纳入自身输入。
    /// 摘要不带密钥，不能证明每个布尔值或预算值由可信来源生成。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "attempt": self.attempt,
            "max_attempts": self.max_attempts,
            "effect_started": self.effect_started,
            "effect_known": self.effect_known,
            "capability_idempotent": self.capability_idempotent,
            "approval_valid": self.approval_valid,
            "budget_remaining": self.budget_remaining,
            "transient_pre_send": self.transient_pre_send,
            "retry_after_ms": self.retry_after_ms,
            "deadline_remaining_ms": self.deadline_remaining_ms,
            "input_digest": self.input_digest,
        }))
    }
}

/// 根据一次失败/运行状态快照，给出是否允许尝试新 attempt 的有限分类。
///
/// 【调用者】Domain 测试直接调用；`kiana-core::automation_retry::classify_retry` 暴露转发门面，
/// 但当前 workspace 生产代码未发现实际调用者。durable workflow 的 Retry 命令走
/// `kiana-workflow::durable::plan_command()`，不会调用此函数。
/// 【输入】由调用侧整理的 attempt、effect 结果确定性、幂等、审批、剩余预算、发送前临时错误和
/// deadline 快照。函数只验证自摘要与部分边界，不会查询这些值的事实来源。
/// 【输出】未知 effect 优先返回 `ReconcileUnknown`；其余任何重试门槛不满足返回 `NoRetry`；全部
/// 门槛通过才返回 `RetryNewAttempt`。返回值只是建议/分类，不会分配新 ID 或改变状态。
/// 【失败】输入结构或摘要错误返回 `automation_retry_input_invalid`；策略不允许重试是正常的
/// `Ok(NoRetry)`，不是函数错误。
/// 【副作用】纯计算，无时钟、等待、预算扣减、日志、EventStore、网络或 dispatch。
///
/// ⚠ Unknown 分支在其他门槛之前，防止 `effect_started && !effect_known` 被误判为可重试。除此之外，
/// 本函数没有校验字段间一致性：若不可信输入同时给出 `effect_started=true`、`effect_known=true`
/// 与 `transient_pre_send=true`，并满足其余条件，仍可能得到 RetryNewAttempt；调用方必须基于真实
/// 运行证据构造快照，且不能把该结论当作授权或 attempt reservation。
/// Retry-After 等于或大于 deadline_remaining_ms 时返回 NoRetry；分类器不负责实际等待或退避。
pub fn classify_automation_retry(
    input: &AutomationRetryInput,
) -> Result<AutomationRetryDecision, &'static str> {
    input.validate()?;
    if input.effect_started && !input.effect_known {
        return Ok(AutomationRetryDecision::ReconcileUnknown);
    }
    if input.attempt >= input.max_attempts
        || !input.capability_idempotent
        || !input.approval_valid
        || input.budget_remaining == 0
        || input.deadline_remaining_ms == 0
        || !input.transient_pre_send
    {
        return Ok(AutomationRetryDecision::NoRetry);
    }
    if input
        .retry_after_ms
        .is_some_and(|delay| delay >= input.deadline_remaining_ms)
    {
        return Ok(AutomationRetryDecision::NoRetry);
    }
    Ok(AutomationRetryDecision::RetryNewAttempt)
}

/// 检查摘要是 `sha256:` 前缀加 64 个十六进制字符；不验证输入摘要对应的原文。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
