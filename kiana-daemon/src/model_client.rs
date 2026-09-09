//! Composition-root model injection for the owned harness.
//!
//! `kiana-runner` never depends on `kiana-services`. Scripted turns win via
//! `KIANA_HARNESS_SCRIPT`; otherwise daemon wraps a provider client; missing
//! credentials fail closed.

use async_trait::async_trait;
use kiana_domain::RoleSpec;
use kiana_runner::{
    ModelClient, ModelDelta, ModelMessage, ModelOutput, ModelRequest, ModelRole, ModelToolCall,
    ModelUsage, ScriptedModel, UnavailableModel,
};
use kiana_services::api::errors::ApiErrorKind;
use kiana_services::api::messages::{Message, MessagesRequest, MessagesResponse};
use kiana_services::api::provider::{
    next_provider_stream_event, provider_registry_entry, AnthropicProvider, FakeProvider,
    OllamaProvider, OpenAiCompatibleProvider, Provider, ProviderError, ProviderStream,
    ANTHROPIC_PROVIDER_ID, FAKE_PROVIDER_ID, OLLAMA_PROVIDER_ID, OPENAI_COMPATIBLE_PROVIDER_ID,
};
use kiana_services::api::retry::{with_retry, RetryConfig};
use kiana_services::api::streaming::{
    ContentBlock as StreamContentBlock, Delta as StreamDelta, StreamEvent,
};
use kiana_services::errors::ServiceError;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::LocalModelConfig;

const ENV_HARNESS_SCRIPT: &str = "KIANA_HARNESS_SCRIPT";
const ENV_PROVIDER: &str = "KIANA_PROVIDER";
const ENV_STREAMING: &str = "KIANA_STREAMING";
const DEFAULT_MAX_TOKENS: u32 = 4096;
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(60);
const MODEL_MAX_RETRIES: u32 = 2;
const MODEL_RETRY_BASE_DELAY: Duration = Duration::from_millis(100);
const MODEL_UNAVAILABLE_HINT: &str = "没有可用的模型。请设置 ANTHROPIC_API_KEY；或设置 KIANA_PROVIDER=ollama（可用 KIANA_OLLAMA_BASE_URL 指定地址，默认 http://localhost:11434）；或设置 KIANA_HARNESS_SCRIPT=/path/to/cassette.json 运行本地 cassette。";

pub(crate) fn from_env() -> Arc<dyn ModelClient> {
    from_config(LocalModelConfig::default())
}

pub(crate) fn from_config(config: LocalModelConfig) -> Arc<dyn ModelClient> {
    match std::env::var(ENV_HARNESS_SCRIPT) {
        Ok(path) if !path.trim().is_empty() => match ScriptedModel::from_json_path(path.trim()) {
            Ok(model) => Arc::new(model),
            Err(error) => Arc::new(unavailable_model(Some(&error))),
        },
        _ => match provider_from_env(config) {
            Ok(Some(model)) => model,
            Ok(None) => Arc::new(unavailable_model(None)),
            Err(error) => Arc::new(unavailable_model(Some(&error))),
        },
    }
}

fn unavailable_model(reason: Option<&str>) -> UnavailableModel {
    let message = match reason.map(str::trim).filter(|reason| !reason.is_empty()) {
        Some(reason) => format!("model_unavailable: {reason}. {MODEL_UNAVAILABLE_HINT}"),
        None => format!("model_unavailable: {MODEL_UNAVAILABLE_HINT}"),
    };
    UnavailableModel::new(message)
}

fn provider_from_env(config: LocalModelConfig) -> Result<Option<Arc<dyn ModelClient>>, String> {
    let provider_id = config
        .provider
        .or_else(|| std::env::var(ENV_PROVIDER).ok())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ANTHROPIC_PROVIDER_ID.to_owned());
    let Some(entry) = provider_registry_entry(&provider_id) else {
        return Err(format!("unknown_provider:{provider_id}"));
    };
    let model_id = config
        .model
        .filter(|value| !value.trim().is_empty())
        .or_else(|| first_env(&entry.model_env_vars))
        .unwrap_or(entry.default_model_id);
    let provider: Box<dyn Provider> = match provider_id.as_str() {
        FAKE_PROVIDER_ID => Box::new(
            FakeProvider::from_script_value(model_id.clone(), None)
                .map_err(|error| error.to_string())?,
        ),
        id if id == ANTHROPIC_PROVIDER_ID => {
            let Some(api_key) = config
                .api_key
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| first_env(&entry.api_key_env_vars))
            else {
                return Ok(None);
            };
            let base_url = config
                .base_url
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| first_env(&entry.base_url_env_vars))
                .or(entry.default_base_url)
                .unwrap_or_default();
            Box::new(AnthropicProvider::new(api_key, base_url, PROVIDER_TIMEOUT))
        }
        id if id == OPENAI_COMPATIBLE_PROVIDER_ID => {
            let Some(api_key) = config
                .api_key
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| first_env(&entry.api_key_env_vars))
            else {
                return Ok(None);
            };
            let base_url = config
                .base_url
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| first_env(&entry.base_url_env_vars))
                .or(entry.default_base_url)
                .unwrap_or_default();
            Box::new(
                OpenAiCompatibleProvider::new(api_key, base_url, PROVIDER_TIMEOUT)
                    .map_err(|error| error.to_string())?,
            )
        }
        id if id == OLLAMA_PROVIDER_ID => {
            let base_url = config
                .base_url
                .filter(|value| !value.trim().is_empty())
                .or_else(|| first_env(&entry.base_url_env_vars))
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
        streaming_policy: streaming_enabled_from_env(),
    })))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamingPolicy {
    Off,
    Auto,
    On,
}

