//! Model-turn interface owned by the Kiana harness.
//!
//! The loop never talks to a provider crate. Daemon injects a client, tests
//! inject a script, and missing models fail closed.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: ModelRole,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ModelToolCall>,
}

impl ModelMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::User,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn assistant_with_tools(text: impl Into<String>, tool_calls: Vec<ModelToolCall>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
            tool_call_id: None,
            tool_calls,
        }
    }

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
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<Value>,
    pub sandbox: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelOutput {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
}

impl ModelOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tool_calls: Vec::new(),
        }
    }

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
pub trait ModelClient: Send + Sync {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String>;
}

#[derive(Debug)]
pub struct UnavailableModel {
    error: String,
}

impl Default for UnavailableModel {
    fn default() -> Self {
        Self::new("model_unavailable")
    }
}

impl UnavailableModel {
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

#[async_trait]
impl ModelClient for UnavailableModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err(self.error.clone())
    }
}

#[derive(Debug)]
pub struct ScriptedModel {
    outputs: Mutex<VecDeque<ModelOutput>>,
}

impl ScriptedModel {
    pub fn new(outputs: Vec<ModelOutput>) -> Self {
        Self {
            outputs: Mutex::new(VecDeque::from(outputs)),
        }
    }

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
