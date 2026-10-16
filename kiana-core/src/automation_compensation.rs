//! 本文件是 AUT-20 补偿计划 Domain validator 的只读 Core facade。
//!
//! 它只暴露转发函数，不在 Core 内创建补偿实例、检查真实审批、提交事件或执行副作用；当前生产
//! Rust 源码没有调用者。durable workflow 的 `Compensate` 命令不经过此 facade。

use kiana_domain::{validate_automation_compensation, AutomationCompensationPlan};

/// 将单条计划交给 Domain 校验器并原样返回结果。
///
/// 错误为 `automation_compensation_plan_invalid`；成功只代表计划字段自洽，不代表授权已验证或
/// 补偿执行已经发生。
pub fn validate_compensation_plan(plan: &AutomationCompensationPlan) -> Result<(), &'static str> {
    validate_automation_compensation(plan)
}
