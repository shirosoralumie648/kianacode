//! One bounded accumulator for streamed and non-streamed model responses.
//!
//! Providers may deliver text, tool-argument, usage and stop fragments in different orders. The
//! accumulator owns that assembly and emits a complete `ModelOutput` only after identities,
//! limits, JSON arguments and terminal markers agree. It has no model, Broker or EventLog access.
//!
//! ---------------------------------------------------------------------------
//! 【本文件负责什么】
//! ---------------------------------------------------------------------------
//! 这个模块是**流式模型输出的规范化与验证层**。它的目的是：
//!
//! 1. **承接流式 Delta 事件**：模型 API（或 `ModelClient` 实现）可能在运行时
//!    逐块吐出 text、tool 参数、usage、stop 信号等，顺序不定。
//!
//! 2. **组装成完整响应**：把所有 Delta 片段汇总，检查完整性和一致性，
//!    最后产出一个完整且合法的 `ModelOutput`。
//!
//! 3. **限流与容错**：
//!    - 单次累积 Delta 数量上限 (4096)
//!    - 文本总字节上限 (512 KiB)
//!    - 工具参数单个字节上限 (256 KiB)
//!    - 工具块数上限 (32)
//!
//! 4. **取消安全性**：一旦收到 cancel() 信号，后续的 Delta 都会被拒绝
//!    （late_delta_after_cancel_is_discarded），确保上层 UI 和工具派发
//!    不会出现从不同版本的累积态出发的情况。
//!
//! 5. **JSON 参数验证**：流式 API 的工具参数通常分块到达，本模块缓冲后
//!    在 finish() 时做完整 JSON 验证（parse_bounded_json），不会部分应用。
//!
//! ---------------------------------------------------------------------------
//! 【在系统里的位置】
//! ---------------------------------------------------------------------------
//! ```text
//!   ModelClient::complete_streaming()
//!        │ 逐块产出 Delta
//!        ▼
//!   【本文件的 ModelStreamAccumulator】
//!        │ push(delta) ──循环── push(delta)
//!        │ └─ 检查限流、冲突、完整性
//!        ▼
//!   finish(partial_output) ← 接收非流式字段，merge/verify
//!        │ 返回 Ok(ModelOutput) | Err(error)
//!        ▼
//!   harness::model_step_once()
//!        │ 工具调用转换 + 事件发出
//!        ▼
//!   CapabilityRequest 处理
//! ```
//!
//! 每个 model attempt 创建一个新的 Accumulator；同一个 step 的重试也重新创建。
//! 它的生命周期：
//! - 创建于 invoke_model() 的 attempt 循环，持有该次 ModelAttemptId
//! - push() 零或多次，逐块接收 ModelDelta
//! - cancel() 标记取消；之后 push 和 finish 都返回错误
//! - finish() 消费 self 和最终 ModelOutput，返回通过验证的输出或错误
//! - Stop 后不再接受增量；重复 Stop 也会以 stream_duplicate_stop 拒绝
//!
//! ---------------------------------------------------------------------------
//! 【核心错误模式】
//! ---------------------------------------------------------------------------
//! | 错误 | 含义 | 恢复 |
//! |---|---|---|
//! | `stream_delta_limit` | Delta 事件数超 4096 | 返回错误；调用方应拒绝该 attempt，不发布部分结果 |
//! | `stream_text_limit` | 文本字节超 512 KiB | 停止接收，拒绝该轮 |
//! | `stream_tool_arguments_limit` | 单工具参数超 256 KiB | 停止接收，拒绝该轮 |
//! | `stream_tool_identity_conflict` | 同一 index 的 id/name 不一致 | 流损坏，拒绝该轮 |
//! | `stream_text_aggregate_mismatch` | 流式 text ≠ finish() 的 text | 数据冲突，拒绝 |
//! | `split_invalid_tool_json_has_zero_dispatches` | 工具 JSON 非法 | 拒绝，不部分应用 |
//! | `eof_without_stop_never_completes` | 增量与最终输出都未提供 stop reason | 拒绝该轮 |
//! | `late_delta_after_cancel_is_discarded` | cancel 后还有 Delta | 自动拒绝该 Delta |
//!
//! ---------------------------------------------------------------------------
//! 【Token 与预算】
//! ---------------------------------------------------------------------------
//! `ModelUsage { input_tokens, output_tokens }` 可通过 delta 更新，也可在 finish() 补充。
//! 更新时这两个计数都不得低于之前值；本模块不相加多次 usage，也不写 BudgetLedger。
//! 当前 harness 从 reply.output.usage 取结算值，结算在调用 finish() 之前，不能把此处
//! 补齐 usage 描述成已经同步修正了外部账本。
//!
//! ---------------------------------------------------------------------------
//! 【设计约束】
//! ---------------------------------------------------------------------------
//! - **无副作用**：本模块不调用 model、broker、eventlog，只做数据转换和验证
//! - **所有权转移**：finish() 按值接收并消费 self/output，在自有副本上补齐字段后返回
//! - **冲突拒绝**：文本、工具列表和 stop reason 的对应值冲突时拒绝；usage 则允许
//!   最终报告继续增长，只拒绝任一计数回退。最终输出缺少工具、usage 或 stop 时，
//!   按各字段规则从流式累积值补齐。

