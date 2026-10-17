//! P4-J7-20 protected replay codec fixtures.
//!
//! The provider captures the private continuation material of each dialect, keeps it out of
//! public output and receipts, and resolves it only for the exact connection/model/protocol
//! scope that produced it. Every deny path here is checked before the success path. The
//! byte-for-byte wire injection is exercised through the real request compiler in the crate's
//! `request` unit tests, which can build a resolved connection fixture.
use kiana_domain::{
    ModelCallSpec, ModelDelta, ModelMessage, ModelProtocol, ModelPurpose, ModelRequest,
    ModelResponseFormat, ModelRoute, ModelToolCall, PreparedModelCall, ProtectedReplayMaterial,
    ProtectedReplayRef, ProtectedReplayScope, ProtectedReplaySource, ProtectedReplayStore,
    ProviderContinuation, ReasoningReplayKind, RequestId, TokenBudget,
    PROVIDER_CONTINUATION_SCHEMA,
};
use kiana_provider::replay::{
    capture_from_value, plan_injection, strip_private_items, ProtectedReplayService,
    RingReplaySealer,
};
use kiana_provider::replay_stream_fixture_capturing;
use serde_json::{json, Value};

fn route(protocol: ModelProtocol, provider_id: &str, model_id: &str) -> ModelRoute {
    ModelRoute {
        provider_id: provider_id.to_owned(),
        protocol,
        connection_id: "connection-a".to_owned(),
        model_id: model_id.to_owned(),
        profile: "default".to_owned(),
        configuration_revision: "config.v1".to_owned(),
        streaming: true,
    }
}

fn anthropic_route() -> ModelRoute {
    route(
        ModelProtocol::AnthropicMessages,
        "anthropic",
        "claude-sonnet-fixture",
    )
}

fn scope(
    route: &ModelRoute,
    kind: ReasoningReplayKind,
    call_id: RequestId,
    expires_at_unix_ms: u64,
) -> ProtectedReplayScope {
    ProtectedReplayScope {
        connection_id: route.connection_id.clone(),
        protocol: kind.protocol(),
        model_id: route.model_id.clone(),
        route_digest: route.digest(),
        effort: Some("medium".to_owned()),
        prompt_digest: "sha256:prompt".to_owned(),
        tool_catalog_digest: "sha256:tools".to_owned(),
        data_revision: "data.v1".to_owned(),
        source_call_id: call_id,
        expires_at_unix_ms,
    }
}

/// Capture fixture material for one protocol and bind it to the fixture route.
fn material_for(
    route: &ModelRoute,
    response: &Value,
    call_id: RequestId,
    expires_at_unix_ms: u64,
) -> ProtectedReplayMaterial {
    let capture = capture_from_value(route, response)
        .expect("capture")
        .expect("private material");
    let kind = capture.kind();
    capture
        .material(scope(route, kind, call_id, expires_at_unix_ms))
        .expect("scoped material")
}

fn continuation_request(route: &ModelRoute, artifact_id: &str) -> ModelRequest {
    let mut assistant = ModelMessage::assistant("working");
    assistant.tool_calls = vec![ModelToolCall {
        id: "call-1".to_owned(),
        name: "shell".to_owned(),
        arguments: json!({"command": "pwd"}),
    }];
    assistant.continuation = Some(ProviderContinuation {
        schema: PROVIDER_CONTINUATION_SCHEMA.to_owned(),
        provider_id: route.provider_id.clone(),
        protocol: route.protocol,
        route_digest: route.digest(),
        item_ref: artifact_id.to_owned(),
    });
    ModelRequest {
        messages: vec![
            ModelMessage::user("continue the tool loop"),
            assistant,
            ModelMessage::tool("call-1", "{\"ok\":true}"),
        ],
        tools: Vec::new(),
        sandbox: "read-only".to_owned(),
    }
}

fn spec(call_id: RequestId, reference: ProtectedReplayRef) -> ModelCallSpec {
    ModelCallSpec {
        call_id,
        attempt_id: RequestId::new(),
        model_attempt_id: None,
        step_id: None,
        step: 2,
        purpose: ModelPurpose::Task,
        assignment: None,
        response_format: ModelResponseFormat::Text,
        replay: vec![reference],
        deadline_unix_ms: u64::MAX,
    }
}

