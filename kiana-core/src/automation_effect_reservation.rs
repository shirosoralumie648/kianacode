//! AUT-14 效果预留事实的 Core 门面。
//!
//! 【系统位置】Domain 定义预留事实与进程内 `AutomationReservationLedger`；本文件仅把 Core 调用方
//! 的 ledger 和 reservation 交给 Domain `reserve()`：
//!
//! ```text
//! Core 调用方
//!     -> reserve_automation_effect(&mut ledger, reservation)
//!     -> kiana-domain::AutomationReservationLedger::reserve()
//!     -> 检查 fact / authority-config-policy 围栏 / 幂等键
//!     -> 原 reservation 或写入内存 ledger 后返回
//! ```
//!
//! AUT-14 当前是 source-level 合约片段。未发现生产调用者；这里没有连接 Broker、provider、EventLog、
//! worker，也没有持久化、崩溃恢复、permit 消耗或实际副作用执行。返回 reservation 不代表外部动作已
//! 获准或发生。具体事实校验、幂等比较和无界内存存储行为见 Domain 文件。

use kiana_domain::{AutomationEffectReservation, AutomationReservationLedger};

/// 在调用方提供的进程内 ledger 中尝试登记一份效果预留事实。
///
/// 【调用者】当前未找到生产路径；这是供 Core 代码使用的薄 facade。
///
/// 【输入】`ledger` 是可变借用，限定本次操作在 Rust 调用期间串行修改该 ledger；`reservation` 包含
/// action/capability、authority/config/policy 版本、budget/path scope、可选 approval digest 和幂等键。
/// 这些值由调用者提供，facade 不查询当前授权或真实资源状态。
///
/// 【输出】新幂等键成功时返回状态被设为 `Reserved` 的事实；相同幂等键和完全相同 reservation digest
/// 会返回已存记录；版本围栏漂移或同键不同摘要会返回领域错误码。
///
/// 【副作用与失败】成功的新记录只写入传入的内存对象。错误向上传播；此处不做 retry、不落库、不发
/// EventLog 事件，也不撤销或执行外部效果。调用方最终如何向用户呈现这些错误不由本模块决定。
///
/// 【为什么保留 facade】让 Core 消费者依赖 Domain 中的单一校验/幂等实现，避免重复规则；同时本层
/// 刻意没有自行添加第二套授权或执行循环。
pub fn reserve_automation_effect(
    ledger: &mut AutomationReservationLedger,
    reservation: AutomationEffectReservation,
) -> Result<AutomationEffectReservation, &'static str> {
    ledger.reserve(reservation)
}
