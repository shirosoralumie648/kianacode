//! AUT-24 release evidence DTO 的 Core 只读校验门面。
//!
//! 调用方提供 `Aut24ReleaseGate` → Core 转调 Domain `gate.validate()` → 返回 `Ok(())` 或原始字段错误。
//! 当前全仓未找到生产调用者；此处不读 CI 回执/文件、不校验审批、不运行发布动作、不写 EventStore，
//! 也不提升 CURRENT_STATUS 或 roadmap 的 proof level。通过只说明 Domain 编码的字段/摘要规则满足，
//! 不等于证据真实、发布已批准或实现达到 durable/live/physical。

use kiana_domain::Aut24ReleaseGate;

/// 使用 Domain 的单一规则检查 AUT-24 gate 声明，并保留领域错误字符串。
///
/// 【调用者】Core `lib.rs` 重导出供 crate 使用，但当前没有 Core/Daemon/Entrypoint 生产路径调用；Domain
/// fixture 直接验证对象，Core guard 是静态 `include_str!` 标记检查。
///
/// 【输入/输出】借用 gate；成功返回 `Ok(())`，失败原样返回 Domain 的 `aut24_*` 错误。
///
/// 【副作用】无同步纯校验，不检查 snapshot/workflow/evidence_ref 文件是否存在，不读取 CI、审批或状态账本，
/// 不重试、不记日志、不触发发布。它不会自行把 source proof 升为更高层级。
///
/// 【设计原因】Core 暴露稳定入口但不复制领域校验，也不因 release-evidence DTO 而新增第二份发布权限来源。
pub fn validate_automation_release_gate(gate: &Aut24ReleaseGate) -> Result<(), String> {
    gate.validate()
}
