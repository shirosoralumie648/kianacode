//! Runner-facing facade for the shared bounded progress reducer.
//!
//! 本模块 re-export `kiana-domain::progress_stall` 的公开类型和函数，并提供 Runner 侧
//! 的证据适配器：把一次已经返回的 `CapabilityResult`/`ToolObservation` 折成
//! `ProgressEvidence`。证据只来自真实观察（状态、错误分类、退出码、输出摘要 digest 与
//! job cursor/heartbeat），模型自称“在推进”不单独构成证据。
//!
//! ## 主要对象
//!
//! - `ProgressEvidence`：观察摘要、workspace/artifact revision、验证引用摘要及 job cursor
//!   等证据；模型文本、call id 和请求 id 不进入摘要，避免每次调用都伪造“新进展”。
//! - `ProgressInput`：结构体，携带证据、可选失败摘要、空 turn 和 stop-hook 反馈，
//!   不是 ModelAttempt/ToolCall/Reject 这样的事件枚举。
//! - `ProgressTracker::observe()`：更新窗口，识别停滞、重复失败和 A→B→A 循环，
//!   并返回 `ProgressDecision`；没有单独的 `decide()` 方法。
//! - `ProgressAction`：Continue、FeedbackOnce、RequestClarification 或 Blocked。
//!   这些是 reducer 的诊断结果，不是授权或 sandbox 许可。
//! - `failure_digest()`：校验失败原因的长度和基本格式后生成摘要，不执行内容脱敏。
//!
//! ## 当前接线边界
//!
//! `ActiveRun` 持有 tracker，harness 在每次已知工具结果和空 turn 时调用 `observe()`，
//! 按决定注入一次性反馈、进入等待澄清或停止 turn；checkpoint/restore 负责携带和校验
//! tracker。反馈文本由 `bounded_feedback_text()` 生成，只描述原因计数，不复制观察正文。
//! 连续重复工具调用检查仍是 `KianaHarness::record_tool_call()`，与本 reducer 的证据窗口
//! 互补：前者管“同一调用反复”，后者管“不同调用但观察没有变化”。

pub use kiana_domain::{
    failure_digest, ProgressAction, ProgressDecision, ProgressEvidence, ProgressInput,
    ProgressTracker, DEFAULT_NO_PROGRESS_LIMIT, DEFAULT_PROGRESS_WINDOW,
};

use kiana_domain::{CapabilityResult, ToolObservation};
use serde_json::{json, Value};

/// 从一次已知工具结果构造进度证据。
///
/// 观察摘要由服务端校验过的工具名、结果状态、错误分类、退出码和输出 digest 组成；两个
/// 内容完全相同的结果会产生相同证据（因此“没有变化”会被识别为停滞），不同结果不会因为
/// 工具名相同而被误伤。`cursor`/`next_offset`/`heartbeat` 等数值字段作为 job 等待进度
/// 单独保留，供长 poll 在输出正文不变但游标推进时证明进展。函数不读取模型文本、call id
/// 或请求 id，避免每次调用都伪造“新进展”。
pub(crate) fn evidence_for_result(
    tool: &str,
    result: &CapabilityResult,
    observation: &ToolObservation,
) -> Result<ProgressEvidence, String> {
    let output = result.output.as_object();
    let numeric = |key: &str| {
        output
            .and_then(|object| object.get(key))
            .and_then(Value::as_u64)
    };
    ProgressEvidence::new(
        &json!({
            "tool": tool,
            "status": observation.status,
            "error_code": observation.error_code,
            "exit_code": observation.exit_code,
            "output_digest": observation.output_digest,
        }),
        None,
        None,
        &[],
        numeric("cursor").or_else(|| numeric("next_offset")),
        numeric("heartbeat"),
    )
}

/// 判断一次结果是否带有后台 job/进程等待语义（`job_handle`/`process_id`/`cursor`/`heartbeat`）。
///
/// 只有这类结果和已知失败才进入停滞阶梯：普通成功结果的 ack 形状相同并不代表没有推进，
/// 把不同操作的同形 ack 当成“无进展”会误杀合法流程；精确重复调用由 `record_tool_call`
/// 覆盖。带 job 语义的 poll 才按同一 JobHandle 的 cursor/heartbeat 变化判断等待是否有界。
pub(crate) fn is_job_observation(result: &CapabilityResult) -> bool {
    let Some(output) = result.output.as_object() else {
        return false;
    };
    ["job_handle", "process_id", "cursor", "heartbeat"]
        .iter()
        .any(|key| output.contains_key(*key))
}

/// 空 turn 的证据：没有工具调用、没有文本，因此不携带任何观察字段。
///
/// 空 turn 的证据刻意保持恒定，使连续空回答命中同一停滞阶梯，而不是被当成新观察。
pub(crate) fn empty_turn_evidence() -> Result<ProgressEvidence, String> {
    ProgressEvidence::new(&json!({"empty_turn": true}), None, None, &[], None, None)
}

/// 把一次失败观察折成重复失败指纹：状态 + 错误分类 + 退出码。
///
/// 输出正文摘要不进入指纹，因为“相同失败反复修复”关注的是失败类别；观察摘要已经单独
/// 记录内容变化，并且任何可观测进展都会重置重复失败窗口。
pub(crate) fn failure_key(observation: &ToolObservation) -> String {
    json!({
        "status": observation.status,
        "error_code": observation.error_code,
        "exit_code": observation.exit_code,
    })
    .to_string()
}

/// 生成有界的一次性反馈文本（供 FeedbackOnce 注入模型上下文）。
///
/// 文本只包含原因和计数，不复制观察正文，也不宣称已经执行任何操作；长度受 `format!`
/// 固定模板约束。反馈次数由 tracker 阶梯与 repair 预算共同限制，不会无限追加催促消息。
pub(crate) fn bounded_feedback_text(reason: &str, consecutive_stall: u32) -> String {
    format!(
        "harness-progress-feedback: no observable progress in recent steps \
         (reason: {reason}; consecutive_stall: {consecutive_stall}). \
         Do not repeat the same attempt: make a state-changing tool call, cite new evidence, \
         or ask one concrete clarifying question. This bounded nudge is not repeated indefinitely."
    )
}
