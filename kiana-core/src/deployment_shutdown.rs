//! DEP-13 统一 shutdown reducer 的 Core 委托层，属于 `kiana-core`。
//!
//! 【调用链】外部关闭协调器 → `DaemonHost::deployment_shutdown()` → Daemon helper → 本文件 →
//! Domain `ShutdownReport::{evaluate,validate_against}`。
//!
//! 本模块不运行取消、scheduler fence、runner/tool drain、EventStore/ArtifactStore flush 或 close；只把调用者提供的
//! shutdown snapshot 交给 Domain 计算/重验 report。源码中尚无生产代码调用 DaemonHost 方法，因此它不是进程退出流程的
//! 已接线执行器。Domain `String` 错误在这里保持原样。

use kiana_domain::{ShutdownInput, ShutdownReport};

/// 根据调用者提供的 shutdown snapshot 计算阶段回执汇总。
///
/// 【调用者】Daemon `deployment_shutdown::evaluate()` 委托本函数，再由 DaemonHost 暴露为 route；当前未发现生产调用者。
/// 【输入/输出】输入包含时间、边界布尔值、未解决计数和有序阶段回执；input 合法时返回 `Stopped`、`NeedsRecovery` 或
/// `Unknown` 的 report。后两种仍为 `Ok(report)`；结构/摘要不合法时原样返回 Domain 字符串错误。
/// 【副作用与限制】只委托 Domain reducer；不向端口派发操作、不读写 EventStore/ArtifactStore、不控制 scheduler/process，
/// 不负责重试、日志或持久化 shutdown receipt。
pub fn evaluate_shutdown(input: &ShutdownInput) -> Result<ShutdownReport, String> {
    ShutdownReport::evaluate(input)
}

/// 将 report 与原始 input 重新绑定并校验派生字段。
///
/// 【调用者】未来接收/解码 shutdown report 的协调器可在采用状态前调用；当前仓库生产源码没有调用者。
/// 【输出/失败】匹配返回 `Ok(())`；input/report 自身格式或重算 report 不一致时原样返回 Domain 字符串错误。
/// 【边界】不验证 ack 来源真实性或底层关闭结果，也不执行 recovery。
pub fn validate_shutdown_report(
    input: &ShutdownInput,
    report: &ShutdownReport,
) -> Result<(), String> {
    report.validate_against(input)
}
