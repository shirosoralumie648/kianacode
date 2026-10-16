//! DEP-12 部署接纳决策的 Core 门面，属于 `kiana-core`。
//!
//! 【调用链】外部调用者 → `DaemonHost::deployment_admission()` → Daemon helper → 本文件 →
//! `kiana_domain::DeploymentAdmissionDecision::{evaluate,validate_against}`。
//!
//! Core 在这里不组装 health、startup、lease 或 operation 快照；它把调用者提供的 Domain DTO 交给确定性 reducer，
//! 并暴露一个供上层重验 decision/input 绑定的函数。源码中没有生产调用者消费这些导出函数，因此这条 route 目前
//! 只会计算决策值，不是 ControlPlane/scheduler 的准入拦截点。Domain 返回的 `String` 错误在本层原样传播。

use kiana_domain::{DeploymentAdmissionDecision, DeploymentAdmissionInput};

/// 根据 caller supplied input 计算部署接纳决策。
///
/// 【调用者】Daemon 的 `deployment_admission::evaluate()` 转调本函数；`DaemonHost` 再公开该 route。当前仓库没有
/// 生产代码调用宿主方法或把结果送入实际 scheduler intake。
/// 【输入】调用方提供的健康/启动、maintenance、pause/drain、migration/backup、lease epoch、operation 与时钟快照。
/// 【输出/失败】合法 input 返回 decision，即使状态是 `Blocked`/`Unknown` 也仍为 `Ok`；结构或摘要错误以
/// Domain `String` 错误返回。Daemon helper 随后将这类错误映射为 `PortError::Failed`。
/// 【副作用】只委托 Domain reducer；不读写 EventStore，不获取 lease、不暂停队列、不停止 operation、不重试或记录日志。
pub fn evaluate_admission(
    input: &DeploymentAdmissionInput,
) -> Result<DeploymentAdmissionDecision, String> {
    DeploymentAdmissionDecision::evaluate(input)
}

/// 校验已得到的 decision 是否由给定 input 完整重算得到。
///
/// 【调用者】适用于未来需要接收/解码 decision 的上层复核；当前仓库生产源码没有调用者。
/// 【输入/输出】借用 input 和 decision，调用 Domain `validate_against()` 重算所有状态/许可/原因字段；匹配返回 `Ok(())`，
/// 格式、摘要或绑定失败时原样返回 Domain 字符串错误。
/// 【限制】重算仍建立在调用方提交的 input 上，不会验证其事实来源；也不执行准入、不写日志或重试。
pub fn validate_admission_decision(
    input: &DeploymentAdmissionInput,
    decision: &DeploymentAdmissionDecision,
) -> Result<(), String> {
    decision.validate_against(input)
}