use kiana_domain::{
    parse_bounded_json, validate_model_calls, ModelAttemptId, ModelDelta, ModelOutput,
    ModelToolCall, ModelUsage,
};
use std::collections::BTreeMap;

pub const STREAM_NORMALIZER_MAX_DELTAS: usize = 4_096;
pub const STREAM_NORMALIZER_MAX_TEXT_BYTES: usize = 512 * 1024;
pub const STREAM_NORMALIZER_MAX_TOOL_BYTES: usize = 256 * 1024;
pub const STREAM_NORMALIZER_MAX_TOOL_BLOCKS: usize = 32;

#[derive(Clone, Debug)]
struct ToolBuffer {
    id: String,
    name: String,
    partial_json: String,
}

/// Deterministic per-attempt stream accumulator. Cancellation rejects push and finish;
/// a Stop rejects later deltas but still permits final output validation via finish.
///
/// 【状态边界】
/// push 可以多次调用；Stop 后其他 delta 返回 stream_delta_after_stop，
/// 重复 Stop 也返回错误。cancel 后所有新 delta 与 finish 都被拒绝。
/// finish 消费 self，因此成功或失败后都不能继续使用同一个 accumulator。
///
/// 【字段说明】
/// - `attempt_id`：本轮的唯一标识，用于追踪日志和事件
/// - `text`：累积的所有 text delta
/// - `text_seen`：标记是否收到过至少一个 Text delta（用于与 finish() 验证）
/// - `tools`：BTreeMap<index, ToolBuffer> ← 流式工具参数按块存储
/// - `usage`：累积的 token 用量（可多次更新，单调递增）
/// - `stop_reason`：模型停止原因（最多一次，不可覆盖）
/// - `delta_count`：已累积的 delta 总数（限流）
/// - `cancelled`：是否已取消（true 时拒绝所有新 delta）
#[derive(Clone, Debug)]
pub struct ModelStreamAccumulator {
    attempt_id: ModelAttemptId,
    text: String,
    text_seen: bool,
    tools: BTreeMap<u32, ToolBuffer>,
    usage: Option<ModelUsage>,
    stop_reason: Option<String>,
    delta_count: usize,
    cancelled: bool,
}

impl ModelStreamAccumulator {
    pub fn new(attempt_id: ModelAttemptId) -> Self {
        Self {
            attempt_id,
            text: String::new(),
            text_seen: false,
            tools: BTreeMap::new(),
            usage: None,
            stop_reason: None,
            delta_count: 0,
            cancelled: false,
        }
    }

    pub fn attempt_id(&self) -> ModelAttemptId {
        self.attempt_id
    }

