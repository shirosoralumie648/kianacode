use kiana_domain::{
    ModelCallSpec, ModelMessage, ModelProtocol, ModelPurpose, ModelRequest, ModelResponseFormat,
    ModelRoute, PreparedModelCall, RequestId, TokenBudget,
};
use kiana_provider::replay_stream_fixture;

fn prepared() -> PreparedModelCall {
    let request = ModelRequest {
        messages: vec![ModelMessage::user("offline streaming fixture")],
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
            provider_id: "fixture-provider".to_owned(),
            protocol: ModelProtocol::OpenAiChat,
            connection_id: "fixture-connection".to_owned(),
            model_id: "fixture-model".to_owned(),
            profile: "fixture".to_owned(),
            configuration_revision: "fixture.v1".to_owned(),
            streaming: true,
        },
        request: request.clone(),
        wire_body: serde_json::json!({"messages": []}),
        request_hash: String::new(),
        budget: TokenBudget::new(1, 0, 0, 16, 4096),
        tool_catalog_hash: kiana_domain::tool_catalog_hash(&request.tools),
        provider_account: None,
        credential_revision: None,
    };
    prepared.seal();
    prepared
}

fn stream_payload() -> Vec<u8> {
    r#"data: {"id":"fixture-response","model":"fixture-model","choices":[{"index":0,"delta":{"content":"你"},"finish_reason":null}]}

data: {"id":"fixture-response","model":"fixture-model","choices":[{"index":0,"delta":{"content":"好"},"finish_reason":null}]}

data: {"id":"fixture-response","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]

"#
    .as_bytes()
    .to_vec()
}

fn chunks(payload: &[u8], width: usize) -> Vec<Vec<u8>> {
    payload.chunks(width).map(|chunk| chunk.to_vec()).collect()
}

#[test]
fn arbitrary_chunk_boundaries_preserve_final_output_and_deltas() {
    let payload = stream_payload();
    for width in [1, 2, 3, 7, 31, 127] {
        let (reply, deltas) = replay_stream_fixture(prepared(), &chunks(&payload, width), 2_048)
            .expect("offline stream replay");
        assert_eq!(reply.output.text, "你好");
        let text = deltas
            .iter()
            .filter_map(|delta| match delta {
                kiana_domain::ModelDelta::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "你好");
    }
}

#[test]
fn truncated_and_oversized_frames_fail_closed_without_retry_or_network() {
    let mut truncated = stream_payload();
    truncated.truncate(truncated.len() - 2);
    let error = replay_stream_fixture(prepared(), &chunks(&truncated, 5), 2_048)
        .expect_err("truncated stream");
    assert_eq!(error.code, "provider_frame_truncated");

    let error = replay_stream_fixture(prepared(), &chunks(&stream_payload(), 1), 8)
        .expect_err("oversized frame");
    assert_eq!(error.code, "provider_frame_limit");
}

#[test]
fn fixture_prepared_call_digest_is_stable_and_contains_no_secret_material() {
    let prepared = prepared();
    let digest = prepared.fingerprint();
    assert_eq!(digest, prepared.request_hash);
    assert_eq!(digest.len(), 71);
    let audit = serde_json::to_string(&prepared.audit()).expect("safe prepared audit");
    assert!(!audit.contains("wire_body"));
    assert!(!audit.contains("offline streaming fixture"));
}
