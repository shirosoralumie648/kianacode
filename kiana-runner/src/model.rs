//! Owned harness 持有的模型轮次接口与本地模型实现。
//!
//! Runner 循环只依赖 [`ModelClient`]，不直接依赖在线 provider crate。daemon 可以注入真实
//! 适配器（当前产品状态仍未证明 live provider），测试可以注入 [`ScriptedModel`]；没有
//! 可用模型时使用 [`UnavailableModel`] 显式失败，避免空转或伪造完成。
//!
//! ---------------------------------------------------------------------------
//! 【本文件负责什么】
//! ---------------------------------------------------------------------------
//! 这个模块汇集 Runner 与模型交互的契约，并提供两种本地实现。具体说：
//!
//! 1. **抽象接口**（trait `ModelClient`）：由 `kiana-ports` 提供。任何想充当
//!    「产出模型响应的东西」都要实现这个 trait。
//!
//! 2. **实现位置**：本文件提供 `UnavailableModel`（显式失败）与 `ScriptedModel`
//!    （按顺序消费预设输出）；真实 provider 适配器从 daemon 侧注入，不在本文件定义。
//!
//! 3. **流式响应的缺省行为**：`ModelClient::complete_streaming` 的默认实现
//!    先调用 `complete()`，仅在 output.text 非空时回调一个 `ModelDelta::Text`，
//!    最后返回原输出。它让非流式实现兼容回调接口，不会模拟工具、usage 或 stop 分片。
//!
//! 4. **Token 计费关联**：`ModelUsage` 字段记录入/出 token 数，
//!    后续由 `kiana-runner::budget` 和 ControlPlane 的 `ModelBudgetPort` 核算。
//!
//! ---------------------------------------------------------------------------
//! 【在系统里的位置】
//! ---------------------------------------------------------------------------
//! ControlPlane 通过 RunnerPort 调用 `KianaHarness`，其 `model_step_once()` 经
//! `invoke_model()` 准备并执行模型 attempt。ModelClient 的 prepared/admitted 路径返回
//! ModelReply，流式增量和最终 ModelOutput 由 stream_normalizer 核对。
//! 工具调用随后转换为 CapabilityRequest，回到 ControlPlane，再由 Broker 执行。
//!
//! 注意：**这个模块不执行任何工具调用**。本地实现只返回预设输出或错误，
//! 不向网络发送请求；ModelClient 契约允许外部适配器提供模型响应。
//! 真正的工具执行发生在 capability-broker 侧，不能由模型客户端绕过授权。
//!
//! ---------------------------------------------------------------------------
//! 【何时失败与恢复】
//! ---------------------------------------------------------------------------
//! - **网络/API 错误**：是否重试取决于 typed recovery disposition、发送/副作用状态、
//!   delta、次数和 deadline；当前 retry 路径对格式/工具/上下文修复返回 unavailable，
//!   不自动进入 repair 或 clarification。
//! - **模型不可用**：用 `UnavailableModel` 显式返回错误，避免伪造响应。
//! - **流式流到一半取消**：`stream_normalizer` 会检测 `cancelled` 标记，
//!   拒绝后续 delta（late_delta_after_cancel_is_discarded）。
//! - **JSON 工具参数损坏**：`stream_normalizer` 在 finish() 时会验证
//!   （split_invalid_tool_json_has_zero_dispatches），不会部分应用。
//!
//! ---------------------------------------------------------------------------
//! 【Token 与预算】
//! ---------------------------------------------------------------------------
//! `ModelUsage { input_tokens, output_tokens }` 表示用量，不是货币费用。
//! harness 在调用前预留 token，收到回复后用 reply 中的 usage 结算；没有报告则
//! 按预留量保守计入本地账本。接入 ModelBudgetPort 时还需独立的准入与结算。

use async_trait::async_trait;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

pub use kiana_domain::{
    ModelDelta, ModelMessage, ModelOutput, ModelRequest, ModelRequestContext, ModelRole,
    ModelToolCall, ModelUsage,
};
pub use kiana_ports::ModelClient;

#[derive(Debug)]
/// 显式报告模型不可用的 fail-closed 客户端。
///
/// 这个实现永远返回错误，不会在没有模型的情况下伪造响应或空转。
pub struct UnavailableModel {
    /// 稳定错误文本，通常是 "model_unavailable" 或自定义诊断消息。
    error: String,
}

