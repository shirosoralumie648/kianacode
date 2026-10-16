//! DaemonHost 到 Core 纯启动协调 reducer 的私有适配层。
//!
//! 预期调用链为 DaemonHost 收到 adapter 已准备的 `StartupCoordinatorRequest` → 本模块
//! `evaluate()` → `kiana-core::evaluate_startup()` → Domain 阶段 reducer。它只把 Core 的
//! 字符串错误包装为 daemon 通用 `PortError`；这里不打开 EventStore/root、不运行迁移、不
//! 重建 projector、不探测 capacity、不获取 lease，也不把 Ready 接到命令 admission。
//! DaemonHost 的 `evaluate_startup()` 方法会调用本模块，但目前未发现该公开方法被其他生产
//! 入口或进程启动路径调用。

use kiana_domain::{StartupCoordinatorReport, StartupCoordinatorRequest};
use kiana_ports::PortError;

/// 沿 DaemonHost → Core → Domain 调用链评估给定启动 request。
///
/// 调用方必须先准备 request 中各阶段的事实和摘要；本函数不会替调用方读取或验证底层
/// 对象。Domain 拒绝输入时错误字符串转换成 `PortError::Failed` 并原样向上返回，不重试、
/// 记录状态或执行恢复动作。
pub(crate) fn evaluate(
    request: &StartupCoordinatorRequest,
) -> Result<StartupCoordinatorReport, PortError> {
    kiana_core::evaluate_startup(request).map_err(PortError::Failed)
}