    /// 标记本轮为已取消。后续 push() 和 finish() 都返回错误。
    /// 本方法不撤回已发出的 UI 文本，也不负责取消外部 I/O。
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// 接收一个流式 Delta，更新累积态。
    ///
    /// 【限流检查】（触发任何一个就拒绝）
    /// - 取消状态：late_delta_after_cancel_is_discarded
    /// - Stop 后非 Stop delta：stream_delta_after_stop
    /// - Delta 数超限：stream_delta_limit
    /// - 文本字节超限：stream_text_limit
    /// - 工具参数超限：stream_tool_arguments_limit 或 stream_tool_fragment_invalid
    /// - Usage 倒流：stream_usage_regressed
    ///
    /// 【工具参数处理】
    /// - index 范围检查、id/name 长度检查
    /// - 同一 index 的 id/name 必须一致（检测流损坏）
    /// - partial_json 逐块累积
    ///
    /// 【完整性约束】
    /// - Stop 后非 Stop delta 和重复 Stop 都返回错误。
    /// - 错误不承诺状态回滚或自动 cancel，例如 delta_count 可能已增加。
    ///   调用方应传播错误并放弃该 attempt，不能忽略错误后拼出成功结果。
    pub fn push(&mut self, delta: ModelDelta) -> Result<(), String> {
        if self.cancelled {
            return Err("late_delta_after_cancel_is_discarded".to_owned());
        }
        if self.stop_reason.is_some() && !matches!(&delta, ModelDelta::Stop { .. }) {
            return Err("stream_delta_after_stop".to_owned());
        }
        self.delta_count = self.delta_count.saturating_add(1);
        if self.delta_count > STREAM_NORMALIZER_MAX_DELTAS {
            return Err("stream_delta_limit".to_owned());
        }
        match delta {
            ModelDelta::Text { text } => {
                if self.text.len().saturating_add(text.len()) > STREAM_NORMALIZER_MAX_TEXT_BYTES {
                    return Err("stream_text_limit".to_owned());
                }
                self.text_seen = true;
                self.text.push_str(&text);
            }
            ModelDelta::ToolArguments {
                index,
                id,
                name,
                partial_json,
            } => {
                if index as usize >= STREAM_NORMALIZER_MAX_TOOL_BLOCKS
                    || id.trim().is_empty()
                    || id.len() > 256
                    || name.trim().is_empty()
                    || name.len() > 256
                    || partial_json.len() > STREAM_NORMALIZER_MAX_TOOL_BYTES
                {
                    return Err("stream_tool_fragment_invalid".to_owned());
                }
                let entry = self.tools.entry(index).or_insert_with(|| ToolBuffer {
                    id: id.clone(),
                    name: name.clone(),
                    partial_json: String::new(),
                });
                // 同一 index 必须保持相同的 id 和 name
                if entry.id != id || entry.name != name {
                    return Err("stream_tool_identity_conflict".to_owned());
                }
                if entry.partial_json.len().saturating_add(partial_json.len())
                    > STREAM_NORMALIZER_MAX_TOOL_BYTES
                {
                    return Err("stream_tool_arguments_limit".to_owned());
                }
                entry.partial_json.push_str(&partial_json);
            }
            ModelDelta::Usage { usage } => {
                // 检查 usage 的单调性（不倒流）
                if let Some(previous) = &self.usage {
                    if usage.input_tokens < previous.input_tokens
                        || usage.output_tokens < previous.output_tokens
                    {
                        return Err("stream_usage_regressed".to_owned());
                    }
                }
                self.usage = Some(usage);
            }
            ModelDelta::Stop { reason } => {
                if reason.trim().is_empty() || reason.len() > 128 {
                    return Err("stream_stop_invalid".to_owned());
                }
                if self.stop_reason.is_some() {
                    return Err("stream_duplicate_stop".to_owned());
                }
                self.stop_reason = Some(reason);
            }
            _ => return Err("model_delta_unsupported".to_owned()),
        }
        Ok(())
    }

