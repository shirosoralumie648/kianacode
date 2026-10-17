//! BQ-10：各协议原始回包 → 同一个中立用量向量的映射与拒绝。
//!
//! 本夹具只证明源码层行为（本地未运行；回执由 CI 产生）。它绑定三件事：
//! 1. Anthropic / OpenAI Chat / OpenAI Responses / Ollama / Gemini / Fake(Legacy) 的字段
//!    抽取进同一组中立维度，且 `basis` 记录抽取规则与方言；
//! 2. 缺失、malformed、总数矛盾、包含关系矛盾、身份混淆、自报授权字段全部先拒绝；
//! 3. 每个协议声明“已含 / 独立 / 未声明”的字段包含关系与适配器抽取一致，
//!    同一 token 不会被两个维度各记一次。

use kiana_domain::{
    json_digest, AttemptId, BillingUnknownReason, ModelAssignment, ModelCallSpec, ModelOutput,
    ModelProtocol, ModelPurpose, ModelReply, ModelRequest, ModelResponseFormat, ModelRoute,
    PreparedModelCall, RequestId, RoleSpec, RunId, SchemaVersion, TokenBudget, TurnId,
    UsageConfidence, UsageId,
};
use kiana_provider::{normalize_provider_usage, provider_usage_containment};
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

/// 服务端安装的模型分配；run/turn 身份只能来自这里，不能来自回包。
fn assignment() -> ModelAssignment {
    let role = RoleSpec::pm();
    let assignment = ModelAssignment {
        schema: "kiana.model-assignment.v1".to_owned(),
        run_id: RunId::new(),
        turn_id: TurnId::new(),
        role_id: role.role_id.clone(),
        role_version: Some(role.version),
        catalog_version: Some(SchemaVersion::new(1, 0)),
        prompt_hash: Some(role.prompt_hash.clone()),
        input_schema: Some(role.input_schema.clone()),
        output_schema: Some(role.output_schema.clone()),
        profile: role.model_profile.clone(),
        project_root: "/repo".to_owned(),
        project_trusted: true,
        authority_revision: None,
        max_wall_time_ms: 10_000,
        runtime_budget: None,
    };
    assignment.validate().expect("server model assignment");
    assignment
}

fn normalize(protocol: ModelProtocol, response: Value) -> kiana_domain::NormalizedUsage {
    normalize_provider_usage(
        &prepared(protocol),
        &response,
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        0,
    )
    .expect("normalized usage")
}

fn reject(protocol: ModelProtocol, response: Value) -> String {
    normalize_provider_usage(
        &prepared(protocol),
        &response,
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        0,
    )
    .expect_err("malformed provider usage must fail closed")
}