const ANTHROPIC_RESPONSE: &str = r#"{
    "content": [
        {"type": "thinking", "thinking": "private chain", "signature": "sig-anthropic"},
        {"type": "text", "text": "working"},
        {"type": "tool_use", "id": "call-1", "name": "shell", "input": {"command": "pwd"}}
    ],
    "stop_reason": "tool_use"
}"#;

const CHAT_RESPONSE: &str = r#"{
    "choices": [
        {
            "message": {
                "role": "assistant",
                "reasoning_content": "deepseek-private-chain",
                "content": "working",
                "tool_calls": [
                    {"id": "call-1", "type": "function", "function": {"name": "shell", "arguments": "{\"command\":\"pwd\"}"}}
                ]
            }
        }
    ]
}"#;

const RESPONSES_RESPONSE: &str = r#"{
    "output": [
        {"type": "reasoning", "id": "rs-1", "encrypted_content": "enc-1"},
        {"type": "message", "content": [{"type": "output_text", "text": "working"}]},
        {"type": "function_call", "call_id": "call-1", "name": "shell", "arguments": "{\"command\":\"pwd\"}"}
    ],
    "status": "completed"
}"#;

const GEMINI_RESPONSE: &str = r#"{
    "id": "interaction-1",
    "status": "requires_action",
    "steps": [
        {"type": "thought", "signature": "sig-gemini", "content": [{"type": "text", "text": "private chain"}]},
        {"type": "model_output", "content": [{"type": "text", "text": "working"}]},
        {"type": "function_call", "id": "call-1", "name": "shell", "arguments": {"command": "pwd"}}
    ]
}"#;

#[test]
fn captured_reasoning_is_resolved_into_the_continuation_plan() {
    for (protocol, provider_id, model_id, response, expected) in [
        (
            ModelProtocol::AnthropicMessages,
            "anthropic",
            "claude-sonnet-fixture",
            ANTHROPIC_RESPONSE,
            "anthropic",
        ),
        (
            ModelProtocol::OpenAiChat,
            "deepseek",
            "deepseek-chat-fixture",
            CHAT_RESPONSE,
            "deepseek",
        ),
        (
            ModelProtocol::OpenAiResponses,
            "openai-responses",
            "gpt-fixture",
            RESPONSES_RESPONSE,
            "responses",
        ),
        (
            ModelProtocol::GeminiInteractions,
            "gemini",
            "gemini-fixture",
            GEMINI_RESPONSE,
            "gemini",
        ),
    ] {
        let route = route(protocol, provider_id, model_id);
        let call_id = RequestId::new();
        let response: Value = serde_json::from_str(response).expect("fixture response");
        let material = material_for(&route, &response, call_id, 60_000);
        let mut store = ProtectedReplayStore::default();
        let reference = store.insert("artifact-1", material).expect("insert");
        assert_eq!(
            reference.kind,
            Some(ReasoningReplayKind::for_protocol(protocol).expect("kind"))
        );
        let injection = plan_injection(
            &route,
            &continuation_request(&route, "artifact-1"),
            &spec(call_id, reference),
            &store,
            1_000,
        )
        .expect("declared material resumes");
        let items = injection.items_for(1);
        assert_eq!(items.len(), 1, "{expected}");
        match expected {
            "anthropic" => {
                assert_eq!(items[0]["type"], "thinking");
                assert_eq!(items[0]["signature"], "sig-anthropic");
                assert_eq!(items[0]["thinking"], "private chain");
            }
            "deepseek" => assert_eq!(items[0], "deepseek-private-chain"),
            "responses" => {
                assert_eq!(items[0]["type"], "reasoning");
                assert_eq!(items[0]["encrypted_content"], "enc-1");
                assert_eq!(items[0]["id"], "rs-1");
            }
            "gemini" => {
                assert_eq!(items[0]["type"], "thought");
                assert_eq!(items[0]["signature"], "sig-gemini");
            }
            other => panic!("unknown fixture {other}"),
        }
        assert!(injection.items_for(0).is_empty());
        assert!(injection.items_for(2).is_empty());
    }
}

