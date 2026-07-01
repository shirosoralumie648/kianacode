use super::client::AnthropicClient;
use super::errors::{ApiError, ApiErrorKind};
use super::messages::{Message, MessagesRequest, MessagesResponse, Usage};
use super::streaming::{
    ContentBlock as StreamContentBlock, Delta, DeltaUsage, MessageDelta, MessageStart, StreamEvent,
};
use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

pub const ANTHROPIC_PROVIDER_ID: &str = "anthropic";
pub const FAKE_PROVIDER_ID: &str = "fake";
pub const FAKE_MODEL_ID: &str = "fake-model";
pub const FAKE_TEXT_ONLY_MODEL_ID: &str = "fake-text-only";
pub const OPENAI_COMPATIBLE_PROVIDER_ID: &str = "openai-compatible";
pub const OPENAI_COMPATIBLE_DEFAULT_MODEL_ID: &str = "gpt-4.1";
pub const OLLAMA_PROVIDER_ID: &str = "ollama";
pub const OLLAMA_DEFAULT_MODEL_ID: &str = "llama3.1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelProfile {
    pub provider_id: String,
    pub model_id: String,
    pub supports_tools: bool,
    pub supports_streaming: bool,
    pub supports_vision: bool,
    pub supports_structured_output: bool,
    pub context_window: u32,
}

impl ModelProfile {
    pub fn supports_request(&self, request: &MessagesRequest) -> ProviderResult<()> {
        if request
            .tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty())
            && !self.supports_tools
        {
            return Err(ProviderError::UnsupportedCapability {
                provider_id: self.provider_id.clone(),
                model_id: self.model_id.clone(),
                capability: "tools".to_string(),
                message: format!(
                    "model {}/{} does not support tools",
                    self.provider_id, self.model_id
                ),
            });
        }
        if request.stream == Some(true) && !self.supports_streaming {
            return Err(ProviderError::UnsupportedCapability {
                provider_id: self.provider_id.clone(),
                model_id: self.model_id.clone(),
                capability: "streaming".to_string(),
                message: format!(
                    "model {}/{} does not support streaming",
                    self.provider_id, self.model_id
                ),
            });
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("unsupported provider capability: {message}")]
    UnsupportedCapability {
        provider_id: String,
        model_id: String,
        capability: String,
        message: String,
    },

    #[error("provider authentication error for {provider_id}: {message}")]
    Auth {
        provider_id: String,
        message: String,
    },

    #[error("provider error from {provider_id}: {code}: {message}")]
    Provider {
        provider_id: String,
        code: String,
        message: String,
    },

    #[error(transparent)]
    Api(#[from] ApiError),
}

impl ProviderError {
    pub fn code(&self) -> &str {
        match self {
            ProviderError::UnsupportedCapability { capability, .. } => match capability.as_str() {
                "tools" => "unsupported_tools",
                "streaming" => "unsupported_streaming",
                _ => "unsupported_capability",
            },
            ProviderError::Auth { .. } => "auth_error",
            ProviderError::Provider { code, .. } => code,
            ProviderError::Api(error) => match &error.kind {
                ApiErrorKind::ServerOverload => "server_overload",
                ApiErrorKind::RateLimit => "rate_limit",
                ApiErrorKind::ApiTimeout => "api_timeout",
                ApiErrorKind::InvalidModel => "invalid_model",
                _ => "api_error",
            },
        }
    }
}

pub type ProviderResult<T> = Result<T, ProviderError>;
pub type ProviderStream = Pin<Box<dyn Stream<Item = ProviderResult<StreamEvent>> + Send + 'static>>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn model_profile(&self, model_id: &str) -> ModelProfile;

    fn ensure_request_supported(&self, request: &MessagesRequest) -> ProviderResult<()> {
        self.model_profile(&request.model).supports_request(request)
    }

    async fn create_message(&self, request: MessagesRequest) -> ProviderResult<MessagesResponse>;

    async fn stream_message(&self, request: MessagesRequest) -> ProviderResult<ProviderStream>;
}

pub fn built_in_model_profiles() -> Vec<ModelProfile> {
    vec![
        anthropic_model_profile("claude-sonnet-4-6"),
        anthropic_model_profile("claude-opus-4-1"),
        anthropic_model_profile("claude-haiku-4-5"),
        openai_compatible_model_profile(OPENAI_COMPATIBLE_DEFAULT_MODEL_ID),
        ollama_model_profile(OLLAMA_DEFAULT_MODEL_ID),
        fake_model_profile(FAKE_MODEL_ID),
        ModelProfile {
            provider_id: FAKE_PROVIDER_ID.to_string(),
            model_id: FAKE_TEXT_ONLY_MODEL_ID.to_string(),
            supports_tools: false,
            supports_streaming: true,
            supports_vision: false,
            supports_structured_output: false,
            context_window: 8_192,
        },
    ]
}

pub fn model_profile(provider_id: &str, model_id: &str) -> Option<ModelProfile> {
    match provider_id {
        ANTHROPIC_PROVIDER_ID => Some(anthropic_model_profile(model_id)),
        OPENAI_COMPATIBLE_PROVIDER_ID => Some(openai_compatible_model_profile(model_id)),
        OLLAMA_PROVIDER_ID => Some(ollama_model_profile(model_id)),
        FAKE_PROVIDER_ID if model_id == FAKE_TEXT_ONLY_MODEL_ID => Some(ModelProfile {
            provider_id: FAKE_PROVIDER_ID.to_string(),
            model_id: model_id.to_string(),
            supports_tools: false,
            supports_streaming: true,
            supports_vision: false,
            supports_structured_output: false,
            context_window: 8_192,
        }),
        FAKE_PROVIDER_ID => Some(fake_model_profile(model_id)),
        _ => None,
    }
}

