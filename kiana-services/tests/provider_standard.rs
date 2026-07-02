use futures::StreamExt;
use kiana_services::api::messages::{Message, MessagesRequest};
use kiana_services::api::provider::{
    FakeProvider, FakeProviderStep, Provider, StreamingMode, FAKE_MODEL_ID, FAKE_PROVIDER_ID,
    FAKE_TEXT_ONLY_MODEL_ID,
};
use kiana_services::api::streaming::{ContentBlock, Delta, StreamEvent};
use serde_json::{json, Value};

fn request(model: &str, tools: Option<Vec<Value>>) -> MessagesRequest {
    MessagesRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: json!("hello"),
        }],
        max_tokens: 128,
        system: None,
        temperature: None,
        tools,
        thinking: None,
        stream: None,
    }
}

#[tokio::test]
async fn fake_provider_standard_contract_reports_metadata_and_text_response() {
    let provider = FakeProvider::new(
        FAKE_MODEL_ID.to_string(),
        vec![FakeProviderStep::AssistantText {
            text: "standard response".to_string(),
        }],
    );

    assert_eq!(provider.provider_id(), FAKE_PROVIDER_ID);
    let profile = provider.model_profile(FAKE_MODEL_ID);
    assert_eq!(profile.provider_id, FAKE_PROVIDER_ID);
    assert_eq!(profile.model_id, FAKE_MODEL_ID);
    assert!(profile.supports_tools);
    assert!(profile.supports_streaming);
    assert_eq!(profile.streaming_mode, StreamingMode::Synthetic);
    assert!(!profile.native_streaming);

    let response = provider
        .create_message(request(FAKE_MODEL_ID, None))
        .await
        .unwrap();

    assert_eq!(response.id, "fake-msg-1");
    assert_eq!(response.model, FAKE_MODEL_ID);
    assert_eq!(response.role, "assistant");
    assert_eq!(response.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(response.content[0]["type"], "text");
    assert_eq!(response.content[0]["text"], "standard response");
}

#[tokio::test]
async fn fake_provider_standard_contract_rejects_unsupported_tools_without_consuming_step() {
    let provider = FakeProvider::new(
        FAKE_TEXT_ONLY_MODEL_ID.to_string(),
        vec![FakeProviderStep::AssistantText {
            text: "still available".to_string(),
        }],
    );

    let error = provider
        .create_message(request(
            FAKE_TEXT_ONLY_MODEL_ID,
            Some(vec![json!({"name": "Read"})]),
        ))
        .await
        .unwrap_err();

    assert_eq!(error.code(), "unsupported_tools");
    assert!(error.to_string().contains("does not support tools"));

    let response = provider
        .create_message(request(FAKE_TEXT_ONLY_MODEL_ID, None))
        .await
        .unwrap();
    assert_eq!(response.id, "fake-msg-1");
    assert_eq!(response.content[0]["text"], "still available");
}

#[tokio::test]
async fn fake_provider_standard_contract_maps_tool_calls() {
    let provider = FakeProvider::new(
        FAKE_MODEL_ID.to_string(),
        vec![FakeProviderStep::ToolCall {
            id: Some("toolu_read".to_string()),
            name: "Read".to_string(),
            input: json!({"file_path": "src/lib.rs"}),
            text: Some("Need the file".to_string()),
        }],
    );

    let response = provider
        .create_message(request(FAKE_MODEL_ID, Some(vec![json!({"name": "Read"})])))
        .await
        .unwrap();

    assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(response.content[0]["type"], "text");
    assert_eq!(response.content[0]["text"], "Need the file");
    assert_eq!(response.content[1]["type"], "tool_use");
    assert_eq!(response.content[1]["id"], "toolu_read");
    assert_eq!(response.content[1]["name"], "Read");
    assert_eq!(response.content[1]["input"]["file_path"], "src/lib.rs");
}

#[tokio::test]
async fn fake_provider_standard_contract_streams_synthetic_events() {
    let provider = FakeProvider::new(
        FAKE_MODEL_ID.to_string(),
        vec![FakeProviderStep::FinalAnswer {
            text: "streamed answer".to_string(),
        }],
    );

    let mut stream = provider
        .stream_message(request(FAKE_MODEL_ID, None))
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.unwrap());
    }

    assert!(matches!(
        events.first(),
        Some(StreamEvent::MessageStart { message }) if message.model == FAKE_MODEL_ID
    ));
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::ContentBlockStart {
            content_block: ContentBlock::Text { .. },
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::ContentBlockDelta {
            delta: Delta::TextDelta { text },
            ..
        } if text == "streamed answer"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::MessageDelta { delta, .. }
            if delta.stop_reason.as_deref() == Some("end_turn")
    )));
    assert!(matches!(events.last(), Some(StreamEvent::MessageStop)));
}

#[tokio::test]
async fn fake_provider_standard_contract_surfaces_provider_error_codes() {
    let provider = FakeProvider::new(
        FAKE_MODEL_ID.to_string(),
        vec![FakeProviderStep::ProviderError {
            code: Some("quota_exceeded".to_string()),
            message: "quota exhausted".to_string(),
        }],
    );

    let error = provider
        .create_message(request(FAKE_MODEL_ID, None))
        .await
        .unwrap_err();

    assert_eq!(error.code(), "quota_exceeded");
    assert!(error.to_string().contains("quota exhausted"));
}