#[test]
fn missing_reasoning_material_blocks_resume() {
    let route = anthropic_route();
    let call_id = RequestId::new();
    let request = continuation_request(&route, "artifact-1");

    // A continuation message without any declared reference can never be resumed.
    let response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    let undeclared = ModelCallSpec {
        replay: Vec::new(),
        ..spec(
            call_id,
            material_for(&route, &response, call_id, 60_000).reference("unused"),
        )
    };
    assert_eq!(
        plan_injection(
            &route,
            &request,
            &undeclared,
            &ProtectedReplayStore::default(),
            1_000
        )
        .unwrap_err()
        .code,
        "missing_reasoning_material_blocks_resume"
    );

    // A reference whose bytes are gone from the protected store blocks the same way.
    let mut store = ProtectedReplayStore::default();
    let material = material_for(&route, &response, call_id, 60_000);
    let reference = store.insert("artifact-1", material).expect("insert");
    assert!(store.delete("artifact-1"));
    assert_eq!(
        store
            .resolve_material(&reference, &route, 1_000)
            .unwrap_err()
            .code,
        "missing_reasoning_material_blocks_resume"
    );

    // A declared but unresolvable reference is missing, not silently skipped.
    assert_eq!(
        plan_injection(
            &route,
            &request,
            &spec(call_id, reference),
            &ProtectedReplayStore::default(),
            1_000,
        )
        .unwrap_err()
        .code,
        "missing_reasoning_material_blocks_resume"
    );
}

#[test]
fn replay_payload_deletion_invalidates_resume() {
    let route = anthropic_route();
    let call_id = RequestId::new();
    let response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    let material = material_for(&route, &response, call_id, 60_000);

    let mut store = ProtectedReplayStore::default();
    let reference = store.insert("artifact-1", material).expect("insert");
    plan_injection(
        &route,
        &continuation_request(&route, "artifact-1"),
        &spec(call_id, reference.clone()),
        &store,
        1_000,
    )
    .expect("resume works while the material exists");

    assert!(store.delete("artifact-1"));
    assert_eq!(
        plan_injection(
            &route,
            &continuation_request(&route, "artifact-1"),
            &spec(call_id, reference),
            &store,
            1_000,
        )
        .unwrap_err()
        .code,
        "missing_reasoning_material_blocks_resume"
    );
}

#[test]
fn expiry_and_revocation_fail_closed_before_dispatch() {
    let route = anthropic_route();
    let call_id = RequestId::new();
    let response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    let material = material_for(&route, &response, call_id, 5_000);
    let mut store = ProtectedReplayStore::default();
    let reference = store.insert("artifact-1", material).expect("insert");

    assert_eq!(
        plan_injection(
            &route,
            &continuation_request(&route, "artifact-1"),
            &spec(call_id, reference.clone()),
            &store,
            5_000,
        )
        .unwrap_err()
        .code,
        "protected_replay_expired"
    );
    assert_eq!(
        store
            .resolve_material(&reference, &route, 5_000)
            .unwrap_err()
            .code,
        "protected_replay_expired"
    );

    store
        .revoke("artifact-1", "data_governance_revocation")
        .expect("revoke");
    assert_eq!(
        store
            .resolve_material(&reference, &route, 1_000)
            .unwrap_err()
            .code,
        "replay_material_revoked"
    );
    assert_eq!(
        store.revocation_reason("artifact-1"),
        Some("data_governance_revocation")
    );
}