fn anthropic_model_profile(model_id: &str) -> ModelProfile {
    ModelProfile {
        provider_id: ANTHROPIC_PROVIDER_ID.to_string(),
        model_id: model_id.to_string(),
        supports_tools: true,
        supports_streaming: true,
        supports_vision: true,
        supports_structured_output: false,
        context_window: 200_000,
    }
}

fn fake_model_profile(model_id: &str) -> ModelProfile {
    ModelProfile {
        provider_id: FAKE_PROVIDER_ID.to_string(),
        model_id: model_id.to_string(),
        supports_tools: true,
        supports_streaming: true,
        supports_vision: false,
        supports_structured_output: true,
        context_window: 8_192,
    }
}

fn openai_compatible_model_profile(model_id: &str) -> ModelProfile {
    ModelProfile {
        provider_id: OPENAI_COMPATIBLE_PROVIDER_ID.to_string(),
        model_id: model_id.to_string(),
        supports_tools: true,
        supports_streaming: true,
        supports_vision: false,
        supports_structured_output: false,
        context_window: 128_000,
    }
}

fn ollama_model_profile(model_id: &str) -> ModelProfile {
    ModelProfile {
        provider_id: OLLAMA_PROVIDER_ID.to_string(),
        model_id: model_id.to_string(),
        supports_tools: true,
        supports_streaming: true,
        supports_vision: false,
        supports_structured_output: false,
        context_window: 128_000,
    }
}

pub struct AnthropicProvider {
    client: AnthropicClient,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: String, timeout: Duration) -> Self {
        Self {
            client: AnthropicClient::with_base_url_and_timeout(api_key, base_url, timeout),
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn provider_id(&self) -> &str {
        ANTHROPIC_PROVIDER_ID
    }

    fn model_profile(&self, model_id: &str) -> ModelProfile {
        anthropic_model_profile(model_id)
    }

    async fn create_message(&self, request: MessagesRequest) -> ProviderResult<MessagesResponse> {
        self.ensure_request_supported(&request)?;
        self.client
            .create_message(request)
            .await
            .map_err(provider_error_from_anyhow)
    }

    async fn stream_message(&self, request: MessagesRequest) -> ProviderResult<ProviderStream> {
        self.ensure_request_supported(&request)?;
        let stream = self
            .client
            .stream_message(request)
            .await
            .map_err(provider_error_from_anyhow)?
            .map(|result| result.map_err(provider_error_from_anyhow));
        Ok(Box::pin(stream))
    }
}

fn provider_error_from_anyhow(error: anyhow::Error) -> ProviderError {
    match error.downcast::<ApiError>() {
        Ok(error) => ProviderError::Api(error),
        Err(error) => ProviderError::Provider {
            provider_id: ANTHROPIC_PROVIDER_ID.to_string(),
            code: "provider_error".to_string(),
            message: error.to_string(),
        },
    }
}

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(api_key: String, base_url: String, timeout: Duration) -> ProviderResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| ProviderError::Provider {
                provider_id: OPENAI_COMPATIBLE_PROVIDER_ID.to_string(),
                code: "client_build_failed".to_string(),
                message: error.to_string(),
            })?;
        Ok(Self {
            client,
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    fn completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn provider_id(&self) -> &str {
        OPENAI_COMPATIBLE_PROVIDER_ID
    }

    fn model_profile(&self, model_id: &str) -> ModelProfile {
        openai_compatible_model_profile(model_id)
    }

    async fn create_message(&self, request: MessagesRequest) -> ProviderResult<MessagesResponse> {
        self.ensure_request_supported(&request)?;
        let body = openai_chat_request_body(&request, false);
        let response = self
            .client
            .post(self.completions_url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Provider {
                provider_id: OPENAI_COMPATIBLE_PROVIDER_ID.to_string(),
                code: "request_failed".to_string(),
                message: error.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(openai_error_from_response(response).await);
        }

        let value = response
            .json::<Value>()
            .await
            .map_err(|error| ProviderError::Provider {
                provider_id: OPENAI_COMPATIBLE_PROVIDER_ID.to_string(),
                code: "invalid_response".to_string(),
                message: error.to_string(),
            })?;
        openai_chat_response_to_messages_response(value, &request.model)
    }

    async fn stream_message(&self, request: MessagesRequest) -> ProviderResult<ProviderStream> {
        let response = self.create_message(request).await?;
        let events = stream_events_from_response(&response);
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

fn openai_chat_request_body(request: &MessagesRequest, stream: bool) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = request.system.as_ref() {
        messages.push(json!({
            "role": "system",
            "content": openai_message_content(system),
        }));
    }
    for message in &request.messages {
        messages.extend(openai_messages_from_message(message));
    }

    let mut body = json!({
        "model": request.model.clone(),
        "messages": messages,
        "max_tokens": request.max_tokens,
        "stream": stream,
    });
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(tools) = request.tools.as_ref().filter(|tools| !tools.is_empty()) {
        body["tools"] = Value::Array(tools.iter().map(openai_tool_definition).collect());
        body["tool_choice"] = json!("auto");
    }
    body
}

fn openai_messages_from_message(message: &Message) -> Vec<Value> {
    if let Some(array) = message.content.as_array() {
        if array
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        {
            return array
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
                .map(|block| {
                    json!({
                        "role": "tool",
                        "tool_call_id": block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_unknown"),
                        "content": openai_tool_result_content(block.get("content").unwrap_or(&Value::Null)),
                    })
                })
                .collect();
        }

        let tool_use_blocks = array
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
            .collect::<Vec<_>>();
        if !tool_use_blocks.is_empty() {
            let text = openai_message_text_content(&message.content);
            return vec![json!({
                "role": message.role.clone(),
                "content": if text.is_empty() { Value::Null } else { Value::String(text) },
                "tool_calls": tool_use_blocks
                    .into_iter()
                    .map(openai_tool_call_from_tool_use)
                    .collect::<Vec<_>>(),
            })];
        }
    }

    vec![json!({
        "role": message.role.clone(),
        "content": openai_message_content(&message.content),
    })]
}

fn openai_message_content(content: &Value) -> Value {
    let text = openai_message_text_content(content);
    if !text.is_empty() {
        return Value::String(text);
    }
    Value::String(content.to_string())
}

fn openai_message_text_content(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    if let Some(array) = content.as_array() {
        let text = array
            .iter()
            .filter_map(|part| {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    Some(text.to_string())
                } else if let Some(text) = part.as_str() {
                    Some(text.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        return text;
    }
    String::new()
}

fn openai_tool_definition(tool: &Value) -> Value {
    if tool.get("type").and_then(Value::as_str) == Some("function") {
        return tool.clone();
    }
    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let parameters = tool
        .get("input_schema")
        .or_else(|| tool.get("inputSchema"))
        .or_else(|| tool.get("parameters"))
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    })
}

fn openai_tool_call_from_tool_use(block: &Value) -> Value {
    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
    json!({
        "id": block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("toolu_unknown"),
        "type": "function",
        "function": {
            "name": block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Unknown"),
            "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string()),
        }
    })
}

fn openai_tool_result_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| part.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        value => value.to_string(),
    }
}

async fn openai_error_from_response(response: reqwest::Response) -> ProviderError {
    let status = response.status();
    let value = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("request failed"))
        .to_string();
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        ProviderError::Auth {
            provider_id: OPENAI_COMPATIBLE_PROVIDER_ID.to_string(),
            message,
        }
    } else {
        let code = value
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("openai_compatible_error")
            .to_string();
        ProviderError::Provider {
            provider_id: OPENAI_COMPATIBLE_PROVIDER_ID.to_string(),
            code,
            message,
        }
    }
}

