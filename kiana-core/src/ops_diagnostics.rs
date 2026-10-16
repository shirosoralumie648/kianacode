//! 运维状态（status）、诊断（doctor）和预检（preflight）的 Core 只读门面。
//!
//! 【在系统中的位置】
//! ```text
//! DaemonHost 路由
//!     -> 本文件：检查/转发模式，并委托给 Domain
//!     -> kiana-domain::OpsDiagnosticsReport
//!     -> Daemon adapter 将结果或错误交回调用入口
//! ```
//! Core 是规则复用边界，不负责采集 config/startup/health/observability 事实，也不访问 EventLog、文件、网络或进程。
//! 当前生产源码没有发现 facts 汇总器或 CLI/UI 对该 DaemonHost 路由的调用；因此这些函数定义了静态委托链，不证明端到端诊断已接通。
//! 详细状态优先级、字段含义及摘要边界见 `kiana-domain/src/ops_diagnostics.rs`。

use kiana_domain::{OpsDiagnosticsInput, OpsDiagnosticsMode, OpsDiagnosticsReport};

/// 将完整诊断 input 交给 Domain reducer 生成 report。
///
/// 【调用者】DaemonHost 的运维诊断 adapter；目前没有发现仓库内生产事实生产者/路由消费者。
/// 【输入/输出】输入为已装配但仍需校验的 `OpsDiagnosticsInput`；成功时返回 Domain report。
/// 【失败情况】Domain 的 header、fact、文本、状态组合或摘要校验错误以相同 `String` 返回，不会被转换成成功报告。
/// 【副作用/设计原因】本层不读写外部状态、不重试、不记录日志，只集中转发 Domain 的纯 reducer，保证入口复用同一套状态规则。
pub fn evaluate_ops_diagnostics(
    input: &OpsDiagnosticsInput,
) -> Result<OpsDiagnosticsReport, String> {
    OpsDiagnosticsReport::evaluate(input)
}

/// 在运行公共 reducer 前，确认调用方指定的模式与 input 内模式一致。
///
/// 【调用者】需要显式选择 status/doctor/preflight 的 DaemonHost adapter。
/// 【输入/输出】接收 input 与调用路由要求的 mode；一致时返回与 `evaluate_ops_diagnostics()` 相同的 report。
/// 【失败情况】不一致时立即返回 `ops_diagnostics_mode_mismatch`，不继续校验或生成 report；一致但 input 无效时传播 Domain 错误。
/// 【为什么单独检查】mode 会进入报告和复现命令，若 adapter 路由目标与 input 声明不同，返回值会看起来属于错误的运维操作。
/// 此检查只匹配 DTO 中的声明，不验证 mode 是否真的触发了不同深度的采集；三个模式目前共用相同 reducer。
pub fn evaluate_ops_mode(
    input: &OpsDiagnosticsInput,
    mode: OpsDiagnosticsMode,
) -> Result<OpsDiagnosticsReport, String> {
    if input.mode != mode {
        return Err("ops_diagnostics_mode_mismatch".to_owned());
    }
    evaluate_ops_diagnostics(input)
}

/// 用原始 input 复核现有 report 的绑定和摘要。
///
/// 【调用者】需要独立校验报告的上层 adapter；当前未发现生产调用者。
/// 【输入/输出】返回 `Ok(())` 表示 Domain 的 `validate_against()` 所实现的有限绑定检查通过。
/// 【失败情况】返回 Domain 的稳定 `String` 错误；不会修复或重新生成 report。
/// 【副作用/限制】纯委托且无外部 I/O。请同时阅读 Domain 函数注释：此校验未绑定观察时间/input digest，limitations 也只比较数量，
/// 所以它不应被误解为对事实来源或所有 report 字段的认证。
pub fn validate_ops_diagnostics(
    input: &OpsDiagnosticsInput,
    report: &OpsDiagnosticsReport,
) -> Result<(), String> {
    report.validate_against(input)
}