fn streaming_enabled_from_env() -> StreamingPolicy {
    match std::env::var(ENV_STREAMING)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("off" | "0" | "false" | "no") => StreamingPolicy::Off,
        Some("on" | "1" | "true" | "yes") => StreamingPolicy::On,
        Some("auto") | None | Some(_) => StreamingPolicy::Auto,
    }
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
    streaming_policy: StreamingPolicy,
}

#[async_trait]
impl ModelClient for ProviderModelClient {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        let messages = map_messages(&request.messages)?;
        let tools = map_tools(&request.tools);
        let system = Some(json!(provider_system_prompt()));
        let response = with_retry(
            || {
                let messages = messages.clone();
                let tools = tools.clone();
                let system = system.clone();
                async move {
                    self.provider
                        .create_message(MessagesRequest {
                            model: self.model.clone(),
                            messages,
                            max_tokens: DEFAULT_MAX_TOKENS,
                            system,
                            temperature: None,
                            tools: if tools.is_empty() { None } else { Some(tools) },
                            thinking: None,
                            stream: Some(false),
                        })
                        .await
                        .map_err(provider_error_to_service_error)
                }
            },
            model_retry_config(),
        )
        .await
        .map_err(model_error_from_service_error)?;
        Ok(output_from_response(&response))
    }

    async fn complete_streaming(
        &self,
        request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        let profile = self.provider.model_profile(&self.model);
        match self.streaming_policy {
            StreamingPolicy::Off => {
                return self.complete_as_single_delta(request, on_delta).await;
            }
            StreamingPolicy::Auto if !profile.native_streaming => {
                return self.complete_as_single_delta(request, on_delta).await;
            }
            StreamingPolicy::On if !profile.native_streaming => {
                return Err(unsupported_streaming_error(
                    self.provider.provider_id(),
                    &self.model,
                ));
            }
            StreamingPolicy::Auto | StreamingPolicy::On => {}
        }

        let messages = map_messages(&request.messages)?;
        let tools = map_tools(&request.tools);
        let system = Some(json!(provider_system_prompt()));
        let stream = with_retry(
            || {
                let messages = messages.clone();
                let tools = tools.clone();
                let system = system.clone();
                async move {
                    self.provider
                        .stream_message(MessagesRequest {
                            model: self.model.clone(),
                            messages,
                            max_tokens: DEFAULT_MAX_TOKENS,
                            system,
                            temperature: None,
                            tools: if tools.is_empty() { None } else { Some(tools) },
                            thinking: None,
                            stream: Some(true),
                        })
                        .await
                        .map_err(provider_error_to_service_error)
                }
            },
            model_retry_config(),
        )
        .await
        .map_err(model_error_from_service_error)?;

        aggregate_provider_stream(self.provider.provider_id(), &self.model, stream, on_delta).await
    }
}

fn model_retry_config() -> RetryConfig {
    RetryConfig {
        max_retries: MODEL_MAX_RETRIES,
        base_delay: MODEL_RETRY_BASE_DELAY,
    }
}

fn unsupported_streaming_error(provider_id: &str, model_id: &str) -> String {
    model_error_from_service_error(provider_error_to_service_error(
        ProviderError::UnsupportedCapability {
            provider_id: provider_id.to_owned(),
            model_id: model_id.to_owned(),
            capability: "streaming".to_owned(),
            message: format!("model {provider_id}/{model_id} does not support native streaming"),
        },
    ))
}

impl ProviderModelClient {
    async fn complete_as_single_delta(
        &self,
        request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        let output = self.complete(request).await?;
        if !output.text.is_empty() {
            on_delta(ModelDelta::Text {
                text: output.text.clone(),
            })?;
        }
        Ok(output)
    }
}

enum StreamBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        initial_input: Value,
        input_json: String,
    },
}

