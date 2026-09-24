use kiana_domain::{
    json_digest, AttemptId, ModelCallSpec, ModelOutput, ModelProtocol, ModelPurpose, ModelReply,
    ModelRequest, ModelResponseFormat, ModelRoute, PreparedModelCall, RequestId, RunId,
    TokenBudget, UsageConfidence, UsageId,
};
use kiana_provider::normalize_provider_usage;
use serde_json::{json, Value};

fn prepared(protocol: ModelProtocol) -> PreparedModelCall {
    let request = ModelRequest {
        messages: Vec::new(),
        tools: Vec::new(),
        sandbox: "read-only".to_owned(),
    };
    let mut prepared = PreparedModelCall {
        schema: kiana_domain::MODEL_CALL_SCHEMA.to_owned(),
        spec: ModelCallSpec {
            call_id: RequestId::new(),
            attempt_id: RequestId::new(),
            model_attempt_id: None,
            step_id: None,
            step: 1,
            purpose: ModelPurpose::Task,
            assignment: None,
            response_format: ModelResponseFormat::Text,
            replay: Vec::new(),
            deadline_unix_ms: u64::MAX,
        },
        route: ModelRoute {
            provider_id: "provider-fixture".to_owned(),
            protocol,
            connection_id: "connection-fixture".to_owned(),
            model_id: "requested-model".to_owned(),
            profile: "fixture".to_owned(),
            configuration_revision: "config-v1".to_owned(),
            streaming: true,
        },
        request: request.clone(),
        wire_body: json!({"messages": []}),
        request_hash: String::new(),
        budget: TokenBudget::new(1, 0, 0, 16, 4096),
        tool_catalog_hash: kiana_domain::tool_catalog_hash(&request.tools),
        provider_account: None,
        credential_revision: None,
    };
    prepared.seal();
    prepared
}

fn normalize(protocol: ModelProtocol, response: Value) -> kiana_domain::NormalizedUsage {
    let prepared = prepared(protocol);
    normalize_provider_usage(
        &prepared,
        &response,
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        0,
    )
    .expect("normalized usage")
}

#[test]
fn all_supported_provider_shapes_map_to_one_neutral_vector() {
    let cases = [
        (
            ModelProtocol::AnthropicMessages,
            json!({
                "model": "served-claude",
                "usage": {"input_tokens": 4, "output_tokens": 3, "cache_read_input_tokens": 2}
            }),
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({
                "model": "served-openai",
                "usage": {
                    "prompt_tokens": 4,
                    "completion_tokens": 3,
                    "total_tokens": 7,
                    "prompt_tokens_details": {"cached_tokens": 2},
                    "completion_tokens_details": {"reasoning_tokens": 1}
                }
            }),
        ),
        (
            ModelProtocol::OpenAiResponses,
            json!({
                "model": "served-responses",
                "usage": {
                    "input_tokens": 4,
                    "output_tokens": 3,
                    "total_tokens": 7,
                    "input_tokens_details": {"cached_tokens": 2},
                    "output_tokens_details": {"reasoning_tokens": 1}
                }
            }),
        ),
        (
            ModelProtocol::OllamaChat,
            json!({"model": "served-ollama", "prompt_eval_count": 4, "eval_count": 3}),
        ),
        (
            ModelProtocol::GeminiInteractions,
            json!({
                "modelVersion": "served-gemini",
                "usageMetadata": {
                    "promptTokenCount": 4,
                    "candidatesTokenCount": 3,
                    "totalTokenCount": 7,
                    "cachedContentTokenCount": 2,
                    "thoughtsTokenCount": 1
                }
            }),
        ),
        (
            ModelProtocol::Legacy,
            json!({
                "model": "served-fake",
                "usage": {"input_tokens": 4, "output_tokens": 3, "total_tokens": 7}
            }),
        ),
    ];

    for (protocol, response) in cases {
        let usage = normalize(protocol, response);
        assert_eq!(usage.vector.input_tokens, Some(4));
        assert_eq!(usage.vector.output_tokens, Some(3));
        assert_eq!(usage.requested_model_id, "requested-model");
        assert!(usage.served_model_id.is_some());
        assert_eq!(usage.confidence, UsageConfidence::Known);
        usage.validate().expect("usage contract");
    }
}