#[test]
fn replay_material_cannot_cross_connection_or_model_scope() {
    let route = anthropic_route();
    let call_id = RequestId::new();
    let response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    let material = material_for(&route, &response, call_id, 60_000);
    let mut service = ProtectedReplayService::new(&[12u8; 32]).expect("service");
    let reference = service.store("artifact-1", &material).expect("store");

    let mut foreign = route.clone();
    foreign.model_id = "claude-opus-fixture".to_owned();
    assert_eq!(
        service
            .resolve_material(&reference, &foreign, 1_000)
            .unwrap_err()
            .code,
        "replay_material_cannot_cross_connection_or_model_scope"
    );
    // The same reference bound to another connection is refused as well.
    let mut other_connection = route.clone();
    other_connection.connection_id = "connection-b".to_owned();
    assert_eq!(
        service
            .resolve_material(&reference, &other_connection, 1_000)
            .unwrap_err()
            .code,
        "replay_material_cannot_cross_connection_or_model_scope"
    );

    // A model switch invalidates the stored entry instead of leaving a stale resume path.
    // The count covers both the plaintext entry and its sealed envelope copy.
    assert_eq!(service.invalidate_route_change(&foreign), 2);
    assert_eq!(
        service
            .resolve_material(&reference, &route, 1_000)
            .unwrap_err()
            .code,
        "replay_material_revoked"
    );
    assert_eq!(
        service.revocation_reason("artifact-1"),
        Some("model_switch")
    );
}

#[test]
fn plan_injection_refuses_unbound_and_duplicate_references() {
    let route = anthropic_route();
    let call_id = RequestId::new();
    let response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    let material = material_for(&route, &response, call_id, 60_000);
    let mut store = ProtectedReplayStore::default();
    let reference = store.insert("artifact-1", material).expect("insert");
    // A second, resolvable artifact that the history never references.
    let other = store
        .insert(
            "artifact-2",
            material_for(&route, &response, call_id, 60_000),
        )
        .expect("insert");

    // The history message must carry the continuation reference of the same artifact.
    assert_eq!(
        plan_injection(
            &route,
            &continuation_request(&route, "artifact-1"),
            &spec(call_id, other),
            &store,
            1_000
        )
        .unwrap_err()
        .code,
        "replay_material_unbound_to_history"
    );

    // A history that references the same artifact twice cannot order the private items.
    let mut request = continuation_request(&route, "artifact-1");
    let mut duplicate = request.messages.clone();
    duplicate.insert(2, request.messages[1].clone());
    request.messages = duplicate;
    assert_eq!(
        plan_injection(
            &route,
            &request,
            &spec(call_id, reference.clone()),
            &store,
            1_000
        )
        .unwrap_err()
        .code,
        "protected_replay_duplicate_continuation"
    );

    // A continuation that names another provider/protocol never crosses into this route.
    let mut foreign = continuation_request(&route, "artifact-1");
    let continuation = foreign.messages[1]
        .continuation
        .as_mut()
        .expect("continuation");
    continuation.provider_id = "deepseek".to_owned();
    assert_eq!(
        plan_injection(&route, &foreign, &spec(call_id, reference), &store, 1_000)
            .unwrap_err()
            .code,
        "opaque_item_cannot_cross_provider"
    );
}

#[test]
fn reasoning_payload_never_enters_receipt_memory_or_logs() {
    let route = anthropic_route();
    let call_id = RequestId::new();
    let response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    let material = material_for(&route, &response, call_id, 60_000);
    let mut service = ProtectedReplayService::new(&[14u8; 32]).expect("service");
    let reference = service.store("artifact-1", &material).expect("store");

    let debug = format!("{material:?}");
    assert!(!debug.contains("private chain"));
    assert!(!debug.contains("sig-anthropic"));
    let reference_json = serde_json::to_string(&reference).expect("reference json");
    assert!(!reference_json.contains("private chain"));
    assert!(!reference_json.contains("sig-anthropic"));
    let summary_json =
        serde_json::to_string(&material.public_summary("artifact-1").expect("summary"))
            .expect("summary json");
    assert!(!summary_json.contains("private chain"));
    assert!(!summary_json.contains("sig-anthropic"));
    let envelope = service
        .sealed_envelope("artifact-1")
        .expect("sealed envelope");
    let envelope_json = serde_json::to_string(&envelope).expect("envelope json");
    assert!(!envelope_json.contains("private chain"));
    assert!(!envelope_json.contains("sig-anthropic"));
    let export = service.export_sealed().expect("export");
    assert!(!export.contains("private chain"));
    assert!(!export.contains("sig-anthropic"));

    // A digest-bound public summary is audit evidence, never continuation input.
    let summary = material.public_summary("artifact-1").expect("summary");
    assert_eq!(
        summary.resume().unwrap_err().code,
        "public_replay_summary_is_not_resume_material"
    );
    assert!(serde_json::to_string(&json!({"summary": summary}))
        .expect("receipt json")
        .contains("digest_only_not_lossless"));
}