async fn aggregate_provider_stream(
    provider_id: &str,
    fallback_model_id: &str,
    mut stream: ProviderStream,
    on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
) -> Result<ModelOutput, String> {
    let mut blocks = BTreeMap::<usize, StreamBlock>::new();
    let mut input_tokens = 0_u32;
    let mut output_tokens = 0_u32;
    let mut stop_reason = None;
    let mut model_id = Some(fallback_model_id.to_owned());

    while let Some(event) = next_provider_stream_event(&mut stream).await {
        match event.map_err(|error| {
            model_error_from_service_error(provider_error_to_service_error(error))
        })? {
            StreamEvent::MessageStart { message } => {
                input_tokens = message.usage.input_tokens;
                model_id = Some(message.model);
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => match content_block {
                StreamContentBlock::Text { text } => {
                    if !text.is_empty() {
                        on_delta(ModelDelta::Text { text: text.clone() })?;
                    }
                    blocks.insert(index, StreamBlock::Text(text));
                }
                StreamContentBlock::ToolUse(tool) => {
                    blocks.insert(
                        index,
                        StreamBlock::ToolUse {
                            id: tool.id,
                            name: tool.name,
                            initial_input: tool.input,
                            input_json: String::new(),
                        },
                    );
                }
                StreamContentBlock::Thinking { .. }
                | StreamContentBlock::RedactedThinking { .. } => {}
            },
            StreamEvent::ContentBlockDelta { index, delta } => match delta {
                StreamDelta::TextDelta { text } => {
                    let Some(StreamBlock::Text(current)) = blocks.get_mut(&index) else {
                        return Err(format!("stream_text_delta_without_content_block:{index}"));
                    };
                    current.push_str(&text);
                    if !text.is_empty() {
                        on_delta(ModelDelta::Text { text })?;
                    }
                }
                StreamDelta::InputJsonDelta { partial_json } => {
                    let Some(StreamBlock::ToolUse { input_json, .. }) = blocks.get_mut(&index)
                    else {
                        return Err(format!("stream_tool_delta_without_content_block:{index}"));
                    };
                    input_json.push_str(&partial_json);
                }
                StreamDelta::ThinkingDelta { .. } | StreamDelta::SignatureDelta { .. } => {}
            },
            StreamEvent::ContentBlockStop { .. } | StreamEvent::Ping | StreamEvent::MessageStop => {
            }
            StreamEvent::MessageDelta { delta, usage } => {
                output_tokens = usage.output_tokens;
                stop_reason = delta.stop_reason;
            }
            StreamEvent::Error { error } => {
                let code = error
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("provider_stream_error");
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("provider stream error");
                return Err(model_error_from_service_error(
                    provider_error_to_service_error(ProviderError::Provider {
                        provider_id: provider_id.to_owned(),
                        code: code.to_owned(),
                        message: message.to_owned(),
                    }),
                ));
            }
        }
    }

    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for (index, block) in blocks {
        match block {
            StreamBlock::Text(chunk) => text.push_str(&chunk),
            StreamBlock::ToolUse {
                id,
                name,
                initial_input,
                input_json,
            } => {
                let arguments = if input_json.trim().is_empty() {
                    initial_input
                } else {
                    serde_json::from_str(&input_json)
                        .map_err(|error| format!("invalid_stream_tool_input:{index}:{error}"))?
                };
                tool_calls.push(ModelToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }
    }

    Ok(ModelOutput {
        text,
        tool_calls,
        usage: Some(ModelUsage {
            input_tokens: u64::from(input_tokens),
            output_tokens: u64::from(output_tokens),
        }),
        stop_reason,
        model_id,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderRetryClass {
    None,
    Timeout,
    Connection,
    RateLimit,
    ServerUnavailable,
}

fn provider_error_to_service_error(error: ProviderError) -> ServiceError {
    let retry_class = provider_error_retry_class(&error);
    let original = error.to_string();
    match retry_class {
        ProviderRetryClass::RateLimit => ServiceError::RateLimit(original),
        ProviderRetryClass::ServerUnavailable => ServiceError::Http {
            status: 503,
            body: original,
        },
        ProviderRetryClass::Timeout | ProviderRetryClass::Connection => {
            ServiceError::Connection(original)
        }
        ProviderRetryClass::None => match error {
            ProviderError::UnsupportedCapability { .. } => {
                ServiceError::Unknown(error.code().to_owned())
            }
            _ => ServiceError::Unknown(original),
        },
    }
}

fn provider_error_retry_class(error: &ProviderError) -> ProviderRetryClass {
    match error {
        ProviderError::Api(error) => match error.kind {
            ApiErrorKind::RateLimit => ProviderRetryClass::RateLimit,
            ApiErrorKind::ApiTimeout => ProviderRetryClass::Timeout,
            ApiErrorKind::Repeated529 | ApiErrorKind::ServerOverload => {
                ProviderRetryClass::ServerUnavailable
            }
            ApiErrorKind::ConnectionError | ApiErrorKind::SslCertError => {
                ProviderRetryClass::Connection
            }
            _ => ProviderRetryClass::None,
        },
        ProviderError::Provider { code, message, .. } => {
            let code = code.to_ascii_lowercase();
            let message = message.to_ascii_lowercase();
            if code.contains("rate_limit")
                || code == "429"
                || code.contains("too_many_requests")
                || message.contains("429")
                || message.contains("rate limit")
                || message.contains("too many requests")
            {
                ProviderRetryClass::RateLimit
            } else if code.contains("server_overload")
                || code.contains("overloaded")
                || code == "503"
                || code.contains("503")
                || message.contains("503")
                || message.contains("service unavailable")
                || message.contains("overloaded")
            {
                ProviderRetryClass::ServerUnavailable
            } else if code.contains("timeout")
                || code == "408"
                || message.contains("timeout")
                || message.contains("timed out")
            {
                ProviderRetryClass::Timeout
            } else if code.contains("connection")
                || code == "request_failed"
                || message.contains("connection")
                || message.contains("connect error")
            {
                ProviderRetryClass::Connection
            } else {
                ProviderRetryClass::None
            }
        }
        _ => ProviderRetryClass::None,
    }
}

fn model_error_from_service_error(error: ServiceError) -> String {
    match error {
        ServiceError::Unknown(message)
        | ServiceError::Auth(message)
        | ServiceError::RateLimit(message)
        | ServiceError::Connection(message) => message,
        ServiceError::Http { body, .. } => body,
        other => other.to_string(),
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

fn output_from_response(response: &MessagesResponse) -> ModelOutput {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in &response.content {
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
    ModelOutput {
        text,
        tool_calls,
        usage: Some(ModelUsage {
            input_tokens: u64::from(response.usage.input_tokens),
            output_tokens: u64::from(response.usage.output_tokens),
        }),
        stop_reason: response.stop_reason.clone(),
        model_id: Some(response.model.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use kiana_services::api::messages::{MessagesResponse, Usage};
    use kiana_services::api::provider::{
        provider_stream_from_events, FakeProviderStep, ModelProfile, ProviderResult, StreamingMode,
        FAKE_TEXT_ONLY_MODEL_ID,
    };
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct TempScript {
        path: PathBuf,
    }

    impl TempScript {
        fn new(contents: &str) -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "kiana-model-client-test-{}-{id}.json",
                std::process::id()
            ));
            std::fs::write(&path, contents).expect("write temporary harness script");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempScript {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    struct TrackingProvider {
        native_streaming: bool,
        stream_called: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Provider for TrackingProvider {
        fn provider_id(&self) -> &str {
            "tracking"
        }

        fn model_profile(&self, model_id: &str) -> ModelProfile {
            ModelProfile {
                provider_id: "tracking".to_owned(),
                provider_display_name: "Tracking".to_owned(),
                model_id: model_id.to_owned(),
                supports_tools: true,
                supports_streaming: true,
                streaming_mode: if self.native_streaming {
                    StreamingMode::Native
                } else {
                    StreamingMode::Synthetic
                },
                native_streaming: self.native_streaming,
                supports_vision: false,
                supports_structured_output: false,
                context_window: 8_192,
            }
        }

        async fn create_message(
            &self,
            request: MessagesRequest,
        ) -> ProviderResult<MessagesResponse> {
            Ok(MessagesResponse {
                id: "tracking-msg".to_owned(),
                model: request.model,
                role: "assistant".to_owned(),
                content: vec![json!({
                    "type": "text",
                    "text": "fallback text",
                })],
                stop_reason: Some("end_turn".to_owned()),
                usage: Usage {
                    input_tokens: 4,
                    output_tokens: 2,
                },
            })
        }

        async fn stream_message(
            &self,
            _request: MessagesRequest,
        ) -> ProviderResult<ProviderStream> {
            self.stream_called.store(true, Ordering::SeqCst);
            panic!("stream_message must not be called when native streaming is unavailable");
        }
    }

    struct EventProvider {
        events: Vec<StreamEvent>,
    }

    #[async_trait]
    impl Provider for EventProvider {
        fn provider_id(&self) -> &str {
            "event-provider"
        }

        fn model_profile(&self, model_id: &str) -> ModelProfile {
            ModelProfile {
                provider_id: "event-provider".to_owned(),
                provider_display_name: "Event Provider".to_owned(),
                model_id: model_id.to_owned(),
                supports_tools: true,
                supports_streaming: true,
                streaming_mode: StreamingMode::Native,
                native_streaming: true,
                supports_vision: false,
                supports_structured_output: false,
                context_window: 8_192,
            }
        }

        async fn create_message(
            &self,
            _request: MessagesRequest,
        ) -> ProviderResult<MessagesResponse> {
            panic!("event provider must only be used through the streaming path");
        }

        async fn stream_message(
            &self,
            _request: MessagesRequest,
        ) -> ProviderResult<ProviderStream> {
            Ok(provider_stream_from_events(self.events.clone()))
        }
    }

    struct RetryingStreamProvider {
        attempts: Arc<AtomicU64>,
        events: Vec<StreamEvent>,
    }

    #[async_trait]
    impl Provider for RetryingStreamProvider {
        fn provider_id(&self) -> &str {
            "retrying-stream-provider"
        }

        fn model_profile(&self, model_id: &str) -> ModelProfile {
            ModelProfile {
                provider_id: "retrying-stream-provider".to_owned(),
                provider_display_name: "Retrying Stream Provider".to_owned(),
                model_id: model_id.to_owned(),
                supports_tools: true,
                supports_streaming: true,
                streaming_mode: StreamingMode::Native,
                native_streaming: true,
                supports_vision: false,
                supports_structured_output: false,
                context_window: 8_192,
            }
        }

        async fn create_message(
            &self,
            _request: MessagesRequest,
        ) -> ProviderResult<MessagesResponse> {
            panic!("retrying stream provider must not use the non-streaming fallback");
        }

        async fn stream_message(
            &self,
            _request: MessagesRequest,
        ) -> ProviderResult<ProviderStream> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                return Err(ProviderError::Provider {
                    provider_id: self.provider_id().to_owned(),
                    code: "server_overload".to_owned(),
                    message: "retry before first delta".to_owned(),
                });
            }
            Ok(provider_stream_from_events(self.events.clone()))
        }
    }

    fn tracking_client(
        native_streaming: bool,
        streaming_policy: StreamingPolicy,
    ) -> (ProviderModelClient, Arc<AtomicBool>) {
        let stream_called = Arc::new(AtomicBool::new(false));
        let client = ProviderModelClient {
            provider: Box::new(TrackingProvider {
                native_streaming,
                stream_called: Arc::clone(&stream_called),
            }),
            model: "tracking-model".to_owned(),
            streaming_policy,
        };
        (client, stream_called)
    }

    #[test]
    fn provider_env_anthropic_with_key_selects_provider_client() {
        let _lock = env_lock();
        let _script = EnvGuard::remove(ENV_HARNESS_SCRIPT);
        let _provider = EnvGuard::set(ENV_PROVIDER, ANTHROPIC_PROVIDER_ID);

        let without_key = provider_from_env(LocalModelConfig::default())
            .expect("anthropic provider configuration should be valid");
        assert!(
            without_key.is_none(),
            "anthropic without ANTHROPIC_API_KEY should not select a provider client"
        );

        let _key = EnvGuard::set("ANTHROPIC_API_KEY", "test-key");
        let with_key = provider_from_env(LocalModelConfig::default())
            .expect("anthropic provider configuration should be valid");
        assert!(
            with_key.is_some(),
            "KIANA_PROVIDER=anthropic with ANTHROPIC_API_KEY should select a provider client"
        );
    }

    #[test]
    fn streaming_env_parses_off_auto_and_on_case_insensitively() {
        let _lock = env_lock();
        let _streaming = EnvGuard::remove(ENV_STREAMING);
        assert_eq!(streaming_enabled_from_env(), StreamingPolicy::Auto);

        for value in ["off", "0", "FALSE", "No"] {
            let _streaming = EnvGuard::set(ENV_STREAMING, value);
            assert_eq!(
                streaming_enabled_from_env(),
                StreamingPolicy::Off,
                "KIANA_STREAMING={value}"
            );
        }

        for value in ["on", "1", "TRUE", "Yes"] {
            let _streaming = EnvGuard::set(ENV_STREAMING, value);
            assert_eq!(
                streaming_enabled_from_env(),
                StreamingPolicy::On,
                "KIANA_STREAMING={value}"
            );
        }

        for value in ["auto", "", "unexpected"] {
            let _streaming = EnvGuard::set(ENV_STREAMING, value);
            assert_eq!(
                streaming_enabled_from_env(),
                StreamingPolicy::Auto,
                "KIANA_STREAMING={value}"
            );
        }
    }

    #[tokio::test]
    async fn provider_env_unknown_name_fails_closed() {
        let _lock = env_lock();
        let _script = EnvGuard::remove(ENV_HARNESS_SCRIPT);
        let _provider = EnvGuard::set(ENV_PROVIDER, "definitely-unknown");

        let error = provider_from_env(LocalModelConfig::default())
            .err()
            .expect("unknown provider should return an error");
        assert_eq!(error, "unknown_provider:definitely-unknown");

        let client = from_config(LocalModelConfig::default());
        let error = client
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("hello")],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .unwrap_err();
        assert!(
            error.starts_with("model_unavailable: unknown_provider:definitely-unknown"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn harness_script_takes_precedence_over_provider_env() {
        let _lock = env_lock();
        let script = TempScript::new(r#"[{"text":"from cassette"}]"#);
        let _script = EnvGuard::set(ENV_HARNESS_SCRIPT, script.path().as_os_str());
        let _provider = EnvGuard::set(ENV_PROVIDER, "definitely-unknown");

        let client = from_config(LocalModelConfig::default());
        let output = client
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("hello")],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .expect("KIANA_HARNESS_SCRIPT should be selected before provider validation");

        assert_eq!(output.text, "from cassette");
        assert!(output.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn streaming_switch_off_uses_non_streaming_fallback() {
        let (client, stream_called) = tracking_client(true, StreamingPolicy::Off);
        let mut deltas = Vec::new();

        let output = client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                },
                &mut |delta| {
                    deltas.push(delta);
                    Ok(())
                },
            )
            .await
            .expect("switch off should use the non-streaming path");

        assert_eq!(output.text, "fallback text");
        assert_eq!(
            deltas,
            vec![ModelDelta::Text {
                text: "fallback text".to_owned()
            }]
        );
        assert!(
            !stream_called.load(Ordering::SeqCst),
            "native stream_message must not be called when KIANA_STREAMING is off"
        );
    }

    #[tokio::test]
    async fn streaming_auto_uses_non_streaming_fallback_for_non_native_provider() {
        let (client, stream_called) = tracking_client(false, StreamingPolicy::Auto);
        let mut deltas = Vec::new();

        let output = client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                },
                &mut |delta| {
                    deltas.push(delta);
                    Ok(())
                },
            )
            .await
            .expect("synthetic-only providers should use the non-streaming fallback");

        assert_eq!(output.text, "fallback text");
        assert_eq!(
            deltas,
            vec![ModelDelta::Text {
                text: "fallback text".to_owned()
            }]
        );
        assert!(
            !stream_called.load(Ordering::SeqCst),
            "synthetic providers must not be reported as native streaming"
        );
    }

    #[tokio::test]
    async fn streaming_on_fails_closed_for_non_native_provider() {
        let (client, stream_called) = tracking_client(false, StreamingPolicy::On);

        let error = client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                },
                &mut |_| Ok(()),
            )
            .await
            .expect_err("forced streaming must not silently fall back");

        assert_eq!(error, "unsupported_streaming");
        assert!(
            !stream_called.load(Ordering::SeqCst),
            "non-native providers must be rejected before stream_message is called"
        );
    }

    #[tokio::test]
    async fn streaming_env_on_rejects_fake_provider_and_auto_falls_back() {
        let _lock = env_lock();
        let _script = EnvGuard::remove(ENV_HARNESS_SCRIPT);
        let _provider = EnvGuard::set(ENV_PROVIDER, FAKE_PROVIDER_ID);
        let _fake_script = EnvGuard::set(
            "KIANA_FAKE_PROVIDER_SCRIPT",
            r#"[{"type":"assistant_text","text":"fallback text"}]"#,
        );
        let config = LocalModelConfig {
            model: Some(FAKE_TEXT_ONLY_MODEL_ID.to_owned()),
            ..LocalModelConfig::default()
        };

        let _streaming = EnvGuard::set(ENV_STREAMING, "on");
        let on_client = from_config(config.clone());
        let error = on_client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                },
                &mut |_| Ok(()),
            )
            .await
            .expect_err("KIANA_STREAMING=on must not silently fall back");
        assert_eq!(error, "unsupported_streaming");

        let _streaming = EnvGuard::set(ENV_STREAMING, "auto");
        let auto_client = from_config(config);
        let mut deltas = Vec::new();
        let output = auto_client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                },
                &mut |delta| {
                    deltas.push(delta);
                    Ok(())
                },
            )
            .await
            .expect("auto must fall back to the complete response");
        assert_eq!(output.text, "fallback text");
        assert_eq!(
            deltas,
            vec![ModelDelta::Text {
                text: "fallback text".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn streaming_retries_transient_connection_before_first_delta() {
        let event = |value: Value| serde_json::from_value::<StreamEvent>(value).unwrap();
        let attempts = Arc::new(AtomicU64::new(0));
        let client = ProviderModelClient {
            provider: Box::new(RetryingStreamProvider {
                attempts: Arc::clone(&attempts),
                events: vec![
                    event(json!({
                        "type": "message_start",
                        "message": {
                            "id": "msg_retry",
                            "model": "retry-model",
                            "role": "assistant",
                            "usage": {"input_tokens": 4, "output_tokens": 0}
                        }
                    })),
                    event(json!({
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": {"type": "text", "text": ""}
                    })),
                    event(json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {"type": "text_delta", "text": "retried stream"}
                    })),
                    event(json!({
                        "type": "message_delta",
                        "delta": {"stop_reason": "end_turn"},
                        "usage": {"input_tokens": 0, "output_tokens": 2}
                    })),
                    event(json!({"type": "message_stop"})),
                ],
            }),
            model: "retry-model".to_owned(),
            streaming_policy: StreamingPolicy::Auto,
        };
        let mut deltas = Vec::new();

        let output = client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                },
                &mut |delta| {
                    deltas.push(delta);
                    Ok(())
                },
            )
            .await
            .expect("transient stream connection failures should be retried");

        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(output.text, "retried stream");
        assert_eq!(
            deltas,
            vec![ModelDelta::Text {
                text: "retried stream".to_owned()
            }]
        );
    }

    #[tokio::test]
    async fn native_streaming_aggregates_input_json_delta_without_network() {
        let event = |value: Value| serde_json::from_value::<StreamEvent>(value).unwrap();
        let events = vec![
            event(json!({
                "type": "message_start",
                "message": {
                    "id": "msg_event",
                    "model": "event-model",
                    "role": "assistant",
                    "usage": {"input_tokens": 9, "output_tokens": 0}
                }
            })),
            event(json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            })),
            event(json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "streamed"}
            })),
            event(json!({
                "type": "content_block_start",
                "index": 1,
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_event",
                    "name": "shell",
                    "input": {}
                }
            })),
            event(json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": "{\"command\":"}
            })),
            event(json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": "\"pwd\"}"}
            })),
            event(json!({
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use"},
                "usage": {"input_tokens": 0, "output_tokens": 5}
            })),
            event(json!({"type": "message_stop"})),
        ];
        let client = ProviderModelClient {
            provider: Box::new(EventProvider { events }),
            model: "event-model".to_owned(),
            streaming_policy: StreamingPolicy::On,
        };
        let mut deltas = Vec::new();

        let output = client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("run pwd")],
                    tools: vec![json!({"name": "shell"})],
                    sandbox: "read-only".to_owned(),
                },
                &mut |delta| {
                    deltas.push(delta);
                    Ok(())
                },
            )
            .await
            .expect("decoded native events should aggregate");

        assert_eq!(output.text, "streamed");
        assert_eq!(
            output.tool_calls,
            vec![ModelToolCall {
                id: "toolu_event".to_owned(),
                name: "shell".to_owned(),
                arguments: json!({"command": "pwd"}),
            }]
        );
        assert_eq!(
            output.usage,
            Some(ModelUsage {
                input_tokens: 9,
                output_tokens: 5,
            })
        );
        assert_eq!(output.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(output.model_id.as_deref(), Some("event-model"));
        assert_eq!(
            deltas,
            vec![ModelDelta::Text {
                text: "streamed".to_owned()
            }]
        );
    }

    #[tokio::test]
    async fn native_streaming_invalid_tool_json_fails_closed() {
        let event = |value: Value| serde_json::from_value::<StreamEvent>(value).unwrap();
        let events = vec![
            event(json!({
                "type": "message_start",
                "message": {
                    "id": "msg_bad_tool",
                    "model": "event-model",
                    "role": "assistant",
                    "usage": {"input_tokens": 3, "output_tokens": 0}
                }
            })),
            event(json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_bad",
                    "name": "shell",
                    "input": {}
                }
            })),
            event(json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"command\":"}
            })),
            event(json!({
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use"},
                "usage": {"input_tokens": 0, "output_tokens": 2}
            })),
        ];
        let client = ProviderModelClient {
            provider: Box::new(EventProvider { events }),
            model: "event-model".to_owned(),
            streaming_policy: StreamingPolicy::On,
        };

        let error = client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("run shell")],
                    tools: vec![json!({"name": "shell"})],
                    sandbox: "read-only".to_owned(),
                },
                &mut |_| Ok(()),
            )
            .await
            .expect_err("invalid streamed tool JSON must fail closed");

        assert!(error.starts_with("invalid_stream_tool_input:0:"), "{error}");
    }

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
            streaming_policy: StreamingPolicy::Off,
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
        assert_eq!(first.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(first.model_id.as_deref(), Some("fake-model"));
        assert_eq!(
            first.usage,
            Some(ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
            })
        );

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
        assert_eq!(second.stop_reason.as_deref(), Some("end_turn"));
    }

    #[tokio::test]
    async fn anthropic_native_streaming_aggregates_output_without_network() {
        let stream_body = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_stream","model":"claude-stream-test","role":"assistant","usage":{"input_tokens":12,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello "}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"shell","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"ls\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":0,"output_tokens":7}}

