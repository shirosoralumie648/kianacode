use super::client::AnthropicClient;
use super::errors::{ApiError, ApiErrorKind};
use super::messages::{MessagesRequest, MessagesResponse, Usage};
use super::streaming::{
    ContentBlock as StreamContentBlock, Delta, DeltaUsage, MessageDelta, MessageStart, StreamEvent,
};
use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
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
    use super::*;

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
}
