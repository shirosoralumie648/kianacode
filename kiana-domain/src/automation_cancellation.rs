//! 本文件定义 AUT-17 取消代次与迟到结果 fencing 的单条领域事实格式及其结构校验器。
//!
//! # 在系统中的位置
//!
//! ```text
//! 外部调用方构造/反序列化 AutomationCancellationFact
//!     -> validate_automation_cancellation() 检查字段组合与 fact_digest
//!     -> Core automation_cancellation facade 原样转发结果
//! ```
//!
//! 当前生产 Rust 源码未发现该 fact 或 validator 的构造/调用者；Core facade 也只做转发。AUT-17
//! 因而仍是 source-level 的取消事实契约，不会真实停止 worker、递增持久化 cancellation generation、
//! 拦截迟到结果或恢复运行。项目 durable workflow 的 `Cancel` 命令走 Core Automation 另一条链路：
//! planner 记录 CancelRequested，Core 在事件提交后尝试取消受管运行，再由 Reconcile 读取事实。
//! 不要把本文件的字段校验误当成那条路径已经使用的 fencing 机制。

use crate::json_digest;
use serde::{Deserialize, Serialize};

/// 取消事实 DTO 的固定 wire schema 名称。
pub const AUTOMATION_CANCEL_SCHEMA: &str = "kiana.automation-cancel.v1";

/// 一条取消事实声称的阶段；枚举只限制可用字符串，不存储或执行阶段转换。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCancelState {
    /// 已记录取消意图，尚未请求底层停止。
    Requested,
    /// 已请求底层停止，但尚无确认事实。
    Stopping,
    /// 已确认底层停止。
    Stopped,
    /// effect 已开始，但无法确认结果或停止情况。
    ResultUnknown,
    /// 迟到结果已被认定需要隔离，不能覆盖当前事实。
    Fenced,
}

/// 描述一次执行尝试取消进展和迟到结果隔离关系的事实 DTO。
///
/// `deny_unknown_fields` 会在 Serde 反序列化时拒绝未知字段。结构带 execution/attempt 身份、取消
/// generation、authority epoch、停止请求/确认标记和原因；`validate()` 只校验单个对象中的部分
/// 逻辑组合，不查询当前 attempt、generation、authority 或事件历史，因此不保证代次单调、owner
/// 有权取消或状态确实按 Requested→Stopping→Stopped 顺序发生。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCancellationFact {
    /// 必须精确匹配 `AUTOMATION_CANCEL_SCHEMA`。
    pub schema: String,
    /// 被取消的 execution 关联 ID；这里不查找真实 worker/运行记录。
    pub execution_id: String,
    /// 被取消的具体 attempt 序号；只要求非零，不验证 attempt 是否实际存在。
    pub attempt: u32,
    /// 取消 generation，用来表示哪一代取消意图；只要求非零，不与当前持久 generation 比较。
    pub cancel_generation: u64,
    /// 取消决定时声称的 authority epoch；只要求非零，不与现行控制面 epoch 比较。
    pub authority_epoch: u64,
    /// 此 fact 声称的取消阶段。
    pub state: AutomationCancelState,
    /// 声称底层外部 effect 是否已经开始；Unknown 状态必须为 true。
    pub effect_started: bool,
    /// 是否已把 stop 请求送给底层 worker/provider；确认停止前必须先有此标记。
    pub stop_requested: bool,
    /// 是否收到可以作为“已停止”依据的确认；true 时 stop_requested 也必须为 true。
    pub stop_confirmed: bool,
    /// 是否有结果晚于本次取消代次到达；若为 true，状态必须是 Fenced。
    pub late_result: bool,
    /// 取消原因文本；不能为空，但本 validator 不限定原因类别或来源。
    pub reason: String,
    /// 对本 fact 其他字段的 JSON `sha256:` 摘要；无密钥，不能认证产生者。
    pub fact_digest: String,
}

impl AutomationCancellationFact {
    /// 校验单条取消事实的 schema、非空/非零身份字段、若干状态组合和摘要一致性。
    ///
    /// 全局约束包括 stop_confirmed 必须有 stop_requested；Requested 不能已经请求 stop；Stopped
    /// 必须有确认；ResultUnknown 必须声称 effect 已开始；late_result 只能搭配 Fenced。
    /// 其他组合仍可能通过，例如 Stopping 不强制 stop_requested=true。是否需要更强矩阵取决于
    /// AUT-17 的完整状态转移契约，当前校验器不检查历史顺序。
    ///
    /// 无效 DTO 统一返回 `automation_cancellation_fact_invalid`。本函数不写事件、不发 stop、不
    /// 更新 generation，也不阻止 worker 上报 late result；其 `Ok(())` 只表示这条 fact 通过本地检查。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_CANCEL_SCHEMA
            || self.execution_id.trim().is_empty()
            || self.attempt == 0
            || self.cancel_generation == 0
            || self.authority_epoch == 0
            || self.reason.trim().is_empty()
            || self.stop_confirmed && !self.stop_requested
            || matches!(self.state, AutomationCancelState::Requested) && self.stop_requested
            || matches!(self.state, AutomationCancelState::Stopped) && !self.stop_confirmed
            || matches!(self.state, AutomationCancelState::ResultUnknown) && !self.effect_started
            || self.late_result && self.state != AutomationCancelState::Fenced
            || !valid_digest(&self.fact_digest)
            || self.fact_digest != self.digest()
        {
            return Err("automation_cancellation_fact_invalid");
        }
        Ok(())
    }

    /// 对除 fact_digest 外的字段计算内容摘要，避免摘要对自身递归。
    /// 内容摘要无密钥，字段提供者可以在修改事实后重新计算，不能证明事实来源可信。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "attempt": self.attempt,
            "cancel_generation": self.cancel_generation,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "effect_started": self.effect_started,
            "stop_requested": self.stop_requested,
            "stop_confirmed": self.stop_confirmed,
            "late_result": self.late_result,
            "reason": self.reason,
        }))
    }
}

/// 对外领域入口，仅转调单条事实校验器；当前工作区未找到生产调用者。
pub fn validate_automation_cancellation(
    fact: &AutomationCancellationFact,
) -> Result<(), &'static str> {
    fact.validate()
}

/// 检查 SHA-256 摘要文本格式；不重新计算输入内容或认证摘要来源。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
