use kiana_domain::{
    ModelCallSpec, ModelProtocol, ModelPurpose, ModelRequest, ModelResponseFormat, ModelRoute,
    PreparedModelCall, RequestId, TokenBudget,
};
use kiana_provider::safe_prepared_metadata;
use serde_json::json;

fn prepared() -> PreparedModelCall {
    let request = ModelRequest {
        messages: vec![kiana_domain::ModelMessage::user(
            "prompt-secret-must-never-be-in-telemetry",
        )],
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
            deadline_unix_ms: 9_999_999_999,
        },
        route: ModelRoute {
            provider_id: "provider".to_owned(),
            protocol: ModelProtocol::OpenAiChat,
            connection_id: "connection".to_owned(),
            model_id: "model".to_owned(),
            profile: "profile".to_owned(),
            configuration_revision: "revision".to_owned(),
            streaming: true,
        },
        request: request.clone(),
        wire_body: json!({
            "messages": [{"role": "user", "content": "prompt-secret-must-never-be-in-telemetry"}],
            "headers": {"authorization": "Bearer header-secret"},
            "raw_response": "response-secret",
        }),
        request_hash: String::new(),
        budget: TokenBudget::new(128, 0, 0, 16, 4096),
        tool_catalog_hash: kiana_domain::tool_catalog_hash(&[]),
        provider_account: None,
        credential_revision: None,
    };
    prepared.seal();
    prepared
}

#[test]
fn provider_summary_is_allowlisted_and_hashes_prompt_identity() {
    let summary = safe_prepared_metadata(&prepared()).unwrap();
    let encoded = serde_json::to_string(&summary).unwrap();
    for sentinel in [
        "prompt-secret-must-never-be-in-telemetry",
        "header-secret",
        "response-secret",
        "wire_body",
        "messages",
    ] {
        assert!(
            !encoded.contains(sentinel),
            "provider telemetry leaked {sentinel}: {encoded}"
        );
    }
    assert_eq!(summary["schema"], "kiana.model-attempt.v1");
    assert!(summary["route_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(summary["prompt_version"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
}
