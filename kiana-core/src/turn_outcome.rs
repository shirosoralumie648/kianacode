//! # 文件职责
//!
//! 这是 Core 与 Domain TurnOutcome 判定器之间的薄门面：把 Domain reducer 的结果再校验一次，或将
//! 可信调用方生成的 outcome 注入结构化 JSON。它不自行读取 pending 状态、不创建事件，也不将模型输出
//! 当作 outcome authority。
//!
//! # 调用链位置
//!
//! ```text
//! Core lifecycle 收到 RunnerEvent::Completed
//!   -> 本文件 propose_turn_outcome(input)
//!   -> kiana-domain::TurnOutcome::decide(input) + validate()
//!   -> annotate_output(output, outcome)
//!   -> lifecycle 脱敏并记录 run.completed，后续 receipt 从事实流投影
//! ```
//!
//! 当前调用点只为完成事件传入 `TurnOutcomeInput::completed()`；等待、取消和未知结果分别由 lifecycle
//! 的其他分支处理。文件里的输入 helper 接受完整 flags/合同，但本门面不会替调用方采集这些事实。

use kiana_domain::{TurnOutcome, TurnOutcomeInput};
use serde_json::Value;

/// 带 Core 侧 TurnOutcome 标注的结构化结果格式名称，对外导出但本文件不据此分支处理。
pub const TURN_OUTCOME_CORE_SCHEMA: &str = "kiana.core-turn-outcome.v1";

/// 通过 Domain reducer 提议一份 TurnOutcome，并在 Core 边界再次校验。
///
/// 【调用者】当前由 lifecycle 的 RunnerEvent::Completed 处理分支调用。
/// 【输入/输出】接收完整 `TurnOutcomeInput`，返回结构有效的 `TurnOutcome`。
/// 【失败情况】Domain 识别到无效身份、合同或引用时原样传播错误；第二次 validate 失败也返回稳定错误。
/// 【副作用】无 IO/事件/授权状态变更；本函数只判断传入事实，不会回查 Runner、approval 或 job 状态。
pub fn propose_turn_outcome(input: TurnOutcomeInput) -> Result<TurnOutcome, String> {
    let outcome = TurnOutcome::decide(input)?;
    outcome.validate()?;
    Ok(outcome)
}

/// 将已校验的 outcome 加进 Harness 结构化结果，并拒绝已有的同名字段。
///
/// 【调用者】lifecycle 在记录 terminal `run.completed` 事实前调用。
/// 【输入】按值取得一个 JSON `Value` 和引用的 outcome；所有权转移意味着本函数修改的是交入的值，
/// 不会通过共享引用回写到别处。
/// 【输出】成功时返回带 `turn_outcome` 属性的对象。
/// 【失败情况】outcome 无效时传播 Domain 错误；顶层不是 JSON object 时返回
/// `turn_output_object_required`；对象已有 `turn_outcome` 时返回 `turn_output_outcome_already_present`；
/// outcome 序列化失败时返回 `turn_outcome_encode_failed`。
/// 【副作用/设计】只修改内存 JSON；拒绝同名字段可避免模型或前序 adapter 预先放入的声明覆盖 Core
/// 生成值，但校验摘要仍不是密码学来源认证。此函数不写 Receipt/EventLog。
pub fn annotate_output(mut output: Value, outcome: &TurnOutcome) -> Result<Value, String> {
    outcome.validate()?;
    let object = output
        .as_object_mut()
        .ok_or_else(|| "turn_output_object_required".to_owned())?;
    if object.contains_key("turn_outcome") {
        return Err("turn_output_outcome_already_present".to_owned());
    }
    object.insert(
        "turn_outcome".to_owned(),
        serde_json::to_value(outcome).map_err(|_| "turn_outcome_encode_failed".to_owned())?,
    );
    Ok(output)
}
