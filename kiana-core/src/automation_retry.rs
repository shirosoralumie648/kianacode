//! 本文件是 AUT-16 重试分类器的只读 Core facade，只把 Domain API 暴露到 Core 边界。
//!
//! 它不在 Core 生命周期、Runner、Provider 或 workflow command 中执行重试；当前生产 Rust 代码没有
//! 找到此 facade 的调用者。新 attempt 创建、reservation、预算扣减和事实记录仍须由实际控制面路径承担。

use kiana_domain::{classify_automation_retry, AutomationRetryDecision, AutomationRetryInput};

/// 转发 Domain 纯分类器，不额外查询 Core 状态或执行返回的决定。
///
/// 输入字段来源由调用者负责；无效 DTO 返回 `automation_retry_input_invalid`，策略拒绝是
/// `Ok(NoRetry)`，不表示 facade 已经创建/拒绝某个运行记录。
pub fn classify_retry(
    input: &AutomationRetryInput,
) -> Result<AutomationRetryDecision, &'static str> {
    classify_automation_retry(input)
}
