//! 本文件是 Core 对 AUT-18 signal/checkpoint 领域校验器的只读转发门面。
//!
//! 该 facade 不读取 ControlPlane 状态、不验证 approver/authority epoch、不暂停或恢复 worker，也不
//! 写 EventStore。当前 workspace 生产 Rust 代码没有找到调用者；它只把领域 DTO 和稳定校验错误
//! 暴露在 Core API 边界，不能据此认定 AUT-18 已接入实际的 pause/resume 流程。

use kiana_domain::{validate_automation_signal, AutomationSignalFact};

/// 将单条 signal fact 交给 Domain 校验器；错误码原样返回。
///
/// 【输入】调用方已构造或反序列化的 `AutomationSignalFact`。
/// 【输出】结构自洽时返回 `Ok(())`，否则返回 `automation_signal_fact_invalid`。
/// 【副作用/边界】无 I/O、无日志、无存储写入，也不校验历史 signal_id 是否重复；当前未找到生产调用者。
pub fn validate_automation_signal_fact(fact: &AutomationSignalFact) -> Result<(), &'static str> {
    validate_automation_signal(fact)
}