fn openai_chat_response_to_messages_response(
    value: Value,
    fallback_model: &str,
) -> ProviderResult<MessagesResponse> {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| ProviderError::Provider {
            provider_id: OPENAI_COMPATIBLE_PROVIDER_ID.to_string(),
            code: "invalid_response".to_string(),
            message: "OpenAI-compatible response missing choices[0]".to_string(),
        })?;
    let message = choice.get("message").unwrap_or(&Value::Null);
    let text = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let usage = value.get("usage").unwrap_or(&Value::Null);
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(json!({
            "type": "text",
            "text": text,
        }));
    }
    content.extend(openai_tool_use_blocks_from_message(message));
    if content.is_empty() {
        content.push(json!({
            "type": "text",
            "text": "",
        }));
    }
    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(MessagesResponse {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("openai-compatible-message")
            .to_string(),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(fallback_model)
            .to_string(),
        role: message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("assistant")
            .to_string(),
        content,
        stop_reason: finish_reason.map(|reason| {
            if reason == "tool_calls" {
                "tool_use".to_string()
            } else {
                reason
            }
        }),
        usage: Usage {
            input_tokens: usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32,
            output_tokens: usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32,
        },
    })
}

fn openai_tool_use_blocks_from_message(message: &Value) -> Vec<Value> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|tool_calls| {
            tool_calls
                .iter()
                .filter_map(|tool_call| {
                    let function = tool_call.get("function").unwrap_or(&Value::Null);
                    let name = function.get("name").and_then(Value::as_str)?;
                    let arguments = function
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let input =
                        serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({}));
                    Some(json!({
                        "type": "tool_use",
                        "id": tool_call
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_openai"),
                        "name": name,
                        "input": input,
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
}

impl OllamaProvider {
    pub fn new(base_url: String, timeout: Duration) -> ProviderResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| ProviderError::Provider {
                provider_id: OLLAMA_PROVIDER_ID.to_string(),
                code: "client_build_failed".to_string(),
                message: error.to_string(),
            })?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url)
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn provider_id(&self) -> &str {
        OLLAMA_PROVIDER_ID
    }

    fn model_profile(&self, model_id: &str) -> ModelProfile {
        ollama_model_profile(model_id)
    }

    async fn create_message(&self, request: MessagesRequest) -> ProviderResult<MessagesResponse> {
        self.ensure_request_supported(&request)?;
        let body = ollama_chat_request_body(&request, false);
        let response = self
            .client
            .post(self.chat_url())
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Provider {
                provider_id: OLLAMA_PROVIDER_ID.to_string(),
                code: "request_failed".to_string(),
                message: error.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(ollama_error_from_response(response).await);
        }

        let value = response
            .json::<Value>()
            .await
            .map_err(|error| ProviderError::Provider {
                provider_id: OLLAMA_PROVIDER_ID.to_string(),
                code: "invalid_response".to_string(),
                message: error.to_string(),
            })?;
        ollama_chat_response_to_messages_response(value, &request.model)
    }

    async fn stream_message(&self, request: MessagesRequest) -> ProviderResult<ProviderStream> {
        let response = self.create_message(request).await?;
        let events = stream_events_from_response(&response);
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

fn ollama_chat_request_body(request: &MessagesRequest, stream: bool) -> Value {
    let mut messages = Vec::new();
    let mut tool_names_by_id = HashMap::new();
    if let Some(system) = request.system.as_ref() {
        messages.push(json!({
            "role": "system",
            "content": ollama_message_content(system),
        }));
    }
    for message in &request.messages {
        messages.extend(ollama_messages_from_message(message, &mut tool_names_by_id));
    }

    let mut body = json!({
        "model": request.model.clone(),
        "messages": messages,
        "stream": stream,
        "options": {
            "num_predict": request.max_tokens,
        },
    });
    if let Some(temperature) = request.temperature {
        body["options"]["temperature"] = json!(temperature);
    }
    if let Some(tools) = request.tools.as_ref().filter(|tools| !tools.is_empty()) {
        body["tools"] = Value::Array(tools.iter().map(openai_tool_definition).collect());
    }
    body
}

fn ollama_messages_from_message(
    message: &Message,
    tool_names_by_id: &mut HashMap<String, String>,
) -> Vec<Value> {
    if let Some(array) = message.content.as_array() {
        if array
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        {
            return array
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
                .map(|block| {
                    json!({
                        "role": "tool",
                        "tool_name": ollama_tool_name_for_result(block, tool_names_by_id),
                        "content": openai_tool_result_content(block.get("content").unwrap_or(&Value::Null)),
                    })
                })
                .collect();
        }

        let tool_use_blocks = array
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
            .collect::<Vec<_>>();
        if !tool_use_blocks.is_empty() {
            for block in &tool_use_blocks {
                if let (Some(id), Some(name)) = (
                    block.get("id").and_then(Value::as_str),
                    block.get("name").and_then(Value::as_str),
                ) {
                    tool_names_by_id.insert(id.to_string(), name.to_string());
                }
            }
            return vec![json!({
                "role": message.role.clone(),
                "content": ollama_message_content(&message.content),
                "tool_calls": tool_use_blocks
                    .into_iter()
                    .map(ollama_tool_call_from_tool_use)
                    .collect::<Vec<_>>(),
            })];
        }
    }

    vec![json!({
        "role": message.role.clone(),
        "content": ollama_message_content(&message.content),
    })]
}

fn ollama_tool_name_for_result(
    block: &Value,
    tool_names_by_id: &HashMap<String, String>,
) -> String {
    block
        .get("tool_name")
        .or_else(|| block.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .and_then(|id| tool_names_by_id.get(id).cloned())
        })
        .unwrap_or_else(|| "tool".to_string())
}

fn ollama_tool_call_from_tool_use(block: &Value) -> Value {
    json!({
        "function": {
            "name": block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Unknown"),
            "arguments": block.get("input").cloned().unwrap_or_else(|| json!({})),
        }
    })
}

fn ollama_message_content(content: &Value) -> String {
    match openai_message_content(content) {
        Value::String(text) => text,
        value => value.to_string(),
    }
}

async fn ollama_error_from_response(response: reqwest::Response) -> ProviderError {
    let status = response.status();
    let value = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    let message = value
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
        })
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("request failed"))
        .to_string();
    ProviderError::Provider {
        provider_id: OLLAMA_PROVIDER_ID.to_string(),
        code: "ollama_error".to_string(),
        message,
    }
}

