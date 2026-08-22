//! Composition-root model injection for the owned harness.
//!
//! `kiana-runner` never depends on `kiana-services`. Scripted turns win via
//! `KIANA_HARNESS_SCRIPT`; otherwise daemon wraps a provider client; missing
//! credentials fail closed.

use async_trait::async_trait;
use kiana_domain::RoleSpec;
use kiana_runner::{
    ModelClient, ModelMessage, ModelOutput, ModelRequest, ModelRole, ModelToolCall, ScriptedModel,
    UnavailableModel,
};
use kiana_services::api::messages::{Message, MessagesRequest};
use kiana_services::api::provider::{
    provider_registry_entry, AnthropicProvider, FakeProvider, OllamaProvider,
    OpenAiCompatibleProvider, Provider, ProviderError, ANTHROPIC_PROVIDER_ID, FAKE_PROVIDER_ID,
    OLLAMA_PROVIDER_ID, OPENAI_COMPATIBLE_PROVIDER_ID,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const ENV_HARNESS_SCRIPT: &str = "KIANA_HARNESS_SCRIPT";
const ENV_PROVIDER: &str = "KIANA_PROVIDER";
const DEFAULT_MAX_TOKENS: u32 = 4096;
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) fn from_env() -> Arc<dyn ModelClient> {
    match std::env::var(ENV_HARNESS_SCRIPT) {
        Ok(path) if !path.trim().is_empty() => match ScriptedModel::from_json_path(path.trim()) {
            Ok(model) => Arc::new(model),
            Err(error) => Arc::new(UnavailableModel::new(error)),
        },
        _ => match provider_from_env() {
            Ok(Some(model)) => model,
            Ok(None) => Arc::new(UnavailableModel::default()),
            Err(error) => Arc::new(UnavailableModel::new(error)),
        },
    }
}

fn provider_from_env() -> Result<Option<Arc<dyn ModelClient>>, String> {
    let provider_id = std::env::var(ENV_PROVIDER)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ANTHROPIC_PROVIDER_ID.to_owned());
    let Some(entry) = provider_registry_entry(&provider_id) else {
        return Err(format!("unknown_provider:{provider_id}"));
    };
    let model_id = first_env(&entry.model_env_vars).unwrap_or(entry.default_model_id);
    let provider: Box<dyn Provider> = match provider_id.as_str() {
        FAKE_PROVIDER_ID => Box::new(
            FakeProvider::from_script_value(model_id.clone(), None)
                .map_err(|error| error.to_string())?,
        ),
        id if id == ANTHROPIC_PROVIDER_ID => {
            let Some(api_key) = first_env(&entry.api_key_env_vars) else {
                return Ok(None);
            };
            let base_url = first_env(&entry.base_url_env_vars)
                .or(entry.default_base_url)
                .unwrap_or_default();
            Box::new(AnthropicProvider::new(api_key, base_url, PROVIDER_TIMEOUT))
        }
        id if id == OPENAI_COMPATIBLE_PROVIDER_ID => {
            let Some(api_key) = first_env(&entry.api_key_env_vars) else {
                return Ok(None);
            };
            let base_url = first_env(&entry.base_url_env_vars)
                .or(entry.default_base_url)
                .unwrap_or_default();
            Box::new(
                OpenAiCompatibleProvider::new(api_key, base_url, PROVIDER_TIMEOUT)
                    .map_err(|error| error.to_string())?,
            )
        }
        id if id == OLLAMA_PROVIDER_ID => {
            let base_url = first_env(&entry.base_url_env_vars)
                .or(entry.default_base_url)
                .unwrap_or_default();
            Box::new(
                OllamaProvider::new(base_url, PROVIDER_TIMEOUT)
                    .map_err(|error| error.to_string())?,
            )
        }
        other => return Err(format!("unknown_provider:{other}")),
    };
    Ok(Some(Arc::new(ProviderModelClient {
        provider,
        model: model_id,
    })))
}

fn provider_system_prompt() -> String {
    let mut prompt = std::env::var("KIANA_SYSTEM_PROMPT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| RoleSpec::builder().prompt);
    if let Ok(extra) = std::env::var("KIANA_APPEND_SYSTEM_PROMPT") {
        let extra = extra.trim();
        if !extra.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(extra);
        }
    }
    prompt
}

fn first_env(keys: &[String]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

struct ProviderModelClient {
    provider: Box<dyn Provider>,
    model: String,
}

#[async_trait]
impl ModelClient for ProviderModelClient {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        let tools = map_tools(&request.tools);
        let response = self
            .provider
            .create_message(MessagesRequest {
                model: self.model.clone(),
                messages: map_messages(&request.messages)?,
                max_tokens: DEFAULT_MAX_TOKENS,
                system: Some(json!(provider_system_prompt())),
                temperature: None,
                tools: if tools.is_empty() { None } else { Some(tools) },
                thinking: None,
                stream: Some(false),
            })
            .await
            .map_err(map_provider_complete_error)?;
        Ok(output_from_content(&response.content))
    }
}

