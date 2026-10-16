//! DEP-11 部署健康聚合器的 Core 薄门面。
//!
//! 【系统位置】
//! `DaemonHost` 把调用方收集的启动、EventLog、投影、租约、Provider 和 operation 事实
//! 交给这里；本模块再转发到 `kiana-domain::HealthAggregationReport` 进行确定性校验和
//! 聚合。主要数据流为：
//!
//! ```text
//! typed HealthAggregationInput
//!     -> DaemonHost route
//!     -> aggregate_health() / Domain::evaluate()
//!     -> HealthAggregationReport + HealthSnapshot
//! ```
//!
//! 这条路径与 `health.rs` 的 EventLog 全量读取投影不同：这里不采集 EventStore、Provider
//! 或租约状态，也不检查 digest 所引用对象的真实性。当前 workspace 里 Domain evaluate
//! 由 fixture 直接调用；DaemonHost 暴露了可调用方法，但暂未发现仓库内 CLI、UI、HTTP 或
//! 其他生产调用者。返回的 `admission_allowed` 只是报告字段，不会在这里改变接纳策略。

use kiana_domain::{HealthAggregationInput, HealthAggregationReport};

/// 校验调用方提供的 typed evidence，并计算对应的部署健康报告。
///
/// 【调用者】由 `kiana-daemon::health_aggregation::evaluate()` 转发；公开 Core helper
/// 目前没有额外的仓库内生产调用点。
///
/// 【输入与输出】输入包含 source cursor/event IDs、启动与 operation 状态、租约围栏、
/// EventLog/projector 状态、Provider evidence 标记及其摘要。成功时返回 Domain 根据这些
/// 值确定性重算出的 `HealthAggregationReport`。
///
/// 【失败与副作用】任何 schema、字段、phase、digest 或 report 绑定校验失败都会把 Domain
/// 返回的稳定字符串错误原样传播。此函数不读 EventStore、不探测 Provider/Broker、不
/// 写 EventLog、不执行恢复，也不实际拦截 admission。
pub fn aggregate_health(input: &HealthAggregationInput) -> Result<HealthAggregationReport, String> {
    HealthAggregationReport::evaluate(input)
}

/// 检查报告是否能由指定的输入重新确定性计算得到。
///
/// 它先校验输入与报告自身，再构造期望报告并比较完整结构；因此仅重新计算一个匹配的
/// `report_digest` 并不能把伪造报告绑定到另一个输入。当前没有仓库内生产调用者，主要
/// 由需要再次核对报告与输入的适配器或校验流程使用。它不重新读取输入引用的外部事实。
pub fn validate_health_aggregation(
    input: &HealthAggregationInput,
    report: &HealthAggregationReport,
) -> Result<(), String> {
    report.validate_against(input)
}
