//! DaemonHost 到 Core 统一 shutdown reducer 的内部转发层，属于 `kiana-daemon`。
//!
//! 预期调用链为 `DaemonHost::deployment_shutdown()` → 本文件 `evaluate()` → Core `evaluate_shutdown()` →
//! Domain `ShutdownReport::evaluate()`。此处负责将 Domain `String` 错误映射到 Daemon 的 `PortError`，不执行关闭动作。
//! 当前仓库没有生产代码调用宿主方法或生成 input，因此这只是 additive 的纯计算 route，不是实际 shutdown coordinator。

use kiana_domain::{ShutdownInput, ShutdownReport};
use kiana_ports::PortError;

/// 经 Core 计算 shutdown report，并转换 input/report 合同错误的类型。
///
/// 【调用者】`DaemonHost::deployment_shutdown()` 调用本 helper；当前该公开方法没有仓库内生产调用者。
/// 【输入/输出】借用上游组装的 Domain `ShutdownInput`；合法输入总会返回一份分类 report，`NeedsRecovery`/`Unknown`
/// 不会被转换为 `Err`。结构、自摘要校验失败时，Core 的 `String` 被包装为 `PortError::Failed`。
/// 【副作用】只做错误类型映射；不触发 cancel/drain/flush/close，不改共享状态，不记录日志或自动重试。
/// 调用者必须另行处理 report 状态，不能把 `Ok(report)` 当作资源已停止的证明。
pub(crate) fn evaluate(input: &ShutdownInput) -> Result<ShutdownReport, PortError> {
    kiana_core::evaluate_shutdown(input).map_err(PortError::Failed)
}