#[test]
fn sealed_short_lived_artifact_survives_a_process_restart_but_not_tampering() {
    let route = anthropic_route();
    let call_id = RequestId::new();
    let response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    let material = material_for(&route, &response, call_id, 60_000);

    let mut first = ProtectedReplayService::new(&[15u8; 32]).expect("service");
    let reference = first.store("artifact-1", &material).expect("store");
    let export = first.export_sealed().expect("export");

    // A restarted process with the same caller key and the imported ciphertext resolves the
    // same bytes: the envelope is the durable form, the process store is only a cache.
    let mut restarted = ProtectedReplayService::new(&[15u8; 32]).expect("service");
    assert_eq!(restarted.import_sealed(&export).expect("import"), 1);
    assert_eq!(
        restarted
            .resolve_material(&reference, &route, 1_000)
            .expect("sealed resume")
            .bytes(),
        material.bytes()
    );

    // A different key can never open the envelope.
    let sealed = restarted
        .sealed_envelope("artifact-1")
        .expect("envelope")
        .clone();
    assert_eq!(
        RingReplaySealer::new(&[16u8; 32])
            .expect("sealer")
            .open(&sealed, &route, 1_000)
            .unwrap_err()
            .code,
        "replay_material_authentication_failed"
    );

    // Tampering with the exported ciphertext is rejected before decryption, and a re-hashed
    // tamper is rejected by the AEAD tag.
    let mut corrupted: Value = serde_json::from_str(&export).expect("export json");
    let mut flipped = corrupted[0]["sealed_bytes"]
        .as_str()
        .expect("sealed bytes")
        .to_owned();
    let mid = flipped.len() / 2;
    let replacement = if &flipped[mid..mid + 1] == "A" {
        "B"
    } else {
        "A"
    };
    flipped.replace_range(mid..mid + 1, replacement);
    corrupted[0]["sealed_bytes"] = json!(flipped.clone());
    assert!(restarted
        .import_sealed(&serde_json::to_string(&corrupted).expect("json"))
        .is_err());
    corrupted[0]["sealed_sha256"] = json!(kiana_domain::json_digest(&json!({"sealed": flipped})));
    let mut rehashed = ProtectedReplayService::new(&[15u8; 32]).expect("service");
    rehashed
        .import_sealed(&serde_json::to_string(&corrupted).expect("json"))
        .expect("import");
    assert_eq!(
        rehashed
            .resolve_material(&reference, &route, 1_000)
            .unwrap_err()
            .code,
        "replay_material_authentication_failed"
    );

    // Short-lived: the same envelope is refused after its TTL.
    assert_eq!(
        restarted
            .resolve_material(&reference, &route, 60_000)
            .unwrap_err()
            .code,
        "protected_replay_expired"
    );
}