    /// 将所有累积的 Delta 与传入的 `ModelOutput` merge，最后返回完整且合法的输出。
    ///
    /// 【Merge 逻辑】
    /// 1. 检查取消状态
    /// 2. 收到过 Text delta 时（即便文本为空），output.text 必须与累计文本相同；
    ///    没有 Text delta 时保留 output.text，并检查其字节上限。
    /// 3. 流式工具块按 index 排序解析为 JSON 对象；output.tool_calls 为空时填入，
    ///    否则两份列表必须一致。
    /// 4. Usage：output 有报告时 input/output 两个计数都必须 ≥ 流式版本；
    ///    output 缺报告时才用流式 usage 补齐。
    /// 5. Stop reason：任一来源可提供；两者都有则必须相同，都没有则拒绝。
    /// 6. 最终验证：tool_calls 合法性、content_blocks 验证、finish 状态验证
    ///
    /// 【不一致时的行为】
    /// 本函数会修改已取得所有权的 output；失败时不返回部分输出，调用方应拒绝该轮。
    pub fn finish(mut self, mut output: ModelOutput) -> Result<ModelOutput, String> {
        if self.cancelled {
            return Err("stream_cancelled_before_complete".to_owned());
        }
        if self.text_seen && output.text != self.text {
            return Err("stream_text_aggregate_mismatch".to_owned());
        }
        if !self.text_seen
            && self.text.is_empty()
            && output.text.len() > STREAM_NORMALIZER_MAX_TEXT_BYTES
        {
            return Err("stream_text_limit".to_owned());
        }
        if let Some(reason) = &self.stop_reason {
            if output
                .stop_reason
                .as_ref()
                .is_some_and(|existing| existing != reason)
            {
                return Err("stream_stop_conflict".to_owned());
            }
            output.stop_reason = Some(reason.clone());
        }
        if output.stop_reason.is_none() {
            return Err("eof_without_stop_never_completes".to_owned());
        }
        if let (Some(observed), Some(reported)) = (&self.usage, &output.usage) {
            if reported.input_tokens < observed.input_tokens
                || reported.output_tokens < observed.output_tokens
            {
                return Err("stream_usage_regressed".to_owned());
            }
        } else if output.usage.is_none() {
            output.usage = self.usage.take();
        }

        if !self.tools.is_empty() {
            let mut calls = Vec::with_capacity(self.tools.len());
            for (index, buffer) in self.tools {
                let arguments = parse_bounded_json(buffer.partial_json.as_bytes())
                    .map_err(|_| format!("split_invalid_tool_json_has_zero_dispatches:{index}"))?;
                if !arguments.is_object() {
                    return Err(format!("stream_tool_arguments_object_required:{index}"));
                }
                calls.push(ModelToolCall {
                    id: buffer.id,
                    name: buffer.name,
                    arguments,
                });
            }
            if output.tool_calls.is_empty() {
                output.tool_calls = calls;
            } else if output.tool_calls != calls {
                return Err("stream_tool_aggregate_mismatch".to_owned());
            }
        }
        validate_model_calls(&output.tool_calls).map_err(|error| error.to_string())?;
        output.content_blocks().map_err(|error| error.to_string())?;
        let has_tools = !output.tool_calls.is_empty()
            || output
                .content
                .iter()
                .any(|block| matches!(block, kiana_domain::ModelContent::ToolCall { .. }));
        kiana_domain::ModelFinish::parse(output.stop_reason.as_deref(), has_tools, false)
            .map_err(|error| error.to_string())?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::ModelToolCall;
    use serde_json::json;

    #[test]
    fn interleaved_tool_deltas_equal_nonstream_response() {
        // 验证流式 delta 的累积 ≡ 非流式 finish() 调用
        // 即：push(Text) + push(ToolArguments) + push(Stop)
        //   = finish(ModelOutput { text, tool_calls, stop_reason })
        let mut accumulator = ModelStreamAccumulator::new(ModelAttemptId::new());
        accumulator
            .push(ModelDelta::Text {
                text: "before".to_owned(),
            })
            .unwrap();
        accumulator
            .push(ModelDelta::ToolArguments {
                index: 0,
                id: "call-1".to_owned(),
                name: "shell".to_owned(),
                partial_json: "{\"command\":\"ls\"}".to_owned(),
            })
            .unwrap();
        accumulator
            .push(ModelDelta::Stop {
                reason: "tool_use".to_owned(),
            })
            .unwrap();
        let output = accumulator
            .finish(ModelOutput {
                text: "before".to_owned(),
                tool_calls: vec![ModelToolCall {
                    id: "call-1".to_owned(),
                    name: "shell".to_owned(),
                    arguments: json!({"command":"ls"}),
                }],
                stop_reason: Some("tool_use".to_owned()),
                ..ModelOutput::default()
            })
            .unwrap();
        assert_eq!(output.text, "before");
        assert_eq!(output.tool_calls.len(), 1);
    }

    #[test]
    fn invalid_json_eof_and_late_cancel_are_rejected() {
        // 测试三个关键失败情况：
        // 1. 损坏的工具 JSON（不完整）
        // 2. 没有 Stop 信号的 EOF
        // 3. Cancel 后尝试 push delta

        // 情况 1：无效 JSON ← 参数块不完整
        let mut invalid = ModelStreamAccumulator::new(ModelAttemptId::new());
        invalid
            .push(ModelDelta::ToolArguments {
                index: 0,
                id: "call-1".to_owned(),
                name: "shell".to_owned(),
                partial_json: "{\"command\":".to_owned(), // 未闭合的 JSON
            })
            .unwrap();
        invalid
            .push(ModelDelta::Stop {
                reason: "tool_use".to_owned(),
            })
            .unwrap();
        assert!(invalid
            .finish(ModelOutput {
                stop_reason: Some("tool_use".to_owned()),
                ..ModelOutput::default()
            })
            .unwrap_err()
            .starts_with("split_invalid_tool_json_has_zero_dispatches"));

        // 情况 2：EOF without stop
        let mut eof = ModelStreamAccumulator::new(ModelAttemptId::new());
        eof.push(ModelDelta::Text {
            text: "partial".to_owned(),
        })
        .unwrap();
        assert_eq!(
            eof.finish(ModelOutput {
                text: "partial".to_owned(),
                ..ModelOutput::default()
            })
            .unwrap_err(),
            "eof_without_stop_never_completes"
        );

        // 情况 3：Cancel 后不接受新 delta
        let mut cancelled = ModelStreamAccumulator::new(ModelAttemptId::new());
        cancelled.cancel();
        assert_eq!(
            cancelled
                .push(ModelDelta::Text {
                    text: "late".to_owned(),
                })
                .unwrap_err(),
            "late_delta_after_cancel_is_discarded"
        );
    }
}
