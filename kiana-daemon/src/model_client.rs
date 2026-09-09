//! Composition-root model injection for the owned harness.
//!
//! `kiana-runner` never depends on `kiana-services`. Scripted turns win via
//! `KIANA_HARNESS_SCRIPT`; otherwise daemon wraps a provider client; missing
//! credentials fail closed.

use async_trait::async_trait;
use kiana_domain::RoleSpec;
use kiana_runner::{
    ModelClient, ModelMessage, ModelOutput, ModelRequest, ModelRole, ModelToolCall, ModelUsage,
    ScriptedModel, UnavailableModel,
};
use kiana_services::api::errors::ApiErrorKind;
use kiana_services::api::messages::{Message, MessagesRequest, MessagesResponse};
use kiana_services::api::provider::{
    provider_registry_entry, AnthropicProvider, FakeProvider, OllamaProvider,
    OpenAiCompatibleProvider, Provider, ProviderError, ANTHROPIC_PROVIDER_ID, FAKE_PROVIDER_ID,
    OLLAMA_PROVIDER_ID, OPENAI_COMPATIBLE_PROVIDER_ID,
};
use kiana_services::api::retry::{with_retry, RetryConfig};
use kiana_services::errors::ServiceError;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

use crate::LocalModelConfig;

const ENV_HARNESS_SCRIPT: &str = "KIANA_HARNESS_SCRIPT";
const ENV_PROVIDER: &str = "KIANA_PROVIDER";
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
            RetryConfig {
                max_retries: MODEL_MAX_RETRIES,
                base_delay: MODEL_RETRY_BASE_DELAY,
            },
        )
        .await
        .map_err(model_error_from_service_error)?;
        Ok(output_from_response(&response))
    }
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
    use kiana_services::api::provider::{FakeProviderStep, FAKE_TEXT_ONLY_MODEL_ID};
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
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
