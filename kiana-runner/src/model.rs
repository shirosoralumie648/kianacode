//! Owned harness 持有的模型轮次接口与本地模型实现。
//!
//! Runner 循环只依赖 [`ModelClient`]，不直接依赖在线 provider crate。daemon 可以注入真实
//! 适配器（当前产品状态仍未证明 live provider），测试可以注入 [`ScriptedModel`]；没有
//! 可用模型时使用 [`UnavailableModel`] 显式失败，避免空转或伪造完成。

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
pub struct UnavailableModel {
    /// 稳定错误文本。
    error: String,
}

impl Default for UnavailableModel {
    fn default() -> Self {
        Self::new("model_unavailable")
    }
}

impl UnavailableModel {
    /// 创建带自定义错误文本的不可用模型。
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

#[async_trait]
impl ModelClient for UnavailableModel {
    /// 无论请求内容如何都返回保存的错误，不访问任何外部服务。
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err(self.error.clone())
    }
}

#[derive(Debug)]
/// 按预先给定顺序消费模型输出的本地脚本客户端。
///
/// 输出队列由 Mutex 保护，每次 `complete` 只弹出一个结果；队列耗尽或锁中毒都会显式
/// 报错。它适合 cassette/单测，不代表在线模型的延迟、限流或安全行为。
pub struct ScriptedModel {
    /// 尚未消费的模型输出队列。
    outputs: Mutex<VecDeque<ModelOutput>>,
}

impl ScriptedModel {
    /// 从输出向量创建脚本客户端，消费顺序与向量顺序一致。
    pub fn new(outputs: Vec<ModelOutput>) -> Self {
        Self {
            outputs: Mutex::new(VecDeque::from(outputs)),
        }
    }

    /// 从 JSON 数组、`{"outputs": [...]}` 或单个输出结构解析脚本。
    ///
    /// 结构错误返回稳定 `harness_script_invalid`，不会部分接受后继续运行。
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
    /// 文件读取失败与 JSON 解析失败使用不同前缀，便于区分环境不可用和脚本内容错误。
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
        let model = ScriptedModel::new(vec![ModelOutput::text("unchanged")]);

        let output = model.complete(text_request()).await.unwrap();

        assert_eq!(output, ModelOutput::text("unchanged"));
    }

    #[tokio::test]
    async fn scripted_model_consumes_outputs_in_order() {
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
        let output: ModelOutput =
            serde_json::from_value(json!({"text": "legacy", "tool_calls": []})).unwrap();
        assert_eq!(output, ModelOutput::text("legacy"));
        assert_eq!(output.usage, None);
        assert_eq!(output.stop_reason, None);
        assert_eq!(output.model_id, None);
    }

    #[test]
    fn model_output_round_trips_usage_metadata() {
        let output = ModelOutput {
            text: "done".to_owned(),
            tool_calls: Vec::new(),
            usage: Some(ModelUsage {
                input_tokens: 12,
                output_tokens: 4,
            }),
            stop_reason: Some("end_turn".to_owned()),
            model_id: Some("test-model".to_owned()),
        };

        let encoded = serde_json::to_value(&output).unwrap();
        assert_eq!(encoded["usage"]["input_tokens"], 12);
        assert_eq!(
            serde_json::from_value::<ModelOutput>(encoded).unwrap(),
            output
        );
    }
}
