//! Owned harness 持有的模型轮次接口与本地模型实现。
//!
//! Runner 循环只依赖 [`ModelClient`]，不直接依赖在线 provider crate。daemon 可以注入真实
//! 适配器（当前产品状态仍未证明 live provider），测试可以注入 [`ScriptedModel`]；没有
//! 可用模型时使用 [`UnavailableModel`] 显式失败，避免空转或伪造完成。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 模型消息的角色分类。
pub enum ModelRole {
    /// 系统提示或受控岗位说明。
    System,
    /// 用户输入、预算提醒或 steering 文本。
    User,
    /// 模型自然语言和工具调用声明。
    Assistant,
    /// Broker 返回的工具结果。
    Tool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 发给模型的单条消息。
pub struct ModelMessage {
    /// 消息角色。
    pub role: ModelRole,
    /// 文本内容；工具消息通常是结构化结果的 JSON 文本。
    pub text: String,
    /// 当角色为 Tool 时关联的工具调用 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 当角色为 Assistant 时声明的工具调用列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ModelToolCall>,
}

impl ModelMessage {
    /// 构造系统消息。
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::System,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// 构造用户消息。
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::User,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// 构造不含工具调用的 assistant 消息。
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// 构造携带工具调用声明的 assistant 消息。
    pub fn assistant_with_tools(text: impl Into<String>, tool_calls: Vec<ModelToolCall>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
            tool_call_id: None,
            tool_calls,
        }
    }

    /// 构造与某个工具调用 ID 关联的工具结果消息。
    pub fn tool(tool_call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Tool,
            text: text.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 模型请求的单个工具调用声明。
pub struct ModelToolCall {
    /// 模型生成的调用 ID；Runner 用它把结果对应回声明。
    pub id: String,
    /// 模型可见的工具名，后续由工具目录映射为能力请求。
    pub name: String,
    /// 未执行的原始 JSON 参数；不能直接当作授权。
    pub arguments: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 一次模型补全请求。
pub struct ModelRequest {
    /// 当前上下文消息。
    pub messages: Vec<ModelMessage>,
    /// 当前允许模型看到的工具 schema。
    pub tools: Vec<Value>,
    /// sandbox 展示值；真实边界由 Broker/执行器再次校验。
    pub sandbox: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
/// 模型返回的文本和工具调用声明。
pub struct ModelOutput {
    /// 自然语言文本。
    #[serde(default)]
    pub text: String,
    /// 需要外层转成能力请求的工具调用。
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
}

impl ModelOutput {
    /// 构造纯文本响应。
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tool_calls: Vec::new(),
        }
    }

    /// 构造带一个固定测试调用 ID 的工具响应。
    pub fn with_tool(text: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            text: text.into(),
            tool_calls: vec![ModelToolCall {
                id: "call-1".to_owned(),
                name: name.into(),
                arguments,
            }],
        }
    }
}

#[async_trait]
/// Runner 使用的异步模型客户端端口。
///
/// 实现只负责将消息交给模型并返回声明；它不得直接执行工具或修改工作区。
pub trait ModelClient: Send + Sync {
    /// 完成一次模型轮次。
    ///
    /// 返回错误时 Runner 应按 `model_unavailable`/脚本错误处理，不应伪造空的成功结果。
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String>;
}

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
}
