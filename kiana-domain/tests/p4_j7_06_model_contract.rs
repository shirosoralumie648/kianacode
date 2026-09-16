use kiana_domain::{
    check_schema_compatibility, ModelContent, ModelError, ModelMessage, ModelRole, SchemaVersion,
    MODEL_CONTENT_SCHEMA,
};
use serde_json::json;

#[test]
fn ordered_text_tool_and_result_blocks_round_trip() {
    let tool = kiana_domain::ModelToolCall {
        id: "call-1".to_owned(),
        name: "memory.search".to_owned(),
        arguments: json!({"query":"latency"}),
    };
    let message = ModelMessage::with_content(
        ModelRole::Assistant,
        vec![
            ModelContent::Text {
                text: "I will inspect the evidence.".to_owned(),
            },
            ModelContent::ToolCall { call: tool.clone() },
        ],
    )
    .unwrap();
    let blocks = message.content_blocks().unwrap();
    assert!(matches!(blocks[0], ModelContent::Text { .. }));
    assert!(matches!(blocks[1], ModelContent::ToolCall { .. }));
    let decoded: ModelMessage =
        serde_json::from_value(serde_json::to_value(&message).unwrap()).unwrap();
    assert_eq!(decoded.content_blocks().unwrap(), blocks);
    assert_eq!(MODEL_CONTENT_SCHEMA, "kiana.model-content.v1");
}

#[test]
fn conflicting_legacy_and_block_content_is_rejected() {
    let mut value = serde_json::to_value(ModelMessage::assistant("legacy")).unwrap();
    value["content"] = json!([{"kind":"text","text":"different"}]);
    let message: ModelMessage = serde_json::from_value(value).unwrap();
    assert_eq!(
        message.validate_content().unwrap_err().code,
        "model_content_legacy_conflict"
    );
}

#[test]
fn model_contract_major_and_error_classification_fail_closed() {
    assert!(check_schema_compatibility(MODEL_CONTENT_SCHEMA, &SchemaVersion::new(1, 1)).is_ok());
    assert!(check_schema_compatibility(MODEL_CONTENT_SCHEMA, &SchemaVersion::new(2, 0)).is_err());
    let error = ModelError::transport(
        "provider_timeout",
        kiana_domain::ModelRetryClass::BeforeSend,
        false,
    );
    let outcome = error.outcome();
    outcome.validate().unwrap();
    assert_eq!(
        outcome.retry_class,
        kiana_domain::ModelRetryClass::BeforeSend
    );
    assert_eq!(
        outcome.side_effect_state,
        kiana_domain::ModelSideEffectState::None
    );
}