fn map_provider_complete_error(error: ProviderError) -> String {
    if matches!(error, ProviderError::UnsupportedCapability { .. }) {
        error.code().to_owned()
    } else {
        error.to_string()
    }
}

fn map_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?;
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let parameters = tool
                .get("input_schema")
                .or_else(|| tool.get("parameters"))
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
            Some(json!({
                "name": name,
                "description": description,
                "input_schema": parameters,
            }))
        })
        .collect()
}

fn map_messages(messages: &[ModelMessage]) -> Result<Vec<Message>, String> {
    let mut mapped = Vec::new();
    for message in messages {
        match message.role {
            ModelRole::System => mapped.push(Message {
                role: "user".to_owned(),
                content: json!(format!("[system]\n{}", message.text)),
            }),
            ModelRole::User => mapped.push(Message {
                role: "user".to_owned(),
                content: json!(message.text),
            }),
            ModelRole::Assistant => {
                let mut content = Vec::new();
                if !message.text.is_empty() {
                    content.push(json!({ "type": "text", "text": message.text }));
                }
                for call in &message.tool_calls {
                    content.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": call.arguments,
                    }));
                }
                if content.is_empty() {
                    content.push(json!({ "type": "text", "text": "" }));
                }
                mapped.push(Message {
                    role: "assistant".to_owned(),
                    content: Value::Array(content),
                });
            }
            ModelRole::Tool => {
                let tool_use_id = message
                    .tool_call_id
                    .clone()
                    .ok_or_else(|| "tool_result_missing_call_id".to_owned())?;
                mapped.push(Message {
                    role: "user".to_owned(),
                    content: json!([{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": message.text,
                    }]),
                });
            }
        }
    }
    Ok(mapped)
}

fn output_from_content(content: &[Value]) -> ModelOutput {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(chunk) = block.get("text").and_then(Value::as_str) {
                    text.push_str(chunk);
                }
            }
            Some("tool_use") => {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_owned();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("shell")
                    .to_owned();
                let arguments = block.get("input").cloned().unwrap_or(Value::Null);
                tool_calls.push(ModelToolCall {
                    id,
                    name,
                    arguments,
                });
            }
            _ => {}
        }
    }
    ModelOutput { text, tool_calls }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_services::api::provider::{FakeProviderStep, FAKE_TEXT_ONLY_MODEL_ID};

    #[tokio::test]
    async fn provider_wrapper_maps_tool_calls_and_final_text() {
        let provider = FakeProvider::new(
            "fake-model".to_owned(),
            vec![
                FakeProviderStep::ToolCall {
                    id: Some("c1".to_owned()),
                    name: "shell".to_owned(),
                    input: json!({ "command": "ls" }),
                    text: Some("running ls".to_owned()),
                },
                FakeProviderStep::FinalAnswer {
                    text: "architecture mapped".to_owned(),
                },
            ],
        );
        let client = ProviderModelClient {
            provider: Box::new(provider),
            model: "fake-model".to_owned(),
        };
        let first = client
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("map it")],
                tools: vec![json!({"name": "shell", "parameters": {"type": "object"}})],
                sandbox: "read-only".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(first.text, "running ls");
        assert_eq!(first.tool_calls[0].id, "c1");
        assert_eq!(first.tool_calls[0].name, "shell");

        let second = client
            .complete(ModelRequest {
                messages: vec![
                    ModelMessage::user("map it"),
                    ModelMessage::assistant_with_tools("running ls", first.tool_calls.clone()),
                    ModelMessage::tool("c1", r#"{"stdout":"listed"}"#),
                ],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(second.text, "architecture mapped");
        assert!(second.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn text_only_provider_maps_unsupported_tools_code() {
        let provider = FakeProvider::new(
            FAKE_TEXT_ONLY_MODEL_ID.to_owned(),
            vec![FakeProviderStep::AssistantText {
                text: "should not be consumed".to_owned(),
            }],
        );
        let client = ProviderModelClient {
            provider: Box::new(provider),
            model: FAKE_TEXT_ONLY_MODEL_ID.to_owned(),
        };
        let error = client
            .complete(ModelRequest {
                messages: vec![ModelMessage::user(
                    "create a file named GOLDEN_PATH.txt containing hello",
                )],
                tools: vec![json!({
                    "name": "apply_patch",
                    "parameters": {"type": "object"}
                })],
                sandbox: "workspace-write".to_owned(),
            })
            .await
            .unwrap_err();
        assert_eq!(error, "unsupported_tools");
    }
}
