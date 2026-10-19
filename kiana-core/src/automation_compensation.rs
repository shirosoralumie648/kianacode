//! 本文件是 AUT-20 补偿合同的只读 Core facade。
//!
//! ```text
//! Core / adapter 调用方
//!     -> validate_compensation_plan(&plan)                       // 领域静态校验
//!     -> admit_compensation_plan(state, &plan, authority, proof, live_epoch)
//!         // 对照当前状态与 live 授权代次，复用 kiana-workflow durable planner
//!     -> Ok(next_state, admission, effect) 或 automation_compensation_* 稳定错误码
//! ```
//!
//! 门面不提交事件、不消费审批、不派发 capability、不启动 workflow 或写 EventStore；`effect`
//! 原样来自 planner（`Compensate` 恒为 `None`），调用方仍须走 ControlPlane 的既有提交与授权链。
//! 当前生产 Rust 源码没有调用者，`handle_workflow_command` 的 `Compensate` 分支也不经过本门面。

use kiana_domain::{
    AutomationAuthority, AutomationCompensationPlan, AutomationProof, AutomationState,
    WorkflowEffect,
};
use kiana_workflow::{plan_compensation_admission, AutomationCompensationAdmission};

/// 将单条计划交给 Domain 校验器并原样返回结果。
///
/// 错误为稳定的 `automation_compensation_*` 码；成功只代表计划字段自洽，不代表授权已验证或
/// 补偿执行已经发生。
pub fn validate_compensation_plan(plan: &AutomationCompensationPlan) -> Result<(), &'static str> {
    plan.validate()
}

/// 对照当前状态与 live 授权代次，判定补偿计划是否可登记为独立补偿实例。
///
/// 【输入】`state` 必须是调用方从 EventStore 折叠出的当前投影；`live_authority_epoch` 必须是
/// 调用方从权威授权账本读到的当前代次（生产应使用
/// `ControlPlane::observed_authority_epoch()`），本门面不读取账本、不认证其来源。
///
/// 【输出】成功返回 planner 生成的下一状态、准入事实和 effect（恒为 `None`）；源实例不被改写。
///
/// 【失败】原实例缺失/非 Failed·Cancelled、原执行不存在或 attempt 不符、补偿执行身份不是本次
/// authority execution、重复补偿、无补偿节点、授权代次陈旧或 planner 拒绝，均返回稳定码。
///
/// 【副作用】纯函数，不写事件、不派发能力、不调用 Broker/runner/model。
pub fn admit_compensation_plan(
    state: &AutomationState,
    plan: &AutomationCompensationPlan,
    authority: &AutomationAuthority,
    proof: &AutomationProof,
    live_authority_epoch: u64,
) -> Result<
    (
        AutomationState,
        AutomationCompensationAdmission,
        Option<WorkflowEffect>,
    ),
    &'static str,
> {
    plan_compensation_admission(state, plan, authority, proof, live_authority_epoch)
}
