//! AUT-19 FanOut/FanIn 的 Core 只读门面。
//!
//! 领域计划/账本与字段校验位于 `kiana-domain::automation_fanout`；本文件只把 Core 调用方收到的
//! plan 交给领域层，并原样返回稳定错误码：
//!
//! ```text
//! Core / adapter 调用方
//!     -> validate_fanout_plan(plan)              // v1 兼容校验
//!     -> admit_fanout_plan(&plan_v2)             // 校验并登记账本，推导 admission 事实
//!     -> merge_fanout_ledger(&ledger, inputs)    // 稳定顺序 fan-in，拒绝缺失/未知/重复
//!     -> Ok(..) 或 automation_fanout_* 稳定错误码
//! ```
//!
//! 当前未找到生产调用点。此门面不创建 workflow instance、不做权限检查、不拆分或并发执行 child、
//! 不分配 budget/scope、不写 EventLog，也不调用 Broker/handler。它与 `kiana-workflow::durable` 中
//! 已有的 DAG FanOut/FanIn 节点不是同一条实现路径；调用方不能把这里的 `Ok(..)` 当作运行时动作
//! 已获准或已完成。

use kiana_domain::{
    validate_automation_fanout, AutomationFanoutAdmission, AutomationFanoutFanInInput,
    AutomationFanoutLedger, AutomationFanoutMerge, AutomationFanoutPlan, AutomationFanoutPlanV2,
};

/// 对一个 AUT-19 v1 FanOut 计划执行领域结构校验，并保留领域层的错误码。
///
/// 【调用者】当前未找到生产调用者；公开给 Core 内部模块或后续 adapter 使用。
///
/// 【输入】`plan` 是调用方已经构造好的父执行、子执行和约束声明；本函数不从数据库或 EventStore
/// 加载真实父执行，也不验证这些 ID 是否存在。
///
/// 【输出】计划字段符合领域 validator 规则时返回 `Ok(())`，否则返回静态错误码。
///
/// 【副作用与失败】只读且同步，不修改计划、不分配资源、不写日志或事件；失败直接透传。
///
/// 【设计原因】Core facade 固定依赖方向：Core 复用底层 `kiana-domain` 的规则，避免再复制一份
/// 可能与领域 DTO 漂移的校验。它仍然只是静态校验入口，不是 durable workflow 的执行调度器。
pub fn validate_fanout_plan(plan: &AutomationFanoutPlan) -> Result<(), &'static str> {
    validate_automation_fanout(plan)
}

/// 校验 v2 计划并登记一个只含事实的账本，返回由真实 scope/ID 推导的 admission。
///
/// 【调用者】当前未找到生产调用者；用于 Core 侧记录“计划已接纳”的只读投影。
///
/// 【输入】`plan` 必须已经通过 `AutomationFanoutPlanV2::new()` 或等价校验；本函数会再次校验，
/// 不信任构造路径或反序列化输入。
///
/// 【输出】成功时返回 `AutomationFanoutAdmission`，其中 `child_scope_is_subset` 与
/// `parent_id_in_children` 由真实 ScopeSet/ID 计算，不是调用方布尔声明。
///
/// 【副作用与失败】不启动 child、不派发 capability、不写事件；失败原样返回
/// `automation_fanout_*` 稳定错误码。返回的账本由调用方自行持有，本函数不持久化。
pub fn admit_fanout_plan(
    plan: &AutomationFanoutPlanV2,
) -> Result<(AutomationFanoutLedger, AutomationFanoutAdmission), &'static str> {
    let ledger = plan.admit()?;
    let admission = ledger.admission();
    Ok((ledger, admission))
}

/// 对账本执行稳定顺序的 fan-in 合并，并保留领域拒绝码。
///
/// 【输入】`ledger` 是先前由 `admit_fanout_plan()` 或领域账本构造的只读事实来源；`inputs` 是
/// child 证据输入。`(child_id, sequence)` 必须唯一、child 必须已接纳且已进入终态。
///
/// 【输出】条目按 `(child_id, sequence)` 字典序排列，父状态由事实推导。
///
/// 【副作用与失败】只读；不写事件、不派发能力。缺失/未知/重复/未完成输入返回对应的
/// `automation_fanout_fan_in_*` 错误码。
pub fn merge_fanout_ledger(
    ledger: &AutomationFanoutLedger,
    inputs: &[AutomationFanoutFanInInput],
) -> Result<AutomationFanoutMerge, &'static str> {
    ledger.merge(inputs)
}
