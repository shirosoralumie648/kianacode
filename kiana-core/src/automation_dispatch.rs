//! # 文件职责
//!
//! 本文件是一个只读的 Core facade（门面）：把公开函数 `observe_automation_dispatch()` 转交给
//! `kiana-domain::validate_automation_observation()`。真正的字段、状态和摘要校验在 Domain；这里
//! 不拥有自动化队列，不选择执行 worker，也不调用 handler。
//!
//! # 输入、处理与输出
//!
//! ```text
//! 调用方准备 AutomationDispatchIntent（先前记录的 intent 快照）
//!       + AutomationObservation（调用方提交的执行结果快照）
//!       -> 本文件调用 Domain validator
//!       -> 校验两者各自有效且身份/authority epoch 一致
//!       -> 返回新的 intent：Observed 或 Unknown，并绑定 observation digest
//! ```
//!
//! 【本函数实际检查什么】Domain validator 要求 intent 当前处于 `Claimed`，并比较 worker ID、
//! execution ID、reservation ID、action digest 与 authority epoch；成功或失败结果都把状态转为
//! `Observed`，只有结果未知才转为 `Unknown`。它不读取当前权限、配额、容量或 wall clock，因此只
//! 能证明两份输入快照彼此一致，不能单独证明现场仍符合最新授权。
//!
//! 【副作用和调用边界】函数是同步纯校验：不写 EventStore、不发出 `dispatch.automated.*` 事件、
//! 不重试、不重新授权，也不会真的“观察”外部 worker。当前 workspace 中未找到生产代码调用本
//! facade；Domain 契约测试直接覆盖 validator。Core 在 `lib.rs` 重导出本函数，使外部 adapter
//! 可以显式调用，但重导出不等于已经接入产品分发路径。
//!
//! ⚠ 不要把此函数的 `Ok` 当作执行许可。调用方若要将结果用于真实分发，仍须通过 ControlPlane
//! 的当前授权/关卡检查，并自行把状态变更持久化；当前文件不完成这些步骤。

use kiana_domain::{
    validate_automation_observation, AutomationDispatchIntent, AutomationObservation,
};

/// 校验自动化分发 intent 与调用方提供的 observation 是否仍绑定同一 attempt。
///
/// 【作用】作为 Core 对 Domain 校验器的公开薄封装；`observe` 是历史 API 命名，本函数不会自行
/// 查询 worker 或系统状态。
/// 【调用者】由 `kiana-core/src/lib.rs` 重导出给 crate 使用方。当前仓库未找到生产调用；Domain
/// 层的状态/绑定规则由 `kiana-domain/tests/automation_dispatch.rs` 覆盖。
/// 【输入】`intent` 是此前准备并 claim 的快照；`observation` 是调用方收集后传入的完成结果。
/// 两者都由借用传入，因此本函数不会消耗调用方持有的数据。
/// 【输出】成功时返回 `intent` 的新副本：`Succeeded`/`Failed` outcome 变成 `Observed`，`Unknown`
/// outcome 变成 `Unknown`，并把 `result_digest` 写入 `observation_digest` 后重算 `intent_digest`。
/// 【失败】Domain 校验器返回静态错误码：例如 intent 自身无效、当前状态不是 `Claimed`、worker 或
/// execution/reservation/action/authority 绑定不同、observation 格式错误。此处不把错误写日志，
/// 是否拒绝、重试或恢复由调用方决定。
/// 【副作用】无；不会访问 EventStore、检查当前 epoch、调用 Policy/Gate、发送能力请求或产生审计事件。
///
/// 【核心流程】所有实质规则都在 `kiana-domain::validate_automation_observation()`：先分别验证 intent
/// 与 observation 的格式/摘要，再逐字段比较绑定身份；只有成功后才克隆并转换状态。因为本函数
/// 只比较调用者提供的数据，`Ok` 不足以证明这些数据来自可信 worker 或当前权限仍有效。
pub fn observe_automation_dispatch(
    intent: &AutomationDispatchIntent,
    observation: &AutomationObservation,
) -> Result<AutomationDispatchIntent, &'static str> {
    validate_automation_observation(intent, observation)
}
