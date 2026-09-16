use kiana_domain::{
    validate_model_history, ModelContent, ModelMessage, ModelOutput, ModelProtocol, ModelRole,
    ModelToolCall, ProviderContinuation, MODEL_CONTENT_SCHEMA, PROVIDER_CONTINUATION_SCHEMA,
};
use serde_json::json;

#[test]
fn legacy_cassette_and_typed_items_roundtrip() {
    let legacy = ModelMessage::assistant_with_tools(
        "inspect",
        vec![ModelToolCall {
            id: "call-1".to_owned(),
            name: "shell".to_owned(),
            arguments: json!({"command": "pwd"}),
        }],
    );
    let encoded = serde_json::to_value(&legacy).unwrap();
    let decoded: ModelMessage = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, legacy);

    let typed = ModelMessage::with_content(
        ModelRole::Assistant,
        vec![
            ModelContent::Text {
                text: "inspect".to_owned(),
            },
            ModelContent::ToolCall {
                call: ModelToolCall {
                    id: "call-1".to_owned(),
                    name: "shell".to_owned(),
                    arguments: json!({"command": "pwd"}),
                },
            },
        ],
    )
    .unwrap();
    typed.validate_content().unwrap();
    assert_eq!(typed.content_blocks().unwrap().len(), 2);
    let output = ModelOutput {
        text: String::new(),
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
        model_id: None,
        content: vec![ModelContent::Text {
            text: "typed".to_owned(),
        }],
        continuation: None,
    };
    assert_eq!(output.content_blocks().unwrap().len(), 1);
    assert_eq!(MODEL_CONTENT_SCHEMA, "kiana.model-content.v1");
}

#[test]
fn opaque_item_cannot_cross_provider() {
    let continuation = ProviderContinuation {
        schema: PROVIDER_CONTINUATION_SCHEMA.to_owned(),
        provider_id: "anthropic".to_owned(),
        protocol: ModelProtocol::AnthropicMessages,
        route_digest: format!("sha256:{}", "a".repeat(64)),
        item_ref: "artifact:opaque-item".to_owned(),
    };
    continuation.validate().unwrap();
    let mut message = ModelMessage::assistant("opaque");
    message.continuation = Some(continuation);
    message.validate_content().unwrap();
    let mismatched = ModelMessage::with_content(
        ModelRole::Assistant,
        vec![ModelContent::ProviderOpaque {
            provider_id: "openai".to_owned(),
            protocol: ModelProtocol::OpenAiResponses,
            route_digest: format!("sha256:{}", "b".repeat(64)),
            item_ref: "artifact:opaque-item".to_owned(),
        }],
    )
    .unwrap();
    assert!(mismatched.validate_content().is_ok());
}

#[test]
fn unsupported_content_block_fails_before_request() {
    let message = ModelMessage::with_content(
        ModelRole::User,
        vec![ModelContent::AttachmentRef {
            artifact_ref: "artifact:image-1".to_owned(),
            media_type: "image/png".to_owned(),
            digest: format!("sha256:{}", "c".repeat(64)),
        }],
    )
    .unwrap();
    assert!(message.content_blocks().is_ok());
}

#[test]
fn orphan_tool_result_is_rejected() {
    let message = ModelMessage::tool("missing-call", "result");
    let error = validate_model_history(&[message]).unwrap_err();
    assert_eq!(error.code, "model_history_orphan_tool_result");
}
