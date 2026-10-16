//! AUT-23 五入口 UAT parity DTO 的 Core 只读校验 facade。
//!
//! 外部调用方提供 `AutomationSurfaceUat` → 本文件转调 Domain validator → 返回成功或错误字符串。此处不
//! 启动 CLI/Web/Workbench/Desktop/MCP、不建立连接、不查询 daemon、EventStore、Broker 或 handler 计数；
//! 也不执行五入口 UAT。当前产品路径没有调用本 facade。它只校验一份由外部提供的 parity 声明。

use kiana_domain::AutomationSurfaceUat;

/// 校验 AUT-23 五入口观测矩阵，并原样传播 Domain 的 `aut23_*` 错误。
///
/// 【调用者】Core crate `lib.rs` 重导出此函数；当前没有生产入口调用。AUT-23 Core guard 是静态源码标记
/// 检查，不是五入口 runtime UAT。
///
/// 【输入/输出】借用调用方构造的 `AutomationSurfaceUat`；全部规则通过返回 `Ok(())`，否则返回首个领域
/// 错误字符串。case 数量、surface 覆盖、命令/快照/版本一致性、拒绝调用数和摘要校验均在 Domain。
///
/// 【副作用/失败】同步纯转发，不修正数据、不重试、不记录或提交观测事件，也不会把错误变成 UI 文案。
/// 通过不代表入口实际共享一个 DaemonHost，也不证明成功路径执行过 handler。
///
/// 【设计原因】Core 只暴露 Domain 唯一矩阵规则，不在 adapter 中再实现一套 parity 判断或第二条执行路径。
pub fn validate_automation_surface_uat(uat: &AutomationSurfaceUat) -> Result<(), String> {
    uat.validate()
}
