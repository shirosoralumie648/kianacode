//! DEP-10 确定性启动协调器的 Core 薄门面。
//!
//! 适配层把 `StartupCoordinatorRequest` 交给 Domain reducer；这里仅调用
//! `StartupCoordinatorReport::evaluate()` 或 `validate_against()` 并传播结构化字符串错误。
//! DaemonHost 适配器源码会调用 `evaluate_startup()`，但目前没有发现仓库内生产入口或进程
//! 启动路径调用公开的 `DaemonHost::evaluate_startup()`；`validate_startup_report()` 也没有
//! 生产调用者。因此不能据此声称启动时已经强制执行此 Gate。整条路径消费已准备好的摘要，
//! 不读 EventStore/文件、不获取 lease、不跑 migration/projector/capacity probe，也不自动
//! 恢复或阻止命令。

use kiana_domain::{StartupCoordinatorReport, StartupCoordinatorRequest};

/// 按 Domain 固定阶段顺序评估一份启动证据请求。
///
/// 【调用关系】预期适配路径为 `DaemonHost::evaluate_startup()` → daemon
/// `startup_coordinator::evaluate()` → 本函数 → Domain `StartupCoordinatorReport::evaluate()`。
/// daemon 的 `DaemonHost::evaluate_startup()` 方法源码会经适配器调用到这里，但该公开方法
/// 当前没有被仓库内生产入口或进程启动路径调用；调用方仍需先自行收集 typed facts。
///
/// 【输入/输出】输入绑定 manifest、storage root、trust、lease、store、migration、projector、
/// capacity 与 recovery 等摘要和阶段 facts；成功返回按 Domain 规则折叠的启动报告。
///
/// 【失败与副作用】schema、digest、阶段顺序或证据门槛不符合时返回 Domain 稳定错误字符串；
/// 本函数没有重试、日志、存储、网络、进程或 admission 副作用。
pub fn evaluate_startup(
    request: &StartupCoordinatorRequest,
) -> Result<StartupCoordinatorReport, String> {
    StartupCoordinatorReport::evaluate(request)
}

/// 验证报告是否与给定 request 对应同一组阶段状态和输入摘要。
///
/// 该函数复用 `report.validate_against(request)`，会重算预期结果并校验 request/report
/// 绑定；仅报告自身的 `report_digest` 正确还不够。当前未发现仓库内生产调用者，也不会
/// 重新读取 digest 引用的 release、storage、lease 或 recovery 对象。
pub fn validate_startup_report(
    request: &StartupCoordinatorRequest,
    report: &StartupCoordinatorReport,
) -> Result<(), String> {
    report.validate_against(request)
}
