use super::client::AnthropicClient;
use super::errors::ApiError;
use super::messages::MessagesRequest;
use anyhow::Result;
use eventsource_stream::Eventsource;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageStart },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: Delta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDelta,
        usage: DeltaUsage,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "error")]
    Error { error: Value },
    #[serde(rename = "message_stop")]
    MessageStop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStart {
    pub id: String,
    pub model: String,
    pub role: String,
    pub usage: DeltaUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        #[serde(default)]
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },
    #[serde(rename = "tool_use")]
    ToolUse(ToolUse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDelta {
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
}

impl AnthropicClient {
    pub async fn stream_message(
        &self,
        mut request: MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        request.stream = Some(true);
        let url = format!("{}/v1/messages", self.base_url());

        let response = self
            .client()
            .post(&url)
            .headers(self.build_headers())
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(key, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (key.as_str().to_string(), value.to_string()))
                })
                .collect();
            let text = response.text().await?;
            let mut error =
                ApiError::new(ApiError::classify(Some(status), &text), text).with_status(status);
            error.headers = headers;
            return Err(error.into());
        }

        let stream = response
            .bytes_stream()
            .eventsource()
            .filter_map(|result| async move {
                match result {
                    Ok(event) => {
                        if event.data == "[DONE]" {
                            return None;
                        }
                        match serde_json::from_str::<StreamEvent>(&event.data) {
                            Ok(event) => Some(Ok(event)),
                            Err(e) => Some(Err(anyhow::anyhow!("Parse error: {}", e))),
                        }
                    }
                    Err(e) => Some(Err(anyhow::anyhow!("Stream error: {}", e))),
                }
            });

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::{ContentBlock, Delta, StreamEvent};
    use serde_json::json;

    #[test]
    fn parses_ping_and_error_stream_events() {
        let ping: StreamEvent = serde_json::from_value(json!({ "type": "ping" })).unwrap();
        assert!(matches!(ping, StreamEvent::Ping));

        let error: StreamEvent = serde_json::from_value(json!({
            "type": "error",
            "error": {
                "type": "overloaded_error",
                "message": "try again"
            }
        }))
        .unwrap();
        match error {
            StreamEvent::Error { error } => {
                assert_eq!(error["type"], "overloaded_error");
                assert_eq!(error["message"], "try again");
            }
            other => panic!("expected error event, got {other:?}"),
        }
    }

    #[test]
    fn parses_thinking_stream_events() {
        let start: StreamEvent = serde_json::from_value(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "thinking",
                "thinking": ""
            }
        }))
        .unwrap();
        match start {
            StreamEvent::ContentBlockStart {
                content_block:
                    ContentBlock::Thinking {
                        thinking,
                        signature,
                    },
                ..
            } => {
                assert_eq!(thinking, "");
                assert_eq!(signature, None);
            }
            other => panic!("expected thinking content block, got {other:?}"),
        }

        let thinking_delta: StreamEvent = serde_json::from_value(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {
                "type": "thinking_delta",
                "thinking": "consider"
            }
        }))
        .unwrap();
        match thinking_delta {
            StreamEvent::ContentBlockDelta {
                delta: Delta::ThinkingDelta { thinking },
                ..
            } => assert_eq!(thinking, "consider"),
            other => panic!("expected thinking delta, got {other:?}"),
        }

        let signature_delta: StreamEvent = serde_json::from_value(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {
                "type": "signature_delta",
                "signature": "sig"
            }
        }))
        .unwrap();
        match signature_delta {
            StreamEvent::ContentBlockDelta {
                delta: Delta::SignatureDelta { signature },
                ..
            } => assert_eq!(signature, "sig"),
            other => panic!("expected signature delta, got {other:?}"),
        }

        let redacted: StreamEvent = serde_json::from_value(json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "redacted_thinking",
                "data": "encrypted"
            }
        }))
        .unwrap();
        match redacted {
            StreamEvent::ContentBlockStart {
                content_block: ContentBlock::RedactedThinking { data },
                ..
            } => assert_eq!(data, "encrypted"),
            other => panic!("expected redacted thinking content block, got {other:?}"),
        }
    }
}
