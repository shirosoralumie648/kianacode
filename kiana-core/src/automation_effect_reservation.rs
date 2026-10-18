//! AUT-14 效果预留、permit 与 dispatch 门面的 Core 薄封装。
//!
//! 【系统位置】Domain 定义预留事实、execution binding、permit、attempt 与进程内
//! `AutomationReservationLedger`；本文件把 Core 调用方的登记、预留、permit 签发和
//! “先验证再执行”的 dispatch 顺序交给 Domain：
//!
//! ```text
//! Core 调用方
//!     -> register / reserve_automation_effect(&mut ledger, reservation)
//!     -> issue_automation_effect_permit(&mut ledger, reservation_id, permit_id, fence_token)
//!     -> dispatch_reserved_automation_effect(&mut ledger, permit_id, attempt_id, epoch, effect)
//!     -> Domain ledger 先核对 permit/epoch/attempt，返回 Started 才调用 effect
//! ```
//!
//! AUT-14 当前是 source-level 合约片段。未发现生产调用者；这里没有连接 Broker、provider、
//! EventLog、worker，也没有持久化、崩溃恢复或实际副作用执行。返回 reservation/permit 不代表外部
//! 动作已获准或发生。`dispatch_reserved_automation_effect` 是唯一把 effect 调用排在验证之后的
//! 门面：拒绝路径不会触碰注入的 effect 闭包。具体事实校验、幂等比较和无界内存存储行为见 Domain
//! 文件；durable CAS 语义由 `kiana-eventlog` 的 reservation 存储适配器承载（同样是 CI-语义，
//! 不是 durable 证明）。

use kiana_domain::{
    AutomationEffectAttempt, AutomationEffectReservation, AutomationReservationLedger,
};

/// 在调用方提供的进程内 ledger 中尝试登记一份效果预留事实。
///
/// 【调用者】当前未找到生产路径；这是供 Core 代码使用的薄 facade。
///
/// 【输入】`ledger` 是可变借用，限定本次操作在 Rust 调用期间串行修改该 ledger；`reservation` 包含
/// action/capability、authority/config/policy 版本、budget/path scope、可选 approval digest 和幂等键。
/// 这些值由调用者提供，facade 不查询当前授权或真实资源状态。
///
/// 【输出】新幂等键成功时返回状态被设为 `Reserved` 的事实；相同幂等键和完全相同 reservation digest
/// 会返回已存记录；版本围栏漂移、未知 execution、scope/approval/budget/path drift 或同键不同摘要会
/// 返回领域错误码。
///
/// 【副作用与失败】成功的新记录只写入传入的内存对象。错误向上传播；此处不做 retry、不落库、不发
/// EventLog 事件，也不撤销或执行外部效果。调用方最终如何向用户呈现这些错误不由本模块决定。
pub fn reserve_automation_effect(
    ledger: &mut AutomationReservationLedger,
    reservation: AutomationEffectReservation,
) -> Result<AutomationEffectReservation, &'static str> {
    ledger.reserve(reservation)
}

/// 为一份已登记 reservation 签发至多一份一次性 permit。
///
/// 【调用者】当前未找到生产路径；ControlPlane 在提交 reservation 事实后、调用 Broker 之前转调。
///
/// 【输入】`reservation_id` 必须已在同一 ledger 中成功 reserve；`permit_id` 有界且非空；`fence_token`
/// 非零。调用方不能自行构造 permit 对象，ledger 只认自己保存的副本。
///
/// 【输出】`Issued` permit；相同 permit_id/fence_token 的重放幂等返回原副本，已消耗的 permit 返回
/// `automation_effect_permit_already_consumed`，换 permit 标识返回 `automation_effect_permit_conflict`。
///
/// 【副作用与失败】只写内存 ledger；不落库、不调用 Broker，也不执行任何动作。
pub fn issue_automation_effect_permit(
    ledger: &mut AutomationReservationLedger,
    reservation_id: &str,
    permit_id: &str,
    fence_token: u64,
) -> Result<kiana_domain::AutomationEffectPermit, &'static str> {
    ledger.issue_permit(reservation_id, permit_id, fence_token)
}

/// 先消耗 permit 建立唯一 attempt，再把服务端 effect 交给注入的闭包执行。
///
/// 【调用者】这是 Core 侧“先事实后副作用”的最小 dispatch 门面：任何要执行 Brokered effect 的
/// 调用方都必须先经过它，而不是在缺少 reservation/permit/attempt 的情况下直接调用 Broker。
/// 当前仓库未找到生产调用；它是 AUT-15 把 inline dispatch 改造成可重取 command 时的接入点。
///
/// 【输入】`permit_id` 必须在 ledger 中登记；`attempt_id` 是本次唯一尝试的稳定 ID；
/// `expected_authority_epoch` 由调用方重报，必须与 permit 和 ledger 当前 epoch 一致；`effect`
/// 是调用方提供的“真正调用 Broker/handler”的闭包。
///
/// 【核心流程】`begin_attempt` 只有在 permit 有效、epoch 匹配、该 reservation 尚无 attempt 时才
/// 返回 `Started`，此时才调用 `effect`。相同 attempt 的重放返回
/// `automation_effect_attempt_already_started` 错误，调用方应改为读取既有 attempt 并对账，绝不会
/// 二次调用 Broker。任何拒绝（未 reservation、未知 permit、旧 epoch、scope drift 在上游 reserve
/// 阶段已拒绝、attempt 冲突等）都发生在 `effect` 被调用之前，因此 Broker 调用次数保持 0。
///
/// 【输出/失败】成功返回 `effect` 的返回值；拒绝返回 Domain 的稳定错误码。effect 自身的失败语义
/// 由闭包决定，本门面不把闭包结果改写成成功或失败事实，也不重试。
///
/// 【副作用与边界】本门面自身只更新内存 ledger；真正的副作用由闭包承担。它不验证 `effect` 是不是
/// 合法的 Broker 调用，只保证“没有通过 permit/attempt 检查就不会被调用”这一顺序。
pub fn dispatch_reserved_automation_effect<T>(
    ledger: &mut AutomationReservationLedger,
    permit_id: &str,
    attempt_id: &str,
    expected_authority_epoch: u64,
    effect: impl FnOnce(&AutomationEffectAttempt) -> T,
) -> Result<T, &'static str> {
    let begin = ledger.begin_attempt(permit_id, attempt_id, expected_authority_epoch)?;
    if !begin.started() {
        return Err("automation_effect_attempt_already_started");
    }
    Ok(effect(begin.attempt()))
}
