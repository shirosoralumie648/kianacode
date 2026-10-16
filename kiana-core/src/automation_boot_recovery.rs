//! AUT-21 启动恢复 fact 的 Core 只读转发门面。
//!
//! 调用方提供 `AutomationBootRecoveryFact` → 本文件转交 Domain validator → 返回成功或稳定错误码。
//! 当前没有生产启动流程调用此 facade；它不读取 EventStore、不重建 projection/index、不扫描 Unknown、不写
//! 恢复事件，也不恢复 worker。真实校验规则和 AUT-21 Ready 游标边界见 `kiana-domain::automation_boot_recovery`。

use kiana_domain::{validate_automation_boot_recovery, AutomationBootRecoveryFact};

/// 用领域层的静态规则校验调用方提供的启动恢复事实。
///
/// 【调用者】当前未找到 DaemonHost、ControlPlane 或生产 worker 恢复路径调用；此函数只作为 Core facade
/// 暴露给显式调用方。
///
/// 【输入/输出】借用一份 fact；成功返回 `Ok(())`，失败保留 Domain 的
/// `automation_boot_recovery_invalid` 错误码。
///
/// 【副作用】无；本函数不修改 fact，不查存储、不建立 reconcile case、不发事件、不重试。通过仅表示
/// Domain validator 所实现的字段规则通过，不能证明恢复进度来自真实事件源或投影。
///
/// 【设计原因】Core 复用 Domain 的单一契约实现，避免上层复制另一份可能漂移的状态校验；启动恢复的
/// 编排职责仍需要由实际生产调用路径承担。
pub fn validate_boot_recovery(fact: &AutomationBootRecoveryFact) -> Result<(), &'static str> {
    validate_automation_boot_recovery(fact)
}
