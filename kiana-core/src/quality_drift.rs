//! EQ-46 drift 度量的 Core 适配器。
//!
//! 本文件处于 Core 对外导出与 Domain 纯计算合同之间：调用方先构造 `DriftEvaluationInput`，本适配器
//! 将其交给 Domain evaluator，然后把成功结果原样返回。它不加载阈值、不从事实日志组装 report、不
//! 写入事件，也不发送通知。因此目前它只是可复用的计算入口；仓内调用搜索只发现测试直接调用，
//! 尚未找到生产命令或后台任务为它准备输入。
//!
//! ```text
//! 调用方提供 DriftEvaluationInput
//!     -> evaluate_quality_drift()
//!     -> kiana-domain::evaluate_drift()
//!     -> metrics + 可选 drift.alerted 事件载荷
//!     -> 调用方决定如何保存/发布（当前仓内无生产接线）
//! ```

use kiana_domain::{evaluate_drift, DriftEvaluation, DriftEvaluationInput};

/// 事件载荷 kind 的固定标签；名称含 `COMMAND`，但这里没有注册一个同名可执行命令。
pub const DRIFT_ALERT_COMMAND: &str = "drift.alerted";

/// 对 Domain drift 评估器的 Core 公开适配入口。
///
/// 【调用者】Core fixture 直接调用；此函数由 `lib.rs` 公开 re-export。仓内没有发现生产处理器调用它，
/// 因而不能据此认为 `drift.alerted` 已接入命令、定时监控或持久化流程。
/// 【输入】调用方组装的完整 `DriftEvaluationInput`，包括 report、threshold、target/source 标签和
/// route/grant 前后摘要。
/// 【输出】成功时返回 Domain 计算的 metrics、可选 alert 与可选事件 payload；事件只是待上层处理的数据，
/// 本函数不会保存或发布它。
/// 【失败】Domain 稳定错误码原样向调用方传播；这里不包装、不重试，也不改变 Core 状态。
/// 【设计原因】把框架/命令层与纯业务判定分开，使 Domain reducer 可独立验证，同时仍让 Core crate
/// 对消费者提供一个稳定入口。
pub fn evaluate_quality_drift(
    input: &DriftEvaluationInput,
) -> Result<DriftEvaluation, &'static str> {
    evaluate_drift(input)
}
