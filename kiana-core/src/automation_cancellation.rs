//! 本文件是 AUT-17 cancellation fact validator 的只读 Core 转发门面。
//!
//! 当前生产代码没有找到调用者；本文件不调用 workflow Cancel helper、不派发 stop、不持久化 generation
//! 或 fence。真实 durable workflow 取消逻辑位于 Core Automation effect 路径，与这里校验的 DTO 分离。

use kiana_domain::{validate_automation_cancellation, AutomationCancellationFact};

/// 将单条 cancellation fact 原样交给 Domain 校验器。
///
/// 有效时只返回 `Ok(())`，无效时返回 `automation_cancellation_fact_invalid`；没有其他检查或副作用，
/// 不会据此停止 execution，也不会阻止迟到结果写入。
pub fn validate_automation_cancel(fact: &AutomationCancellationFact) -> Result<(), &'static str> {
    validate_automation_cancellation(fact)
}