event: message_stop
data: {"type":"message_stop"}

"#;
        let (base_url, mut request_rx, server) =
            start_mock_anthropic_stream_server(stream_body.to_owned()).await;
        let provider =
            AnthropicProvider::new("test-key".to_owned(), base_url, Duration::from_secs(5));
        let client = ProviderModelClient {
            provider: Box::new(provider),
            model: "claude-stream-test".to_owned(),
            streaming_policy: StreamingPolicy::On,
        };
        let mut deltas = Vec::new();

        let output = client
            .complete_streaming(
                ModelRequest {
                    messages: vec![ModelMessage::user("say hello")],
                    tools: vec![json!({
                        "name": "shell",
                        "input_schema": {"type": "object"}
                    })],
                    sandbox: "read-only".to_owned(),
                },
                &mut |delta| {
                    deltas.push(delta);
                    Ok(())
                },
            )
            .await
            .expect("native Anthropic SSE should aggregate");

        assert_eq!(output.text, "Hello world");
        assert_eq!(
            output.tool_calls,
            vec![ModelToolCall {
                id: "toolu_1".to_owned(),
                name: "shell".to_owned(),
                arguments: json!({"command": "ls"}),
            }]
        );
        assert_eq!(
            output.usage,
            Some(ModelUsage {
                input_tokens: 12,
                output_tokens: 7,
            })
        );
        assert_eq!(output.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(output.model_id.as_deref(), Some("claude-stream-test"));
        assert_eq!(
            deltas,
            vec![
                ModelDelta::Text {
                    text: "Hello ".to_owned()
                },
                ModelDelta::Text {
                    text: "world".to_owned()
                }
            ]
        );

        let request = request_rx
            .recv()
            .await
            .expect("mock server should receive the streaming request");
        assert!(request.starts_with("POST /v1/messages "));
        assert!(request.contains("\"stream\":true"));
        assert!(request.contains("\"name\":\"shell\""));
        server.await.unwrap();
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
            streaming_policy: StreamingPolicy::Off,
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

    #[tokio::test]
    async fn provider_retries_transient_error_without_network() {
        let provider = FakeProvider::new(
            "fake-model".to_owned(),
            vec![
                FakeProviderStep::ProviderError {
                    code: Some("rate_limit".to_owned()),
                    message: "slow down".to_owned(),
                },
                FakeProviderStep::FinalAnswer {
                    text: "recovered".to_owned(),
                },
            ],
        );
        let client = ProviderModelClient {
            provider: Box::new(provider),
            model: "fake-model".to_owned(),
            streaming_policy: StreamingPolicy::Off,
        };

        let output = client
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("hello")],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .expect("transient provider errors should be retried");

        assert_eq!(output.text, "recovered");
    }

    #[tokio::test]
    async fn provider_does_not_retry_non_transient_error_without_network() {
        let provider = FakeProvider::new(
            "fake-model".to_owned(),
            vec![
                FakeProviderStep::ProviderError {
                    code: Some("invalid_model".to_owned()),
                    message: "model does not exist".to_owned(),
                },
                FakeProviderStep::FinalAnswer {
                    text: "should not be consumed".to_owned(),
                },
            ],
        );
        let client = ProviderModelClient {
            provider: Box::new(provider),
            model: "fake-model".to_owned(),
            streaming_policy: StreamingPolicy::Off,
        };

        let error = client
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("hello")],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .expect_err("non-transient provider errors must not be retried");

        assert!(error.contains("invalid_model"), "{error}");
    }

    #[tokio::test]
    async fn provider_retries_rate_limit_then_succeeds() {
        let (base_url, mut request_rx, server) = start_mock_openai_compatible_server(vec![
            (
                429,
                json!({
                    "error": {
                        "code": "rate_limit",
                        "message": "slow down"
                    }
                }),
            ),
            (
                200,
                json!({
                    "id": "chatcmpl_retry",
                    "model": "gpt-test",
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "recovered"
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 11,
                        "completion_tokens": 3
                    }
                }),
            ),
        ])
        .await;
        let provider =
            OpenAiCompatibleProvider::new("test-key".to_owned(), base_url, Duration::from_secs(5))
                .unwrap();
        let client = ProviderModelClient {
            provider: Box::new(provider),
            model: "gpt-test".to_owned(),
            streaming_policy: StreamingPolicy::Off,
        };

        let output = client
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("hello")],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .expect("429 should be retried");

        assert_eq!(output.text, "recovered");
        assert_eq!(output.model_id.as_deref(), Some("gpt-test"));
        assert_eq!(output.stop_reason.as_deref(), Some("stop"));
        assert_eq!(
            output.usage,
            Some(ModelUsage {
                input_tokens: 11,
                output_tokens: 3,
            })
        );
        assert!(request_rx.recv().await.is_some());
        assert!(request_rx.recv().await.is_some());
        assert!(
            request_rx.try_recv().is_err(),
            "429 followed by success must use exactly two attempts"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn provider_does_not_retry_auth_errors() {
        let (base_url, mut request_rx, server) = start_mock_openai_compatible_server(vec![(
            401,
            json!({
                "error": {
                    "code": "invalid_api_key",
                    "message": "bad key"
                }
            }),
        )])
        .await;
        let provider =
            OpenAiCompatibleProvider::new("bad-key".to_owned(), base_url, Duration::from_secs(5))
                .unwrap();
        let client = ProviderModelClient {
            provider: Box::new(provider),
            model: "gpt-test".to_owned(),
            streaming_policy: StreamingPolicy::Off,
        };

        let error = client
            .complete(ModelRequest {
                messages: vec![ModelMessage::user("hello")],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            })
            .await
            .expect_err("401 must fail immediately");

        assert!(
            error.contains("provider authentication error"),
            "unexpected auth error: {error}"
        );
        assert!(error.contains("bad key"), "unexpected auth error: {error}");
        assert!(request_rx.recv().await.is_some());
        assert!(request_rx.try_recv().is_err(), "401 must not be retried");
        server.await.unwrap();
    }

    async fn start_mock_openai_compatible_server(
        responses: Vec<(u16, Value)>,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            for (status, response) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_http_request(&mut socket).await;
                request_tx.send(request).unwrap();
                let body = serde_json::to_string(&response).unwrap();
                let reason = match status {
                    200 => "OK",
                    401 => "Unauthorized",
                    429 => "Too Many Requests",
                    503 => "Service Unavailable",
                    _ => "Error",
                };
                let raw = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(raw.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{addr}"), request_rx, server)
    }

    async fn start_mock_anthropic_stream_server(
        stream_body: String,
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
            let raw = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                stream_body.len(),
                stream_body
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
}