fn ollama_chat_response_to_messages_response(
    value: Value,
    fallback_model: &str,
) -> ProviderResult<MessagesResponse> {
    let message = value.get("message").unwrap_or(&Value::Null);
    let has_tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|tool_calls| !tool_calls.is_empty());
    let text = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(json!({
            "type": "text",
            "text": text,
        }));
    }
    content.extend(ollama_tool_use_blocks_from_message(message));
    if content.is_empty() {
        content.push(json!({
            "type": "text",
            "text": "",
        }));
    }
    Ok(MessagesResponse {
        id: value
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("ollama-message")
            .to_string(),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(fallback_model)
            .to_string(),
        role: message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("assistant")
            .to_string(),
        content,
        stop_reason: if has_tool_calls {
            Some("tool_use".to_string())
        } else {
            value
                .get("done_reason")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    value
                        .get("done")
                        .and_then(Value::as_bool)
                        .filter(|done| *done)
                        .map(|_| "stop".to_string())
                })
        },
        usage: Usage {
            input_tokens: value
                .get("prompt_eval_count")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32,
            output_tokens: value
                .get("eval_count")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32,
        },
    })
}

fn ollama_tool_use_blocks_from_message(message: &Value) -> Vec<Value> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|tool_calls| {
            tool_calls
                .iter()
                .enumerate()
                .filter_map(|(index, tool_call)| {
                    let function = tool_call.get("function").unwrap_or(&Value::Null);
                    let name = function.get("name").and_then(Value::as_str)?;
                    let input = function
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    Some(json!({
                        "type": "tool_use",
                        "id": tool_call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("toolu_ollama_{index}")),
                        "name": name,
                        "input": input,
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FakeProviderStep {
    AssistantText {
        text: String,
    },
    ToolCall {
        id: Option<String>,
        name: String,
        #[serde(default)]
        input: Value,
        #[serde(default)]
        text: Option<String>,
    },
    FinalAnswer {
        text: String,
    },
    ProviderError {
        code: Option<String>,
        message: String,
    },
}

#[derive(Clone)]
pub struct FakeProvider {
    profile: ModelProfile,
    steps: Arc<Mutex<VecDeque<FakeProviderStep>>>,
    calls: Arc<AtomicUsize>,
}

impl FakeProvider {
    pub fn new(model_id: String, steps: Vec<FakeProviderStep>) -> Self {
        Self {
            profile: model_profile(FAKE_PROVIDER_ID, &model_id)
                .unwrap_or_else(|| fake_model_profile(&model_id)),
            steps: Arc::new(Mutex::new(VecDeque::from(steps))),
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn from_script_value(model_id: String, script: Option<&Value>) -> ProviderResult<Self> {
        let steps = match script {
            Some(value) => parse_fake_script_value(value)?,
            None => std::env::var("KIANA_FAKE_PROVIDER_SCRIPT")
                .ok()
                .map(|raw| parse_fake_script_text(&raw))
                .transpose()?
                .unwrap_or_else(|| {
                    vec![FakeProviderStep::AssistantText {
                        text: "Fake provider response.".to_string(),
                    }]
                }),
        };
        Ok(Self::new(model_id, steps))
    }

    fn next_step(&self) -> ProviderResult<(usize, FakeProviderStep)> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let step = self
            .steps
            .lock()
            .map_err(|_| ProviderError::Provider {
                provider_id: FAKE_PROVIDER_ID.to_string(),
                code: "fake_provider_lock".to_string(),
                message: "fake provider script lock was poisoned".to_string(),
            })?
            .pop_front()
            .ok_or_else(|| ProviderError::Provider {
                provider_id: FAKE_PROVIDER_ID.to_string(),
                code: "fake_provider_script_exhausted".to_string(),
                message: "fake provider script has no remaining steps".to_string(),
            })?;
        Ok((call_index, step))
    }

    fn response_from_step(
        &self,
        model: &str,
        call_index: usize,
        step: FakeProviderStep,
    ) -> ProviderResult<MessagesResponse> {
        let mut content = Vec::new();
        let stop_reason = match step {
            FakeProviderStep::AssistantText { text } | FakeProviderStep::FinalAnswer { text } => {
                content.push(json!({
                    "type": "text",
                    "text": text,
                }));
                Some("end_turn".to_string())
            }
            FakeProviderStep::ToolCall {
                id,
                name,
                input,
                text,
            } => {
                if let Some(text) = text.filter(|text| !text.is_empty()) {
                    content.push(json!({
                        "type": "text",
                        "text": text,
                    }));
                }
                content.push(json!({
                    "type": "tool_use",
                    "id": id.unwrap_or_else(|| format!("fake_toolu_{call_index}")),
                    "name": name,
                    "input": input,
                }));
                Some("tool_use".to_string())
            }
            FakeProviderStep::ProviderError { code, message } => {
                return Err(ProviderError::Provider {
                    provider_id: FAKE_PROVIDER_ID.to_string(),
                    code: code.unwrap_or_else(|| "fake_provider_error".to_string()),
                    message,
                });
            }
        };
        Ok(MessagesResponse {
            id: format!("fake-msg-{call_index}"),
            model: model.to_string(),
            role: "assistant".to_string(),
            content,
            stop_reason,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
        })
    }
}

#[async_trait]
impl Provider for FakeProvider {
    fn provider_id(&self) -> &str {
        FAKE_PROVIDER_ID
    }

    fn model_profile(&self, model_id: &str) -> ModelProfile {
        model_profile(FAKE_PROVIDER_ID, model_id).unwrap_or_else(|| self.profile.clone())
    }

    async fn create_message(&self, request: MessagesRequest) -> ProviderResult<MessagesResponse> {
        self.ensure_request_supported(&request)?;
        let (call_index, step) = self.next_step()?;
        self.response_from_step(&request.model, call_index, step)
    }

    async fn stream_message(&self, mut request: MessagesRequest) -> ProviderResult<ProviderStream> {
        request.stream = Some(true);
        self.ensure_request_supported(&request)?;
        let response = self.create_message(request).await?;
        let events = stream_events_from_response(&response);
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

fn parse_fake_script_value(value: &Value) -> ProviderResult<Vec<FakeProviderStep>> {
    serde_json::from_value(value.clone()).map_err(|error| ProviderError::Provider {
        provider_id: FAKE_PROVIDER_ID.to_string(),
        code: "invalid_fake_provider_script".to_string(),
        message: error.to_string(),
    })
}

fn parse_fake_script_text(raw: &str) -> ProviderResult<Vec<FakeProviderStep>> {
    serde_json::from_str(raw).map_err(|error| ProviderError::Provider {
        provider_id: FAKE_PROVIDER_ID.to_string(),
        code: "invalid_fake_provider_script".to_string(),
        message: error.to_string(),
    })
}

fn stream_events_from_response(response: &MessagesResponse) -> Vec<StreamEvent> {
    let mut events = vec![StreamEvent::MessageStart {
        message: MessageStart {
            id: response.id.clone(),
            model: response.model.clone(),
            role: response.role.clone(),
            usage: DeltaUsage {
                input_tokens: response.usage.input_tokens,
                output_tokens: 0,
            },
        },
    }];

    for (index, block) in response.content.iter().enumerate() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: StreamContentBlock::Text {
                        text: String::new(),
                    },
                });
                if !text.is_empty() {
                    events.push(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::TextDelta { text },
                    });
                }
                events.push(StreamEvent::ContentBlockStop { index });
            }
            Some("tool_use") => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: StreamContentBlock::ToolUse(super::streaming::ToolUse {
                        id: block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("fake_toolu")
                            .to_string(),
                        name: block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Unknown")
                            .to_string(),
                        input: block.get("input").cloned().unwrap_or_else(|| json!({})),
                    }),
                });
                events.push(StreamEvent::ContentBlockStop { index });
            }
            _ => {}
        }
    }

    events.push(StreamEvent::MessageDelta {
        delta: MessageDelta {
            stop_reason: response.stop_reason.clone(),
        },
        usage: DeltaUsage {
            input_tokens: 0,
            output_tokens: response.usage.output_tokens,
        },
    });
    events.push(StreamEvent::MessageStop);
    events
}

