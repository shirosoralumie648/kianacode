//! DaemonHost 的 reconcile 只读路由适配层。
//!
//! 【系统位置与数据流】
//! ```text
//! DaemonHost::ops_reconcile(input)
//!     -> 本文件 evaluate(input)
//!     -> kiana-core::evaluate_reconcile()
//!     -> kiana-domain::ReconcileReport::evaluate()
//!     -> ReconcileReport 或 PortError 返回 Host 调用方
//! ```
//! 此适配层只负责 daemon/port 错误边界，不组装 cursor、lease、fence、approval 或 projection facts，也不执行 report 中的修复建议。
//! 当前没有生产代码调用该 Host 路由；此静态链路不表示 ops CLI/UI 已接通。

use kiana_domain::{ReconcileInput, ReconcileReport};
use kiana_ports::PortError;

/// DaemonHost 内部的只读 reconcile 转发函数。
///
/// 【调用者】`DaemonHost::ops_reconcile()`；当前没有已发现的生产调用者。
/// 【输入/输出】接收上游提供的 `ReconcileInput`，成功返回 Domain 生成的 `ReconcileReport`。
/// 【失败情况】Core/Domain 的错误字符串被包装为 `PortError::Failed`；Unknown/Blocked 等业务状态仍作为成功 report 返回，
/// 由 Host 外层调用者检查 `report.status`。
/// 【副作用/设计原因】这里只转换边界错误类型，避免 DaemonHost API 泄漏 Domain 的裸错误字符串；不访问 store、网络或进程，不重试也不提交 repair。
pub(crate) fn evaluate(input: &ReconcileInput) -> Result<ReconcileReport, PortError> {
    kiana_core::evaluate_reconcile(input).map_err(PortError::Failed)
}