#[test]
fn protected_replay_kind_must_match_the_route_protocol() {
    let route = anthropic_route();
    let call_id = RequestId::new();
    let response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    let capture = capture_from_value(&route, &response)
        .expect("capture")
        .expect("private material");
    // Gemini thought material cannot be bound to an Anthropic scope.
    assert_eq!(
        capture
            .clone()
            .material(scope(
                &route,
                ReasoningReplayKind::GeminiThoughtSignature,
                call_id,
                60_000,
            ))
            .unwrap_err()
            .code,
        "protected_replay_kind_protocol_mismatch"
    );

    // And a captured reference cannot be replayed into another dialect route.
    let mut foreign_route = route.clone();
    foreign_route.protocol = ModelProtocol::OpenAiChat;
    let kind = capture.kind();
    let material = capture
        .material(scope(&route, kind, call_id, 60_000))
        .expect("material");
    let mut service = ProtectedReplayService::new(&[17u8; 32]).expect("service");
    let reference = service.store("artifact-1", &material).expect("store");
    assert_eq!(
        service
            .resolve_material(&reference, &foreign_route, 1_000)
            .unwrap_err()
            .code,
        "replay_material_cannot_cross_connection_or_model_scope"
    );
    // The route protocol must have a private dialect at all.
    let mut ollama = foreign_route.clone();
    ollama.protocol = ModelProtocol::OllamaChat;
    assert_eq!(
        plan_injection(
            &ollama,
            &continuation_request(&ollama, "artifact-1"),
            &spec(call_id, reference.clone()),
            &service,
            1_000,
        )
        .unwrap_err()
        .code,
        "protected_replay_protocol_unsupported"
    );
}

#[test]
fn strip_private_items_removes_only_the_private_dialect() {
    let route = route(
        ModelProtocol::GeminiInteractions,
        "gemini",
        "gemini-fixture",
    );
    let mut response: Value = serde_json::from_str(GEMINI_RESPONSE).expect("fixture response");
    strip_private_items(&route, &mut response);
    let steps = response["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["type"], "model_output");
    assert_eq!(steps[1]["type"], "function_call");

    let route = anthropic_route();
    let mut response: Value = serde_json::from_str(ANTHROPIC_RESPONSE).expect("fixture response");
    strip_private_items(&route, &mut response);
    let content = response["content"].as_array().expect("content");
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "tool_use");
}

#[test]
fn capture_has_no_material_for_public_only_responses() {
    let route = anthropic_route();
    let response = json!({
        "content": [{"type": "text", "text": "public only"}],
        "stop_reason": "end_turn"
    });
    assert!(capture_from_value(&route, &response)
        .expect("capture")
        .is_none());

    // A protocol without a private dialect never reports capture.
    let ollama = route(ModelProtocol::OllamaChat, "ollama", "qwen-fixture");
    assert!(
        capture_from_value(&ollama, &json!({"message": {"content": "hi"}}))
            .expect("capture")
            .is_none()
    );
}

fn chunks(payload: &str, width: usize) -> Vec<Vec<u8>> {
    payload
        .as_bytes()
        .chunks(width)
        .map(|chunk| chunk.to_vec())
        .collect()
}