#[test]
fn missing_usage_stays_unknown_and_partial_fields_never_become_zero() {
    let missing = normalize(
        ModelProtocol::OpenAiChat,
        json!({"model": "served", "choices": []}),
    );
    assert_eq!(missing.vector.input_tokens, None);
    assert_eq!(missing.vector.output_tokens, None);
    assert_eq!(missing.vector.cache_read_tokens, None);
    assert_eq!(missing.vector.cache_write_tokens, None);
    assert_eq!(missing.vector.reasoning_output_tokens, None);
    assert_eq!(missing.vector.audio_input_tokens, None);
    assert_eq!(missing.vector.audio_output_tokens, None);
    assert_eq!(missing.confidence, UsageConfidence::Unknown);

    let partial = normalize(
        ModelProtocol::AnthropicMessages,
        json!({"usage": {"input_tokens": 4}}),
    );
    assert_eq!(partial.vector.input_tokens, Some(4));
    assert_eq!(partial.vector.output_tokens, None);
    assert_eq!(partial.vector.cache_read_tokens, None);
    assert_eq!(partial.vector.cache_write_tokens, None);
    assert_eq!(partial.vector.reasoning_output_tokens, None);
    assert_eq!(partial.vector.audio_input_tokens, None);
    assert_eq!(partial.vector.audio_output_tokens, None);
    assert_eq!(partial.confidence, UsageConfidence::Partial);
}

#[test]
fn reported_total_without_components_stays_partial_without_zero_filling() {
    let usage = normalize(
        ModelProtocol::OpenAiChat,
        json!({"usage": {"total_tokens": 7}}),
    );
    assert_eq!(usage.vector.input_tokens, None);
    assert_eq!(usage.vector.output_tokens, None);
    assert_eq!(usage.vector.cache_read_tokens, None);
    assert_eq!(usage.vector.reasoning_output_tokens, None);
    assert_eq!(usage.confidence, UsageConfidence::Partial);
    assert_eq!(
        usage.unknown_reason,
        Some(kiana_domain::BillingUnknownReason::Partial)
    );
}

#[test]
fn malformed_or_inconsistent_provider_usage_is_rejected_before_settlement() {
    let prepared = prepared(ModelProtocol::OpenAiChat);
    for response in [
        json!({"usage": {"prompt_tokens": "4", "completion_tokens": 3}}),
        json!({"usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 8}}),
        json!({"usage": {"prompt_tokens": -1, "completion_tokens": 3}}),
    ] {
        assert!(
            normalize_provider_usage(
                &prepared,
                &response,
                UsageId::new(),
                AttemptId::new(),
                RunId::new(),
                0,
            )
            .is_err(),
            "malformed usage must fail closed"
        );
    }
}

#[test]
fn normalized_digest_is_over_safe_shape_and_does_not_authorize_effects() {
    let prepared = prepared(ModelProtocol::OpenAiChat);
    let response = json!({
        "model": "served",
        "usage": {"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3},
        "authorization": "fixture-secret"
    });
    let usage = normalize_provider_usage(
        &prepared,
        &response,
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        2,
    )
    .expect("normalized usage");
    let encoded = serde_json::to_string(&usage).expect("usage json");
    assert!(!encoded.contains("fixture-secret"));
    assert_eq!(usage.raw_digest, json_digest(&response));
    assert_eq!(usage.retry_ordinal, 2);
}

#[test]
fn legacy_reply_without_served_model_keeps_model_observation_unknown() {
    let prepared = prepared(ModelProtocol::Legacy);
    let reply = ModelReply::legacy(ModelOutput::text("ok")).expect("legacy reply");
    let usage = kiana_provider::normalize_model_reply(
        &prepared,
        &reply,
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
    )
    .expect("normalized legacy usage");
    assert_eq!(usage.requested_model_id, "requested-model");
    assert_eq!(usage.served_model_id, None);
    assert_eq!(usage.confidence, UsageConfidence::Unknown);
}
