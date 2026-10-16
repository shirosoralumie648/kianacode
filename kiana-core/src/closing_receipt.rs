//! CO-35 Company ClosingReceipt 在 Core 中的投影/校验适配器。
//!
//! 项目收尾的真实数据流是 `CompanyState/EventLog transition -> typed
//! CompanyClosingReceiptContract -> CompanyClosingReceiptLedger`。本文件只把已经构造好的
//! receipt 交给 Domain ledger 校验并记录；它不从聊天文本、模型自述或一次 runtime result
//! 推断“项目已关闭”，也不替代现有 Company closeout/EventLog authority。
//!
//! ```text
//! packet/attempt/run/review/acceptance/delivery/incident/evidence facts
//!        -> CompanyClosingReceiptContract
//!        -> 【当前文件】record_closing_receipt
//!        -> Domain validate + idempotent ledger record
//!        -> Company read model / closing projection
//! ```
//!
//! Success、Failure、Cancelled、Waived 各有不同的 stop、acceptance、delivery、incident、
//! residual obligation 和角色独立条件；本 Core 适配器不放宽这些条件、不提交 Company transition、
//! 不写 EventLog，也不把成功返回提升为 durable/live/physical 业务结果。

use kiana_domain::{CompanyClosingReceiptContract, CompanyClosingReceiptLedger};

pub(crate) fn record_closing_receipt(
    // `ledger` 是调用方持有的 Company 事实投影；本函数不创建第二份全局 ledger。
    ledger: &mut CompanyClosingReceiptLedger,
    receipt: CompanyClosingReceiptContract,
) -> Result<(), &'static str> {
    // Domain ledger 负责 validate、相同 digest 的幂等重放、重复项目拒绝和保留完整 receipt。
    ledger.record(receipt)
}