impl Default for UnavailableModel {
    fn default() -> Self {
        Self::new("model_unavailable")
    }
}

impl UnavailableModel {
    /// 创建带自定义错误文本的不可用模型。
    ///
    /// # 参数
    /// * `error` - 返回给 Runner 的错误消息
    ///
    /// # 例子
    /// ```ignore
    /// let unavailable = UnavailableModel::new("model_gated_by_policy");
    /// ```
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

#[async_trait]
impl ModelClient for UnavailableModel {
    /// 无论请求内容如何都返回保存的错误，不访问任何外部服务。
    ///
    /// 这种设计确保了 fail-closed：没有模型就不跑，不会伪造输出。
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err(self.error.clone())
    }
}

#[derive(Debug)]
/// 按预先给定顺序消费模型输出的本地脚本客户端。
///
/// 输出队列由 Mutex 保护，每次 `complete` 只弹出一个结果；队列耗尽或锁中毒都会显式
/// 报错。它适合 cassette/单测，不代表在线模型的延迟、限流或安全行为。
///
/// # 工作原理
/// 1. 创建时传入一个 `Vec<ModelOutput>` ← 预录的全部响应
/// 2. 每次调 `complete()` 就从队列前面弹一个
/// 3. 队列空时返回 `harness_script_exhausted` 错误
/// 4. Mutex 中毒时返回 `harness_script_lock_poisoned` 错误
///
/// # 使用场景
/// - 单元测试：录制一系列工具交互并验证 Runner 的逻辑
/// - 集成测试（cassette 模式）：完整业务流测试
/// - 快速验证：不依赖网络或 API key
pub struct ScriptedModel {
    /// 尚未消费的模型输出队列。受 Mutex 保护以支持并发访问（虽然通常单线程）。
    outputs: Mutex<VecDeque<ModelOutput>>,
}

impl ScriptedModel {
    /// 从输出向量创建脚本客户端，消费顺序与向量顺序一致。
    ///
    /// # 参数
    /// * `outputs` - 按调用顺序排列的模型输出列表
    ///
    /// # 例子
    /// ```ignore
    /// let responses = vec![
    ///     ModelOutput::text("Starting analysis"),
    ///     ModelOutput { text: "Found issue".to_owned(), tool_calls: vec![...], .. },
    /// ];
    /// let model = ScriptedModel::new(responses);
    /// ```
    pub fn new(outputs: Vec<ModelOutput>) -> Self {
        Self {
            outputs: Mutex::new(VecDeque::from(outputs)),
        }
    }

    /// 从 JSON 数组或 `{"outputs": [...]}` 解析脚本。
    ///
    /// 两种形式最终都反序列化为 `Vec<ModelOutput>`。缺少 outputs 时会直接尝试把整个
    /// value 解析为向量；单个输出对象不会自动包装，outputs 非数组也会失败。
    /// 结构错误返回 `harness_script_invalid:<error>`，不会构造一个部分成功的脚本。
    ///
    /// # 例子
    /// ```ignore
    /// let value = serde_json::json!({
    ///     "outputs": [
    ///         {"text": "hello", "tool_calls": []}
    ///     ]
    /// });
    /// let model = ScriptedModel::from_json(&value)?;
    /// ```
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let outputs = if let Some(array) = value.as_array() {
            serde_json::from_value(Value::Array(array.clone()))
        } else if let Some(outputs) = value.get("outputs") {
            serde_json::from_value(outputs.clone())
        } else {
            serde_json::from_value(value.clone())
        }
        .map_err(|error| format!("harness_script_invalid:{error}"))?;
        Ok(Self::new(outputs))
    }

    /// 从 JSON 文件读取并构造脚本模型。
    ///
    /// 两步过程：
    /// 1. 读文件 ← 失败时返回 `harness_script_unavailable`
    /// 2. 解析 JSON ← 失败时返回 `harness_script_invalid`
    ///
    /// 这样便于区分环境不可用（文件找不到、权限错）和脚本内容错误（损坏 JSON）。
    ///
    /// # 错误前缀
    /// - `harness_script_unavailable:<io_error>` ← 文件读取失败
    /// - `harness_script_invalid:<parse_error>` ← JSON 解析失败
    ///
    /// # 例子
    /// ```ignore
    /// let model = ScriptedModel::from_json_path("scripts/my_test.json")?;
    /// ```
    pub fn from_json_path(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path.as_ref())
            .map_err(|error| format!("harness_script_unavailable:{error}"))?;
        let value: Value = serde_json::from_str(&raw)
            .map_err(|error| format!("harness_script_invalid:{error}"))?;
        Self::from_json(&value)
    }
}