fn text_deltas(deltas: &[ModelDelta]) -> String {
    deltas
        .iter()
        .filter_map(|delta| match delta {
            ModelDelta::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// A prepared call whose route is fixed, so the fixture-only replay boundary can be used
/// without a gateway, credential or permit.
fn prepared_call(protocol: ModelProtocol, provider_id: &str, model_id: &str) -> PreparedModelCall {
    let request = ModelRequest {
        messages: vec![ModelMessage::user("protected replay streaming fixture")],
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
        route: route(protocol, provider_id, model_id),
        request: request.clone(),
        wire_body: json!({"model": model_id, "stream": true, "messages": []}),
        request_hash: String::new(),
        budget: TokenBudget::new(1, 0, 0, 16, 4_096),
        tool_catalog_hash: kiana_domain::tool_catalog_hash(&request.tools),
        provider_account: None,
        credential_revision: None,
    };
    prepared.seal();
    prepared
}

const ANTHROPIC_THINKING_STREAM: &str = concat!(
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\",\"model\":\"claude-fixture\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"private chain\"}}\n\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-anthropic\"}}\n\n",
    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"working\"}}\n\n",
    "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

const CHAT_REASONING_STREAM: &str = concat!(
    "data: {\"id\":\"chat-1\",\"model\":\"deepseek-fixture\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"reasoning_content\":\"deepseek-private-chain\"}}]}\n\n",
    "data: {\"id\":\"chat-1\",\"model\":\"deepseek-fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"working\"}}]}\n\n",
    "data: {\"id\":\"chat-1\",\"model\":\"deepseek-fixture\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\n",
    "data: [DONE]\n\n",
);

const GEMINI_THOUGHT_STREAM: &str = concat!(
    "data: {\"event_type\":\"interaction.created\",\"interaction\":{\"id\":\"interaction-1\",\"status\":\"in_progress\",\"model\":\"gemini-fixture\"}}\n\n",
    "data: {\"event_type\":\"step.start\",\"index\":0,\"step\":{\"type\":\"thought\",\"content\":[{\"type\":\"text\",\"text\":\"private chain\"}]}}\n\n",
    "data: {\"event_type\":\"step.delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-gemini\"}}\n\n",
    "data: {\"event_type\":\"step.stop\",\"index\":0}\n\n",
    "data: {\"event_type\":\"step.start\",\"index\":1,\"step\":{\"type\":\"model_output\",\"content\":[{\"type\":\"text\",\"text\":\"working\"}]}}\n\n",
    "data: {\"event_type\":\"step.stop\",\"index\":1}\n\n",
    "data: {\"event_type\":\"interaction.completed\",\"interaction\":{\"id\":\"interaction-1\",\"status\":\"completed\",\"usage\":{\"total_input_tokens\":5,\"total_output_tokens\":2}}}\n\n",
);

#[test]
fn streaming_capture_moves_signed_thinking_into_protected_material() {
    let mut prepared = prepared_call(
        ModelProtocol::AnthropicMessages,
        "anthropic",
        "claude-fixture",
    );
    let call_id = prepared.spec.call_id;
    let route = prepared.route.clone();
    // Any byte split must produce the same captured material.
    let (reply, deltas, capture) = replay_stream_fixture_capturing(
        prepared.clone(),
        &chunks(ANTHROPIC_THINKING_STREAM, 7),
        64 * 1024,
    )
    .expect("signed thinking stream");
    assert_eq!(reply.output.text, "working");
    assert!(reply.output.tool_calls.is_empty());
    assert_eq!(text_deltas(&deltas), "working");
    let capture = capture.expect("private material captured");
    assert_eq!(
        capture.kind(),
        ReasoningReplayKind::AnthropicThinkingSignature
    );
    assert_eq!(capture.items().len(), 1);
    assert_eq!(capture.items()[0]["thinking"], "private chain");
    assert_eq!(capture.items()[0]["signature"], "sig-anthropic");

    // The bytes leave the provider only through the protected adapter, and the public summary
    // stays digest-only.
    let material = capture
        .clone()
        .material(scope(&route, capture.kind(), call_id, 60_000))
        .expect("scoped material");
    let summary_json =
        serde_json::to_string(&material.public_summary("artifact-1").expect("summary"))
            .expect("summary json");
    assert!(!summary_json.contains("private chain"));
    assert!(!summary_json.contains("sig-anthropic"));
    let mut service = ProtectedReplayService::new(&[22u8; 32]).expect("service");
    let reference = service.store("artifact-1", &material).expect("store");
    assert_eq!(reference.kind, Some(capture.kind()));

    // The continuation of the same turn replays the exact captured item.
    let injection = plan_injection(
        &route,
        &continuation_request(&route, "artifact-1"),
        &spec(call_id, reference.clone()),
        &service,
        1_000,
    )
    .expect("captured signature resumes the tool continuation");
    assert_eq!(injection.items_for(1), capture.items());
    // Byte-for-byte: the resolved material is exactly what the stream produced.
    assert_eq!(
        service
            .resolve_material(&reference, &route, 1_000)
            .expect("resolve")
            .bytes(),
        capture.encode().expect("encode")
    );

    // A disabled capture keeps the fail-closed behaviour on the same stream.
    assert_eq!(
        kiana_provider::replay_stream_fixture(
            prepared_call(
                ModelProtocol::AnthropicMessages,
                "anthropic",
                "claude-fixture"
            ),
            &chunks(ANTHROPIC_THINKING_STREAM, 11),
            64 * 1024,
        )
        .unwrap_err()
        .code,
        "provider_private_replay_not_configured"
    );
}

#[test]
fn streaming_chat_reasoning_content_is_captured_but_never_emitted_as_delta() {
    let prepared = prepared_call(
        ModelProtocol::OpenAiChat,
        "deepseek",
        "deepseek-chat-fixture",
    );
    let call_id = prepared.spec.call_id;
    let route = prepared.route.clone();
    let (reply, deltas, capture) =
        replay_stream_fixture_capturing(prepared, &chunks(CHAT_REASONING_STREAM, 5), 64 * 1024)
            .expect("reasoning_content stream");
    assert_eq!(reply.output.text, "working");
    assert_eq!(text_deltas(&deltas), "working");
    let capture = capture.expect("private material captured");
    assert_eq!(
        capture.kind(),
        ReasoningReplayKind::DeepSeekReasoningContent
    );
    assert_eq!(capture.items().len(), 1);
    assert_eq!(
        capture.items()[0],
        Value::String("deepseek-private-chain".to_owned())
    );

    let material = capture
        .clone()
        .material(scope(&route, capture.kind(), call_id, 60_000))
        .expect("scoped material");
    let mut service = ProtectedReplayService::new(&[23u8; 32]).expect("service");
    let reference = service.store("artifact-1", &material).expect("store");
    let injection = plan_injection(
        &route,
        &continuation_request(&route, "artifact-1"),
        &spec(call_id, reference),
        &service,
        1_000,
    )
    .expect("reasoning_content resumes the tool continuation");
    assert_eq!(injection.items_for(1), capture.items());
}

#[test]
fn streaming_gemini_thought_signature_is_captured_without_public_leak() {
    let prepared = prepared_call(
        ModelProtocol::GeminiInteractions,
        "gemini",
        "gemini-fixture",
    );
    let call_id = prepared.spec.call_id;
    let route = prepared.route.clone();
    let (reply, deltas, capture) =
        replay_stream_fixture_capturing(prepared, &chunks(GEMINI_THOUGHT_STREAM, 9), 64 * 1024)
            .expect("thought step stream");
    assert_eq!(reply.output.text, "working");
    // A completed interaction with no function step is a normal end_turn.
    assert_eq!(reply.finish, kiana_domain::ModelFinish::EndTurn);
    assert_eq!(text_deltas(&deltas), "working");
    let capture = capture.expect("private material captured");
    assert_eq!(capture.kind(), ReasoningReplayKind::GeminiThoughtSignature);
    assert_eq!(capture.items()[0]["type"], "thought");
    assert_eq!(capture.items()[0]["signature"], "sig-gemini");

    let material = capture
        .clone()
        .material(scope(&route, capture.kind(), call_id, 60_000))
        .expect("scoped material");
    let mut service = ProtectedReplayService::new(&[24u8; 32]).expect("service");
    let reference = service.store("artifact-1", &material).expect("store");
    let injection = plan_injection(
        &route,
        &continuation_request(&route, "artifact-1"),
        &spec(call_id, reference),
        &service,
        1_000,
    )
    .expect("thought signature resumes the tool continuation");
    assert_eq!(injection.items_for(1), capture.items());
}

#[test]
fn incomplete_private_signature_never_becomes_replay_material() {
    // The thinking block closes without a signature: it must not be stored as if it were
    // replayable, and it must not reach the public reply either.
    let payload = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\",\"model\":\"claude-fixture\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"private chain\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    );
    assert_eq!(
        replay_stream_fixture_capturing(
            prepared_call(
                ModelProtocol::AnthropicMessages,
                "anthropic",
                "claude-fixture"
            ),
            &chunks(payload, 13),
            64 * 1024,
        )
        .unwrap_err()
        .code,
        "provider_private_replay_incomplete"
    );

    // A public-only stream stays public with capture enabled and reports no material.
    let payload = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-2\",\"model\":\"claude-fixture\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"working\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let (reply, deltas, capture) = replay_stream_fixture_capturing(
        prepared_call(
            ModelProtocol::AnthropicMessages,
            "anthropic",
            "claude-fixture",
        ),
        &chunks(payload, 23),
        64 * 1024,
    )
    .expect("public-only stream");
    assert_eq!(reply.output.text, "working");
    assert_eq!(text_deltas(&deltas), "working");
    assert!(capture.is_none());
}