#[cfg(test)]
mod tests {
    use super::super::messages::Message;
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn model_profiles_are_serializable() {
        let profiles = built_in_model_profiles();
        let value = serde_json::to_value(&profiles).unwrap();
        assert_eq!(value[0]["provider_id"], ANTHROPIC_PROVIDER_ID);
        assert!(value
            .as_array()
            .unwrap()
            .iter()
            .any(
                |profile| profile["provider_id"].as_str() == Some(FAKE_PROVIDER_ID)
                    && profile["model_id"].as_str() == Some(FAKE_MODEL_ID)
                    && profile["supports_tools"].as_bool() == Some(true)
            ));
        assert!(value.as_array().unwrap().iter().any(|profile| {
            profile["provider_id"].as_str() == Some(OPENAI_COMPATIBLE_PROVIDER_ID)
                && profile["model_id"].as_str() == Some(OPENAI_COMPATIBLE_DEFAULT_MODEL_ID)
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
        }));
        assert!(value.as_array().unwrap().iter().any(|profile| {
            profile["provider_id"].as_str() == Some(OLLAMA_PROVIDER_ID)
                && profile["model_id"].as_str() == Some(OLLAMA_DEFAULT_MODEL_ID)
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
        }));
    }

    #[tokio::test]
    async fn openai_compatible_provider_sends_chat_completions_request() {
        let (base_url, mut request_rx, server) = start_mock_openai_compatible_server(
            200,
            json!({
                "id": "chatcmpl_test",
                "model": "gpt-test",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "hello from openai"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 11,
                    "completion_tokens": 3
                }
            }),
        )
        .await;
        let provider = OpenAiCompatibleProvider::new(
            "openai-test-key".to_string(),
            base_url,
            Duration::from_secs(5),
        )
        .unwrap();