#[async_trait]
impl ModelClient for ScriptedModel {
    /// 弹出下一个脚本输出；没有剩余输出时返回 `harness_script_exhausted`。
    ///
    /// 队列顺序严格遵循创建时的顺序；如果实际调用次数超过录制数，就报 exhausted。
    ///
    /// # 错误
    /// - `harness_script_exhausted` ← 队列已空，但 Runner 还在继续请求
    /// - `harness_script_lock_poisoned` ← 极罕见的 Mutex 中毒（panic 恢复）
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        self.outputs
            .lock()
            .map_err(|_| "harness_script_lock_poisoned".to_owned())?
            .pop_front()
            .ok_or_else(|| "harness_script_exhausted".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn text_request() -> ModelRequest {
        ModelRequest {
            messages: vec![ModelMessage::user("hi")],
            tools: Vec::new(),
            sandbox: "read-only".to_owned(),
        }
    }

    #[tokio::test]
    async fn default_streaming_emits_complete_text_as_single_delta() {
        // 验证缺省流式实现：非流式响应 → 单个 Delta 事件
        let model: Arc<dyn ModelClient> =
            Arc::new(ScriptedModel::new(vec![ModelOutput::text("hello world")]));
        let mut deltas = Vec::new();

        let output = model
            .complete_streaming(text_request(), &mut |delta| {
                deltas.push(delta);
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(output, ModelOutput::text("hello world"));
        assert_eq!(
            deltas,
            vec![ModelDelta::Text {
                text: "hello world".to_owned()
            }]
        );
    }

    #[tokio::test]
    async fn complete_behavior_is_unchanged_by_streaming_default() {
        // 确保默认流式实现不改变非流式 complete() 的行为
        let model = ScriptedModel::new(vec![ModelOutput::text("unchanged")]);

        let output = model.complete(text_request()).await.unwrap();

        assert_eq!(output, ModelOutput::text("unchanged"));
    }

    #[tokio::test]
    async fn scripted_model_consumes_outputs_in_order() {
        // 脚本模型严格按输入顺序消费，没有乱序
        let model = ScriptedModel::from_json(&json!([
            {"text": "first", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "done"}
        ]))
        .unwrap();
        let first = model
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("hi")],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(first.tool_calls[0].name, "shell");
        let second = model
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("hi")],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(second.text, "done");
        assert!(second.tool_calls.is_empty());
    }

    #[test]
    fn legacy_cassette_output_without_metadata_still_deserializes() {
        // 向后兼容：旧格式 cassette 录音不含 usage/stop_reason/model_id 也能解析
        let output: ModelOutput =
            serde_json::from_value(json!({"text": "legacy", "tool_calls": []})).unwrap();
        assert_eq!(output, ModelOutput::text("legacy"));
        assert_eq!(output.usage, None);
        assert_eq!(output.stop_reason, None);
        assert_eq!(output.model_id, None);
    }

    #[test]
    fn model_output_round_trips_usage_metadata() {
        // 验证 usage/stop_reason/model_id 可正确序列化反序列化
        let output = ModelOutput {
            text: "done".to_owned(),
            tool_calls: Vec::new(),
            usage: Some(ModelUsage {
                input_tokens: 12,
                output_tokens: 4,
            }),
            stop_reason: Some("end_turn".to_owned()),
            model_id: Some("test-model".to_owned()),
            content: Vec::new(),
            continuation: None,
            structured: None,
        };

        let encoded = serde_json::to_value(&output).unwrap();
        assert_eq!(encoded["usage"]["input_tokens"], 12);
        assert_eq!(
            serde_json::from_value::<ModelOutput>(encoded).unwrap(),
            output
        );
    }
}
