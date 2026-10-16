//! AUT-22 自动化快照 validator 的 Core 只读 facade。
//!
//! 输入的 `AutomationSnapshot` 由调用方提供 → 本文件转调 Domain `validate()` → 返回成功或字段错误字符串。
//! 当前没有生产 builder/调用者；此处不读 EventStore、不查询投影、不提供 UI/scheduler query endpoint，
//! 也不 claim queue、消费审批、启动 workflow 或修改 incident。不要与 Core `automation.rs` 中读取事件并
//! planner 折叠的 `workflow_snapshot()` 命令混淆：它返回不同 DTO，不经过此文件。

use kiana_domain::AutomationSnapshot;

/// 校验调用方提供的 AUT-22 聚合查询视图，并原样返回 Domain 错误。
///
/// 【调用者】Core `lib.rs` 重导出该入口，但在当前 workspace 中未找到生产代码调用；现有 Core guard 只是
/// 静态源码检查，Domain fixture 手工构造快照。
///
/// 【输入/输出】借用 `AutomationSnapshot`；成功返回 `Ok(())`，失败返回 `aut22_*` 字段错误字符串。
/// 具体头部、嵌套对象、workflow 绑定、去重和摘要条件均由 `kiana-domain::AutomationSnapshot::validate()`
/// 实现，本层不复制规则。
///
/// 【副作用/边界】同步纯转发，不改快照、不读取当前游标/authority、不构建投影、不重试或记录事件；通过
/// 只说明输入满足 Domain 实际编码的本地条件，不能证明查询结果完整、新鲜、真实或已经接入产品界面。
///
/// 【设计原因】Core facade 让上层通过稳定 crate 边界使用 Domain contract，同时保持只读，不建立绕过
/// ControlPlane 的第二个查询/执行事实源。
pub fn validate_automation_snapshot(snapshot: &AutomationSnapshot) -> Result<(), String> {
    snapshot.validate()
}
