//! 投影器、索引、工作队列与租约协调决策的 Core 只读门面。
//!
//! 【系统位置与数据流】
//! ```text
//! DaemonHost::ops_reconcile
//!     -> kiana-daemon adapter
//!     -> 本文件 evaluate_reconcile / validate_reconcile_report
//!     -> kiana-domain::ReconcileReport reducer 与绑定校验
//!     -> report 或错误返回路由调用方
//! ```
//! Core 负责让不同入口复用同一套 Domain 决策，不汇总真实 cursor/generation/lease/approval facts，也不访问 EventLog、投影存储或队列。
//! 当前仓库只有 Host 路由定义，没有已发现的生产调用者或 input builder；Unknown、Blocked、RepairNeeded 是 `Ok(report)` 中的业务状态。

use kiana_domain::{ReconcileInput, ReconcileReport};

/// 把输入交给 Domain 纯 reducer，生成四目标 reconcile 报告。
///
/// 【调用者】Daemon adapter `evaluate()`，再由 `DaemonHost::ops_reconcile()` 暴露；生产调用者当前未发现。
/// 【输入/输出】接收外部 facts 汇总者构造的 `ReconcileInput`，成功返回状态、诊断和可选 generation。
/// 【失败情况】Domain 校验/代数溢出错误按原 `String` 传回 daemon 层；Blocked/Unknown 本身不会转换为错误。
/// 【副作用/设计原因】无外部 I/O、日志、重试或状态修改；把决策固定在 Domain 以避免入口各自实现不同规则。
pub fn evaluate_reconcile(input: &ReconcileInput) -> Result<ReconcileReport, String> {
    ReconcileReport::evaluate(input)
}

/// 对已有报告和原始 input 重新计算并比较绑定关系。
///
/// 【调用者】需要复核输出的 adapter/入口；当前没有发现生产调用者。
/// 【输入/输出】接收原 `ReconcileInput` 与 `ReconcileReport`；Domain 绑定检查通过时返回 `Ok(())`。
/// 【失败处理】Domain 结构、决策或摘要错误按 `String` 返回，不会修复报告。
/// 【限制】只复核 input 声明间的一致性；不会回查 actor、approval、lease、epoch 或 projection store 的权威来源。
pub fn validate_reconcile_report(
    input: &ReconcileInput,
    report: &ReconcileReport,
) -> Result<(), String> {
    report.validate_against(input)
}