        let response = provider
            .create_message(MessagesRequest {
                model: "gpt-test".to_string(),
                messages: vec![Message {
                    role: "user".to_string(),
                    content: json!("hello"),
                }],
                max_tokens: 64,
                system: Some(json!("system prompt")),
                temperature: Some(0.2),
                tools: None,
                thinking: None,
                stream: None,
            })
            .await
            .unwrap();

        assert_eq!(response.id, "chatcmpl_test");
        assert_eq!(response.model, "gpt-test");
        assert_eq!(response.role, "assistant");
        assert_eq!(response.content[0]["text"], "hello from openai");
        assert_eq!(response.stop_reason.as_deref(), Some("stop"));
        assert_eq!(response.usage.input_tokens, 11);
        assert_eq!(response.usage.output_tokens, 3);

        let request = request_rx.recv().await.unwrap();
        assert!(request.starts_with("POST /chat/completions HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer openai-test-key"));
        let body: Value = serde_json::from_str(http_body(&request)).unwrap();
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["stream"], false);
        assert!((body["temperature"].as_f64().unwrap() - 0.2).abs() < 0.000_001);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "system prompt");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "hello");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn openai_compatible_provider_maps_auth_errors() {
        let (base_url, mut request_rx, server) = start_mock_openai_compatible_server(
            401,
            json!({
                "error": {
                    "code": "invalid_api_key",
                    "message": "invalid key"
                }
            }),
        )
        .await;
        let provider =
            OpenAiCompatibleProvider::new("bad-key".to_string(), base_url, Duration::from_secs(5))
                .unwrap();

        let error = provider
            .create_message(MessagesRequest {
                model: "gpt-test".to_string(),
                messages: vec![Message {
                    role: "user".to_string(),
                    content: json!("hello"),
                }],
                max_tokens: 64,
                system: None,
                temperature: None,
                tools: None,
                thinking: None,
                stream: None,
            })
            .await
            .unwrap_err();

        assert_eq!(error.code(), "auth_error");
        assert!(error.to_string().contains("invalid key"));
        assert!(request_rx.recv().await.is_some());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn openai_compatible_provider_maps_tools_and_tool_calls() {
        let (base_url, mut request_rx, server) = start_mock_openai_compatible_server(
            200,
            json!({
                "id": "chatcmpl_tool",
                "model": "gpt-test",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "I'll inspect it.",
                        "tool_calls": [{
                            "id": "call_read",
                            "type": "function",
                            "function": {
                                "name": "Read",
                                "arguments": "{\"file_path\":\"README.md\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 4
                }
            }),
        )
        .await;
        let provider = OpenAiCompatibleProvider::new(
            "openai-test-key".to_string(),
            base_url,
            Duration::from_secs(5),
        )
        .unwrap();

        let response = provider
            .create_message(MessagesRequest {
                model: "gpt-test".to_string(),
                messages: vec![
                    Message {
                        role: "user".to_string(),
                        content: json!("read the README"),
                    },
                    Message {
                        role: "assistant".to_string(),
                        content: json!([
                            {
                                "type": "text",
                                "text": "Calling Read"
                            },
                            {
                                "type": "tool_use",
                                "id": "call_previous",
                                "name": "Read",
                                "input": {
                                    "file_path": "README.md"
                                }
                            }
                        ]),
                    },
                    Message {
                        role: "user".to_string(),
                        content: json!([{
                            "type": "tool_result",
                            "tool_use_id": "call_previous",
                            "content": "README contents"
                        }]),
                    },
                ],
                max_tokens: 64,
                system: None,
                temperature: None,
                tools: Some(vec![json!({
                    "name": "Read",
                    "description": "Read a file",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"}
                        },
                        "required": ["file_path"]
                    }
                })]),
                thinking: None,
                stream: None,
            })
            .await
            .unwrap();

        assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(response.content[0]["type"], "text");
        assert_eq!(response.content[0]["text"], "I'll inspect it.");
        assert_eq!(response.content[1]["type"], "tool_use");
        assert_eq!(response.content[1]["id"], "call_read");
        assert_eq!(response.content[1]["name"], "Read");
        assert_eq!(response.content[1]["input"]["file_path"], "README.md");

        let request = request_rx.recv().await.unwrap();
        let body: Value = serde_json::from_str(http_body(&request)).unwrap();
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "Read");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["file_path"]["type"],
            "string"
        );
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call_previous");
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["arguments"],
            "{\"file_path\":\"README.md\"}"
        );
        assert_eq!(body["messages"][2]["role"], "tool");
        assert_eq!(body["messages"][2]["tool_call_id"], "call_previous");
        assert_eq!(body["messages"][2]["content"], "README contents");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn ollama_provider_sends_chat_request() {
        let (base_url, mut request_rx, server) = start_mock_ollama_server(json!({
            "model": "llama-test",
            "created_at": "2026-07-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": "hello from ollama"
            },
            "done": true,
            "done_reason": "stop",
            "prompt_eval_count": 7,
            "eval_count": 4
        }))
        .await;
        let provider =
            OllamaProvider::new(base_url, Duration::from_secs(5)).expect("provider builds");

        let response = provider
            .create_message(MessagesRequest {
                model: "llama-test".to_string(),
                messages: vec![Message {
                    role: "user".to_string(),
                    content: json!("hello"),
                }],
                max_tokens: 64,
                system: Some(json!("system prompt")),
                temperature: Some(0.1),
                tools: None,
                thinking: None,
                stream: None,
            })
            .await
            .unwrap();

        assert_eq!(response.id, "2026-07-01T00:00:00Z");
        assert_eq!(response.model, "llama-test");
        assert_eq!(response.role, "assistant");
        assert_eq!(response.content[0]["text"], "hello from ollama");
        assert_eq!(response.stop_reason.as_deref(), Some("stop"));
        assert_eq!(response.usage.input_tokens, 7);
        assert_eq!(response.usage.output_tokens, 4);

        let request = request_rx.recv().await.unwrap();
        assert!(request.starts_with("POST /api/chat HTTP/1.1"));
        let body: Value = serde_json::from_str(http_body(&request)).unwrap();
        assert_eq!(body["model"], "llama-test");
        assert_eq!(body["stream"], false);
        assert_eq!(body["options"]["num_predict"], 64);
        assert!((body["options"]["temperature"].as_f64().unwrap() - 0.1).abs() < 0.000_001);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "system prompt");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "hello");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn ollama_provider_maps_tools_and_tool_calls() {
        let (base_url, mut request_rx, server) = start_mock_ollama_server(json!({
            "model": "llama-test",
            "created_at": "2026-07-01T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "function": {
                        "name": "Read",
                        "arguments": {
                            "file_path": "README.md"
                        }
                    }
                }]
            },
            "done": true,
            "done_reason": "stop",
            "prompt_eval_count": 9,
            "eval_count": 2
        }))
        .await;
        let provider = OllamaProvider::new(base_url, Duration::from_secs(5)).unwrap();

        let response = provider
            .create_message(MessagesRequest {
                model: OLLAMA_DEFAULT_MODEL_ID.to_string(),
                messages: vec![
                    Message {
                        role: "user".to_string(),
                        content: json!("read the README"),
                    },
                    Message {
                        role: "assistant".to_string(),
                        content: json!([{
                            "type": "tool_use",
                            "id": "call_previous",
                            "name": "Read",
                            "input": {
                                "file_path": "README.md"
                            }
                        }]),
                    },
                    Message {
                        role: "user".to_string(),
                        content: json!([{
                            "type": "tool_result",
                            "tool_use_id": "call_previous",
                            "content": "README contents"
                        }]),
                    },
                ],
                max_tokens: 64,
                system: None,
                temperature: None,
                tools: Some(vec![json!({
                    "name": "Read",
                    "description": "Read a file",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"}
                        }
                    }
                })]),
                thinking: None,
                stream: None,
            })
            .await
            .unwrap();

        assert_eq!(response.content[0]["type"], "tool_use");
        assert_eq!(response.content[0]["id"], "toolu_ollama_0");
        assert_eq!(response.content[0]["name"], "Read");
        assert_eq!(response.content[0]["input"]["file_path"], "README.md");
        assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));

        let request = request_rx.recv().await.unwrap();
        let body: Value = serde_json::from_str(http_body(&request)).unwrap();
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "Read");
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["name"],
            "Read"
        );
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["arguments"]["file_path"],
            "README.md"
        );
        assert_eq!(body["messages"][2]["role"], "tool");
        assert_eq!(body["messages"][2]["tool_name"], "Read");
        assert_eq!(body["messages"][2]["content"], "README contents");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn fake_provider_returns_assistant_text() {
        let provider = FakeProvider::new(
            FAKE_MODEL_ID.to_string(),
            vec![FakeProviderStep::AssistantText {
                text: "hello from fake".to_string(),
            }],
        );

        let response = provider
            .create_message(MessagesRequest {
                model: FAKE_MODEL_ID.to_string(),
                messages: vec![],
                max_tokens: 128,
                system: None,
                temperature: None,
                tools: None,
                thinking: None,
                stream: None,
            })
            .await
            .unwrap();

        assert_eq!(response.content[0]["text"], "hello from fake");
    }

    #[tokio::test]
    async fn fake_provider_can_script_tool_call_then_final_answer() {
        let provider = FakeProvider::new(
            FAKE_MODEL_ID.to_string(),
            vec![
                FakeProviderStep::ToolCall {
                    id: Some("toolu_read".to_string()),
                    name: "Read".to_string(),
                    input: json!({ "file_path": "README.md" }),
                    text: Some("reading".to_string()),
                },
                FakeProviderStep::FinalAnswer {
                    text: "done".to_string(),
                },
            ],
        );
        let request = MessagesRequest {
            model: FAKE_MODEL_ID.to_string(),
            messages: vec![],
            max_tokens: 128,
            system: None,
            temperature: None,
            tools: Some(vec![json!({"name": "Read"})]),
            thinking: None,
            stream: None,
        };

        let first = provider.create_message(request).await.unwrap();
        assert_eq!(first.content[1]["name"], "Read");
        let second = provider
            .create_message(MessagesRequest {
                model: FAKE_MODEL_ID.to_string(),
                messages: vec![],
                max_tokens: 128,
                system: None,
                temperature: None,
                tools: Some(vec![json!({"name": "Read"})]),
                thinking: None,
                stream: None,
            })
            .await
            .unwrap();
        assert_eq!(second.content[0]["text"], "done");
    }

    #[tokio::test]
    async fn unsupported_tools_profile_fails_before_provider_step_is_consumed() {
        let provider = FakeProvider::new(
            FAKE_TEXT_ONLY_MODEL_ID.to_string(),
            vec![FakeProviderStep::AssistantText {
                text: "should not be consumed".to_string(),
            }],
        );

        let error = provider
            .create_message(MessagesRequest {
                model: FAKE_TEXT_ONLY_MODEL_ID.to_string(),
                messages: vec![],
                max_tokens: 128,
                system: None,
                temperature: None,
                tools: Some(vec![json!({"name": "Read"})]),
                thinking: None,
                stream: None,
            })
            .await
            .unwrap_err();

        assert_eq!(error.code(), "unsupported_tools");
        assert_eq!(provider.steps.lock().unwrap().len(), 1);
    }

    async fn start_mock_openai_compatible_server(
        status: u16,
        response: Value,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut socket).await;
            request_tx.send(request).unwrap();
            let body = serde_json::to_string(&response).unwrap();
            let reason = match status {
                200 => "OK",
                401 => "Unauthorized",
                403 => "Forbidden",
                _ => "Error",
            };
            let raw = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(raw.as_bytes()).await.unwrap();
        });
        (format!("http://{addr}"), request_rx, server)
    }

    async fn start_mock_ollama_server(
        response: Value,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut socket).await;
            request_tx.send(request).unwrap();
            let body = serde_json::to_string(&response).unwrap();
            let raw = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(raw.as_bytes()).await.unwrap();
        });
        (format!("http://{addr}"), request_rx, server)
    }

    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = socket.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(expected_len) = expected_http_request_len(&buffer) {
                if buffer.len() >= expected_len {
                    break;
                }
            }
        }
        String::from_utf8(buffer).unwrap()
    }

    fn expected_http_request_len(buffer: &[u8]) -> Option<usize> {
        let header_end = find_header_end(buffer)?;
        let headers = std::str::from_utf8(&buffer[..header_end]).ok()?;
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        Some(header_end + 4 + content_length)
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn http_body(request: &str) -> &str {
        request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap()
    }
}