#[test]
fn all_supported_provider_shapes_map_to_one_neutral_vector() {
    let cases = [
        (
            ModelProtocol::AnthropicMessages,
            "anthropic_messages",
            json!({
                "model": "served-claude",
                "usage": {"input_tokens": 4, "output_tokens": 3, "cache_read_input_tokens": 2}
            }),
        ),
        (
            ModelProtocol::OpenAiChat,
            "openai_chat",
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
            "openai_responses",
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
            "ollama_chat",
            json!({"model": "served-ollama", "prompt_eval_count": 4, "eval_count": 3}),
        ),
        (
            ModelProtocol::GeminiInteractions,
            "gemini_interactions",
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
            "fake",
            json!({
                "model": "served-fake",
                "usage": {"input_tokens": 4, "output_tokens": 3, "total_tokens": 7}
            }),
        ),
    ];

    for (protocol, dialect, response) in cases {
        let usage = normalize(protocol, response);
        assert_eq!(usage.vector.input_tokens, Some(4), "{dialect}");
        assert_eq!(usage.vector.output_tokens, Some(3), "{dialect}");
        assert_eq!(usage.requested_model_id, "requested-model", "{dialect}");
        assert!(usage.served_model_id.is_some(), "{dialect}");
        assert_eq!(usage.confidence, UsageConfidence::Known, "{dialect}");
        // basis 必须记录“哪套规则、哪个方言抽出的数字”，历史账单才能被解释。
        assert_eq!(
            usage.basis,
            format!("kiana.provider-usage-adapter.v1:{dialect}")
        );
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
    assert_eq!(
        missing.unknown_reason,
        Some(BillingUnknownReason::ProviderUnreported)
    );

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
    let long_model = "x".repeat(250);
    let too_long_model = "y".repeat(257);
    let cases = [
        // 回包根节点 / 用量对象 / 嵌套细节对象的形状错误。
        (
            ModelProtocol::OpenAiChat,
            json!([1, 2]),
            "provider_usage_response_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"usage": "complete"}),
            "provider_usage_usage_invalid",
        ),
        (
            ModelProtocol::GeminiInteractions,
            json!({"usageMetadata": []}),
            "provider_usage_usage_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"usage": {"prompt_tokens": 4, "prompt_tokens_details": []}}),
            "provider_usage_prompt_tokens_details_invalid",
        ),
        (
            ModelProtocol::OpenAiResponses,
            json!({"usage": {"input_tokens": 4, "input_tokens_details": "cache"}}),
            "provider_usage_input_tokens_details_invalid",
        ),
        // 类型错误 / 负数 / 浮点 / 超上限：绝不静默跳过或截断。
        (
            ModelProtocol::OpenAiChat,
            json!({"usage": {"prompt_tokens": "4", "completion_tokens": 3}}),
            "provider_usage_input_tokens_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"usage": {"prompt_tokens": -1, "completion_tokens": 3}}),
            "provider_usage_input_tokens_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"usage": {"prompt_tokens": 1.5, "completion_tokens": 3}}),
            "provider_usage_input_tokens_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"usage": {"prompt_tokens": kiana_provider::MAX_PROVIDER_USAGE_TOKENS + 1}}),
            "provider_usage_input_tokens_invalid",
        ),
        (
            ModelProtocol::AnthropicMessages,
            json!({"usage": {"input_tokens": "4"}}),
            "provider_usage_input_tokens_invalid",
        ),
        (
            ModelProtocol::OllamaChat,
            json!({"prompt_eval_count": -1}),
            "provider_usage_input_tokens_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"usage": {
                "prompt_tokens": 4,
                "completion_tokens": 3,
                "completion_tokens_details": {"reasoning_tokens": "2"}
            }}),
            "provider_usage_reasoning_tokens_invalid",
        ),
        // served model 是外部自由文本，必须是有界、无控制字符的短字符串。
        (
            ModelProtocol::OpenAiChat,
            json!({"model": 5, "usage": {"prompt_tokens": 4, "completion_tokens": 3}}),
            "provider_usage_served_model_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"model": "   ", "usage": {"prompt_tokens": 4, "completion_tokens": 3}}),
            "provider_usage_served_model_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"model": "line\nbreak", "usage": {"prompt_tokens": 4, "completion_tokens": 3}}),
            "provider_usage_served_model_invalid",
        ),
        (
            ModelProtocol::OpenAiChat,
            json!({"model": too_long_model, "usage": {"prompt_tokens": 4, "completion_tokens": 3}}),
            "provider_usage_served_model_invalid",
        ),
    ];
    for (protocol, response, expected) in cases {
        assert_eq!(reject(protocol, response), expected);
    }

    // 恰好等于上限的计数值与 250 字符的模型名仍然合法（上限是拒绝线，不是截断线）。
    let at_bound = normalize(
        ModelProtocol::OpenAiChat,
        json!({
            "model": long_model.clone(),
            "usage": {
                "prompt_tokens": kiana_provider::MAX_PROVIDER_USAGE_TOKENS,
                "completion_tokens": 0,
                "total_tokens": kiana_provider::MAX_PROVIDER_USAGE_TOKENS
            }
        }),
    );
    assert_eq!(
        at_bound.vector.input_tokens,
        Some(kiana_provider::MAX_PROVIDER_USAGE_TOKENS)
    );
    assert_eq!(
        at_bound.served_model_id.as_deref(),
        Some(long_model.as_str())
    );
}

