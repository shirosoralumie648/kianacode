//! DaemonHost 运维状态（status）、诊断（doctor）和预检（preflight）的只读适配层。
//!
//! 【在系统中的位置与数据流】
//! ```text
//! DaemonHost::ops_diagnostics / DaemonHost::ops_mode
//!     -> 本文件 evaluate/evaluate_mode
//!     -> kiana-core 统一检查与委托
//!     -> kiana-domain 校验输入并构造报告
//!     -> 返回 report 或 PortError 给路由调用者
//! ```
//! 这里不负责生成四轴事实。当前路由没有生产调用者，且仓库未发现生产 facts collector；因此“通过 DaemonHost”目前是接口/委托关系，
//! 不是已经连接 CLI、Workbench、Web 或 Desktop 的端到端功能。本文件不做 EventStore/文件/网络 I/O，不执行报告中的复现命令或 remediation。

use kiana_domain::{OpsDiagnosticsInput, OpsDiagnosticsMode, OpsDiagnosticsReport};
use kiana_ports::PortError;

/// DaemonHost 内部路由用的默认 reducer 入口。
///
/// 【调用者】由 DaemonHost 运维诊断路由调用；目前没有已发现的 CLI/UI/HTTP 生产消费者。
/// 【输入/输出】接收调用方组装的诊断 input，返回聚合 report。
/// 【失败情况】Core/Domain 的稳定错误字符串原样包装为 `PortError::Failed`，由更外层路由决定如何呈现。
/// 【副作用/设计原因】这里仅保持 daemon/port 错误边界，不采集事实、不记日志、不重试，也不改变 Host 或账号状态。
pub(crate) fn evaluate(input: &OpsDiagnosticsInput) -> Result<OpsDiagnosticsReport, PortError> {
    kiana_core::evaluate_ops_diagnostics(input).map_err(PortError::Failed)
}

/// 显式模式路由入口；要求 URL/命令选择的模式与 input 内声明一致。
///
/// 【调用者】`DaemonHost::ops_mode` 路径。
/// 【输入/输出】接收事实 input 和请求模式，成功时返回同一 Domain reducer 产生的 report。
/// 【失败情况】模式不匹配或其他 Core/Domain 校验错误均通过 `PortError::Failed` 返回；本 adapter 不区分用户可恢复错误类别。
/// 【副作用/限制】无外部 I/O。mode 目前只校验并回显，不切换 facts 采集方式；上游如果没有生成事实，adapter 不会补采集。
pub(crate) fn evaluate_mode(
    input: &OpsDiagnosticsInput,
    mode: OpsDiagnosticsMode,
) -> Result<OpsDiagnosticsReport, PortError> {
    kiana_core::evaluate_ops_mode(input, mode).map_err(PortError::Failed)
}