#[test]
fn reported_total_contradictions_are_rejected() {
    for response in [
        json!({"usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 8}}),
        json!({"usage": {"prompt_tokens": 4, "total_tokens": 2}}),
        json!({"usage": {"completion_tokens": 3, "total_tokens": 2}}),
    ] {
        assert_eq!(
            reject(ModelProtocol::OpenAiChat, response),
            "provider_usage_total_inconsistent"
        );
    }

    // 分项与总数一致时通过；只有一侧分项且不超过总数时保留 Partial（不补零）。
    let consistent = normalize(
        ModelProtocol::OpenAiChat,
        json!({"usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7}}),
    );
    assert_eq!(consistent.confidence, UsageConfidence::Known);
    let one_sided = normalize(
        ModelProtocol::OpenAiChat,
        json!({"usage": {"prompt_tokens": 4, "total_tokens": 7}}),
    );
    assert_eq!(one_sided.vector.output_tokens, None);
    assert_eq!(one_sided.confidence, UsageConfidence::Partial);
}

#[test]
fn requested_and_served_model_are_separate_observations_and_the_response_cannot_rewrite_identity() {
    let prepared = prepared(ModelProtocol::OpenAiChat);
    let usage_id = UsageId::new();
    let attempt_id = AttemptId::new();
    let run_id = RunId::new();
    let response = json!({
        "model": "served-downgraded",
        "requested_model": "spoofed-requested",
        "requested_model_id": "spoofed-requested-id",
        "run_id": "spoofed-run",
        "attempt_id": "spoofed-attempt",
        "usage_id": "spoofed-usage",
        "provider_id": "spoofed-provider",
        "route_id": "spoofed-route",
        "connection_id": "spoofed-connection",
        "usage": {"prompt_tokens": 4, "completion_tokens": 3}
    });
    let usage = normalize_provider_usage(&prepared, &response, usage_id, attempt_id, run_id, 0)
        .expect("normalized usage");

    // 请求模型/路由/身份全部来自服务端快照，回包只能提供 “served” 观测。
    assert_eq!(usage.requested_model_id, "requested-model");
    assert_eq!(usage.served_model_id.as_deref(), Some("served-downgraded"));
    assert_ne!(usage.served_model_id.as_deref(), Some("requested-model"));
    assert_eq!(usage.provider_id.as_deref(), Some("provider-fixture"));
    assert_eq!(usage.route_id, "connection-fixture");
    assert_eq!(usage.usage_id, usage_id);
    assert_eq!(usage.attempt_id, attempt_id);
    assert_eq!(usage.run_id, run_id);

    let encoded = serde_json::to_string(&usage).expect("usage json");
    assert!(
        !encoded.contains("spoofed"),
        "provider回包不得写入身份字段: {encoded}"
    );
}

#[test]
fn server_run_identity_binding_is_enforced_before_usage_is_recorded() {
    let assignment = assignment();
    let mut prepared = prepared(ModelProtocol::OpenAiChat);
    prepared.spec.assignment = Some(assignment.clone());
    prepared.seal();
    let response = json!({
        "model": "served",
        "usage": {"prompt_tokens": 4, "completion_tokens": 3}
    });

    let usage = normalize_provider_usage(
        &prepared,
        &response,
        UsageId::new(),
        AttemptId::new(),
        assignment.run_id,
        0,
    )
    .expect("matching run identity");
    assert_eq!(usage.run_id, assignment.run_id);

    let error = normalize_provider_usage(
        &prepared,
        &response,
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        0,
    )
    .expect_err("run mismatch must be rejected");
    assert_eq!(error, "provider_usage_run_mismatch");

    let error = normalize_provider_usage(
        &prepared,
        &response,
        UsageId::new(),
        AttemptId::new(),
        assignment.run_id,
        1_000_001,
    )
    .expect_err("implausible retry ordinal must be rejected");
    assert_eq!(error, "provider_usage_retry_ordinal_invalid");
}

#[test]
fn provider_self_reported_fields_cannot_extend_or_authorize_the_observation() {
    let response = json!({
        "model": "served",
        "authorization": "Bearer fixture-secret",
        "permit": "permit-sentinel",
        "approved": true,
        "grant": "grant-sentinel",
        "capability": "shell",
        "usage": {"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3}
    });
    let usage = normalize(ModelProtocol::OpenAiChat, response.clone());

    // 原始回包只留摘要；自报的“已授权”字段既不落进观测，也不改变任何维度。
    assert_eq!(usage.raw_digest, json_digest(&response));
    assert_eq!(usage.vector.input_tokens, Some(2));
    assert_eq!(usage.vector.output_tokens, Some(1));
    let encoded = serde_json::to_string(&usage).expect("usage json");
    for sentinel in [
        "Bearer",
        "fixture-secret",
        "permit-sentinel",
        "grant-sentinel",
        "approved",
        "capability",
    ] {
        assert!(
            !encoded.contains(sentinel),
            "provider 自报字段进入观测: {sentinel}"
        );
    }

    // 诚实往返稳定；注入一个“授权”字段则整条记录不可表示（deny_unknown_fields）。
    let decoded: Value = serde_json::from_str(&encoded).expect("usage value");
    assert_eq!(
        serde_json::from_value::<kiana_domain::NormalizedUsage>(decoded.clone())
            .expect("usage round-trip"),
        usage
    );
    let mut extended = decoded;
    extended["permit"] = json!("permit-sentinel");
    assert!(
        serde_json::from_value::<kiana_domain::NormalizedUsage>(extended).is_err(),
        "授权字段不得被记录进用量观测"
    );
}

#[test]
fn declared_containment_matches_every_wire_dialect() {
    // 三态声明：Some(true) 已含在容器里 / Some(false) 独立可加 / None 未声明。
    for (protocol, cache_read, cache_write, reasoning) in [
        (ModelProtocol::OpenAiChat, Some(true), None, Some(true)),
        (ModelProtocol::OpenAiResponses, Some(true), None, Some(true)),
        (
            ModelProtocol::GeminiInteractions,
            Some(true),
            None,
            Some(false),
        ),
        (
            ModelProtocol::AnthropicMessages,
            Some(false),
            Some(false),
            None,
        ),
        (ModelProtocol::OllamaChat, None, None, None),
        (ModelProtocol::Legacy, None, None, None),
    ] {
        let containment = provider_usage_containment(protocol);
        assert_eq!(containment.protocol, protocol, "{protocol:?}");
        assert_eq!(
            containment.cache_read_contained_in_input, cache_read,
            "{protocol:?}"
        );
        assert_eq!(
            containment.cache_write_contained_in_input, cache_write,
            "{protocol:?}"
        );
        assert_eq!(
            containment.reasoning_contained_in_output, reasoning,
            "{protocol:?}"
        );
    }

    // OpenAI：缓存与推理都嵌在容器里，净额是厂商自报的 input+output（15），不是 10+4+5+2。
    let chat = normalize(
        ModelProtocol::OpenAiChat,
        json!({
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
                "prompt_tokens_details": {"cached_tokens": 4},
                "completion_tokens_details": {"reasoning_tokens": 2}
            }
        }),
    );
    assert_eq!(chat.vector.cache_read_tokens, Some(4));
    assert_eq!(chat.vector.reasoning_output_tokens, Some(2));
    let chat_chargeable = provider_usage_containment(ModelProtocol::OpenAiChat)
        .chargeable(&chat.vector)
        .expect("chargeable usage");
    assert_eq!(chat_chargeable.input_units, 6);
    assert_eq!(chat_chargeable.cache_read_units, 4);
    assert_eq!(chat_chargeable.output_units, 3);
    assert_eq!(chat_chargeable.reasoning_units, 2);
    assert_eq!(chat_chargeable.total_units, 15);

    // Anthropic：缓存读写报在 input 之外，因此额外相加（同一张表声明为独立）。
    let anthropic = normalize(
        ModelProtocol::AnthropicMessages,
        json!({
            "usage": {
                "input_tokens": 4,
                "output_tokens": 3,
                "cache_read_input_tokens": 2,
                "cache_creation_input_tokens": 1
            }
        }),
    );
    let anthropic_chargeable = provider_usage_containment(ModelProtocol::AnthropicMessages)
        .chargeable(&anthropic.vector)
        .expect("chargeable usage");
    assert_eq!(anthropic_chargeable.input_units, 4);
    assert_eq!(anthropic_chargeable.cache_read_units, 2);
    assert_eq!(anthropic_chargeable.cache_write_units, 1);
    assert_eq!(anthropic_chargeable.total_units, 10);

    // Gemini：缓存已含在 prompt 里；thinking 报在 candidates 之外，要单独加。
    let gemini = normalize(
        ModelProtocol::GeminiInteractions,
        json!({
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "totalTokenCount": 15,
                "cachedContentTokenCount": 4,
                "thoughtsTokenCount": 2
            }
        }),
    );
    let gemini_chargeable = provider_usage_containment(ModelProtocol::GeminiInteractions)
        .chargeable(&gemini.vector)
        .expect("chargeable usage");
    assert_eq!(gemini_chargeable.input_units, 6);
    assert_eq!(gemini_chargeable.cache_read_units, 4);
    assert_eq!(gemini_chargeable.reasoning_units, 2);
    assert_eq!(gemini_chargeable.total_units, 17);

    // Ollama：只有 prompt/eval 两个数，没有未声明重叠。
    let ollama = normalize(
        ModelProtocol::OllamaChat,
        json!({"prompt_eval_count": 10, "eval_count": 5}),
    );
    assert!(!provider_usage_containment(ModelProtocol::OllamaChat)
        .has_undeclared_overlap(&ollama.vector));
    assert_eq!(
        provider_usage_containment(ModelProtocol::OllamaChat)
            .chargeable(&ollama.vector)
            .expect("chargeable usage")
            .total_units,
        15
    );

    // Fake/Legacy：关系未声明；出现缓存维度时按“未声明重叠”保守处理，不发明价格关系。
    let legacy = normalize(
        ModelProtocol::Legacy,
        json!({
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15,
                "cache_read_tokens": 4
            }
        }),
    );
    assert!(
        provider_usage_containment(ModelProtocol::Legacy).has_undeclared_overlap(&legacy.vector)
    );
    assert_eq!(
        provider_usage_containment(ModelProtocol::Legacy)
            .chargeable(&legacy.vector)
            .expect("chargeable usage")
            .total_units,
        19
    );
}

#[test]
fn containment_contradictions_are_rejected_before_settlement() {
    for (protocol, response, expected) in [
        (
            ModelProtocol::OpenAiChat,
            json!({"usage": {
                "prompt_tokens": 2,
                "completion_tokens": 3,
                "total_tokens": 5,
                "prompt_tokens_details": {"cached_tokens": 5}
            }}),
            "provider_usage_containment_violated:input_tokens",
        ),
        (
            ModelProtocol::OpenAiResponses,
            json!({"usage": {
                "input_tokens": 2,
                "output_tokens": 3,
                "total_tokens": 5,
                "output_tokens_details": {"reasoning_tokens": 4}
            }}),
            "provider_usage_containment_violated:output_tokens",
        ),
        (
            ModelProtocol::GeminiInteractions,
            json!({"usageMetadata": {
                "promptTokenCount": 2,
                "candidatesTokenCount": 3,
                "totalTokenCount": 5,
                "cachedContentTokenCount": 5
            }}),
            "provider_usage_containment_violated:input_tokens",
        ),
    ] {
        assert_eq!(reject(protocol, response), expected, "{protocol:?}");
    }

    // Anthropic 的缓存维度独立可加：同一个数字在这里合法。
    let anthropic = normalize(
        ModelProtocol::AnthropicMessages,
        json!({"usage": {"input_tokens": 2, "output_tokens": 3, "cache_read_input_tokens": 5}}),
    );
    assert_eq!(anthropic.vector.cache_read_tokens, Some(5));
    assert_eq!(anthropic.confidence, UsageConfidence::Known);

    // 未声明的方言不发明关系：Legacy 接受 5 > 2，但标记为未声明重叠保持保守。
    let legacy = normalize(
        ModelProtocol::Legacy,
        json!({"usage": {"input_tokens": 2, "output_tokens": 3, "cache_read_tokens": 5}}),
    );
    assert!(
        provider_usage_containment(ModelProtocol::Legacy).has_undeclared_overlap(&legacy.vector)
    );

    // 只报维度、没报容器的部分观测不在这里拒绝：保留 Partial，包含关系算术报不可证。
    let unprovable = normalize(
        ModelProtocol::OpenAiChat,
        json!({"usage": {"completion_tokens": 3, "prompt_tokens_details": {"cached_tokens": 5}}}),
    );
    assert_eq!(unprovable.vector.input_tokens, None);
    assert_eq!(unprovable.vector.cache_read_tokens, Some(5));
    assert_eq!(unprovable.confidence, UsageConfidence::Partial);
    assert_eq!(
        provider_usage_containment(ModelProtocol::OpenAiChat)
            .chargeable(&unprovable.vector)
            .expect_err("containment is unprovable without the container"),
        "provider_usage_containment_unprovable:input_tokens"
    );
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
