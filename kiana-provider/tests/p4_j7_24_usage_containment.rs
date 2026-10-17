//! P4-J7-24：containment 表必须与各协议的适配器抽取一致，且能改变计费结果。
//!
//! 这条夹具把三层串起来证明一次：**原始回包** → `normalize_provider_usage` 的字段抽取 →
//! `ProviderUsageContainment` 的声明 → `ProviderUsageSettlement` 的估价。任何一层把
//! “已含在容器里的维度” 当作独立维度相加，都会在这里的总额上暴露成重复计数。
//!
//! 证明上限：本夹具只证明源码层行为（本地未运行；回执由 CI 产生）。

use kiana_domain::{
    AttemptId, BillingUnknownReason, ModelCallSpec, ModelProtocol, ModelPurpose, ModelRequest,
    ModelResponseFormat, ModelRoute, NormalizedUsage, PreparedModelCall, ProviderUsageContainment,
    ProviderUsageSettlement, RateCard, RateCardId, RequestId, RunId, SettlementCost, TokenBudget,
    UsageConfidence, UsageId,
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

fn normalize(protocol: ModelProtocol, response: Value) -> NormalizedUsage {
    normalize_provider_usage(
        &prepared(protocol),
        &response,
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        1,
    )
    .expect("normalized usage")
}

/// 费率卡：输入 2 / 输出 3，可按需要声明缓存与推理价；请求价为 0 以便估价可完全确定。
fn card(
    cache_read_price: Option<u64>,
    cache_write_price: Option<u64>,
    reasoning_price: Option<u64>,
) -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider-fixture",
        "requested-model",
        "USD",
        Some(2),
        Some(3),
        1,
        None,
        1,
        "fixture-rate-card",
    )
    .expect("rate card");
    card.cache_read_price_per_unit = cache_read_price;
    card.cache_write_price_per_unit = cache_write_price;
    card.reasoning_price_per_unit = reasoning_price;
    card.request_price = Some(0);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid rate card");
    card
}

/// 返回 `(金额, 未知原因)`：金额只在**完全定价**时才存在。
///
/// `ProviderUsageSettlement` 把“未定价维度”保持为 `Unknown`（没有金额、只有原因），所以这里
/// 必须两种形状都能读：有金额就断言金额，没有就必须断言原因，绝不能猜一个数字出来。
fn cost_summary(
    usage: &NormalizedUsage,
    protocol: ModelProtocol,
    card: &RateCard,
) -> (Option<i128>, Option<BillingUnknownReason>) {
    let settlement =
        ProviderUsageSettlement::from_usage_with_protocol(usage, protocol, Some(card), 1, 100)
            .expect("settlement");
    match settlement.cost {
        SettlementCost::Estimated { estimate } => (
            estimate.amount.map(|amount| amount.micros),
            estimate.unknown_reason,
        ),
        SettlementCost::Unknown { reason } => (None, Some(reason)),
        SettlementCost::Measured { .. } => (None, None),
    }
}

fn chargeable_units(usage: &NormalizedUsage, protocol: ModelProtocol) -> u64 {
    ProviderUsageContainment::declared(protocol)
        .chargeable(&usage.vector)
        .expect("chargeable usage")
        .total_units
}

#[test]
fn openai_contained_cache_and_reasoning_are_charged_once() {
    // ChatGPT 形状：cached_tokens 在 prompt_tokens 之内、reasoning_tokens 在 completion_tokens
    // 之内。适配器从嵌套细节对象里抽出来，containment 表声明它们“已含”。
    let chat = normalize(
        ModelProtocol::OpenAiChat,
        json!({
            "model": "served-openai",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
                "prompt_tokens_details": {"cached_tokens": 4},
                "completion_tokens_details": {"reasoning_tokens": 2}
            }
        }),
    );
    assert_eq!(chat.vector.input_tokens, Some(10));
    assert_eq!(chat.vector.cache_read_tokens, Some(4));
    assert_eq!(chat.vector.reasoning_output_tokens, Some(2));
    assert_eq!(chat.confidence, UsageConfidence::Known);
    // 净额就是厂商自己报的 input+output，而不是 10+4+5+2。
    assert_eq!(chargeable_units(&chat, ModelProtocol::OpenAiChat), 15);

    // 只标输入/输出价的费率卡可以完整定价：容器已覆盖缓存与推理，不需要再加一行。
    let (micros, unknown) = cost_summary(&chat, ModelProtocol::OpenAiChat, &card(None, None, None));
    assert_eq!(micros, Some(10 * 2 + 5 * 3));
    assert_eq!(unknown, None);

    // 同一张卡如果分别标了缓存/推理价，也只按各自那一行收一次，并从容器里扣除。
    let (micros, unknown) = cost_summary(
        &chat,
        ModelProtocol::OpenAiChat,
        &card(Some(1), None, Some(7)),
    );
    assert_eq!(micros, Some(6 * 2 + 4 * 1 + 3 * 3 + 2 * 7));
    assert_eq!(unknown, None);

    // Responses 形状同样是“含在容器里”，抽取字段名不同但声明一致。
    let responses = normalize(
        ModelProtocol::OpenAiResponses,
        json!({
            "model": "served-responses",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15,
                "input_tokens_details": {"cached_tokens": 4},
                "output_tokens_details": {"reasoning_tokens": 2}
            }
        }),
    );
    assert_eq!(responses.vector.cache_read_tokens, Some(4));
    assert_eq!(responses.vector.reasoning_output_tokens, Some(2));
    assert_eq!(
        chargeable_units(&responses, ModelProtocol::OpenAiResponses),
        15
    );
    let (micros, unknown) = cost_summary(
        &responses,
        ModelProtocol::OpenAiResponses,
        &card(None, None, None),
    );
    assert_eq!(micros, Some(10 * 2 + 5 * 3));
    assert_eq!(unknown, None);
}

#[test]
fn anthropic_disjoint_cache_dimensions_are_priced_additively() {
    // Anthropic 把缓存读/写报在 input_tokens 之外：同样的数字在这里是真正可加的。
    let usage = normalize(
        ModelProtocol::AnthropicMessages,
        json!({
            "model": "served-claude",
            "usage": {
                "input_tokens": 4,
                "output_tokens": 3,
                "cache_read_input_tokens": 2,
                "cache_creation_input_tokens": 1
            }
        }),
    );
    assert_eq!(usage.vector.cache_read_tokens, Some(2));
    assert_eq!(usage.vector.cache_write_tokens, Some(1));
    assert_eq!(usage.vector.reasoning_output_tokens, None);
    assert_eq!(
        chargeable_units(&usage, ModelProtocol::AnthropicMessages),
        10
    );

    // 未定价的独立维度会让估价保持不完整，而不是被静默塞进输入价里：没有金额，只有原因。
    let (micros, unknown) = cost_summary(
        &usage,
        ModelProtocol::AnthropicMessages,
        &card(None, None, None),
    );
    assert_eq!(micros, None);
    assert_eq!(unknown, Some(BillingUnknownReason::RateCardMissing));

    // 定价后得到完整的可加账单。
    let (micros, unknown) = cost_summary(
        &usage,
        ModelProtocol::AnthropicMessages,
        &card(Some(1), Some(1), None),
    );
    assert_eq!(micros, Some(4 * 2 + 3 * 3 + 2 * 1 + 1 * 1));
    assert_eq!(unknown, None);
}

#[test]
fn gemini_contains_cache_but_not_thinking_tokens() {
    let usage = normalize(
        ModelProtocol::GeminiInteractions,
        json!({
            "modelVersion": "served-gemini",
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "totalTokenCount": 15,
                "cachedContentTokenCount": 4,
                "thoughtsTokenCount": 2
            }
        }),
    );
    assert_eq!(usage.vector.cache_read_tokens, Some(4));
    assert_eq!(usage.vector.reasoning_output_tokens, Some(2));
    // 缓存含在 promptTokenCount 里；thinking token 报在 candidate 之外，因此要单独加。
    assert_eq!(
        chargeable_units(&usage, ModelProtocol::GeminiInteractions),
        10 + 5 + 2
    );

    // 不标推理价时估算保持不完整：thinking token 有数但没有价。
    let (micros, unknown) = cost_summary(
        &usage,
        ModelProtocol::GeminiInteractions,
        &card(None, None, None),
    );
    assert_eq!(micros, None);
    assert_eq!(unknown, Some(BillingUnknownReason::RateCardMissing));

    // 标上推理价后，缓存仍不额外收费（已含），thinking 按自己的价收一次。
    let (micros, unknown) = cost_summary(
        &usage,
        ModelProtocol::GeminiInteractions,
        &card(None, None, Some(1)),
    );
    assert_eq!(micros, Some(10 * 2 + 5 * 3 + 2 * 1));
    assert_eq!(unknown, None);
}

#[test]
fn undeclared_or_self_hosted_routes_never_invent_a_price() {
    // Legacy（离线 fake）没有声明缓存/推理关系：token 总值按可加读法保守计算，
    // 但估价拒绝定价，回退到与其他未知重叠一致的 Unsupported。
    let legacy = normalize(
        ModelProtocol::Legacy,
        json!({
            "model": "served-fake",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15,
                "cache_read_tokens": 4,
                "reasoning_output_tokens": 2
            }
        }),
    );
    assert!(ProviderUsageContainment::declared(ModelProtocol::Legacy)
        .has_undeclared_overlap(&legacy.vector));
    assert_eq!(chargeable_units(&legacy, ModelProtocol::Legacy), 21);
    let (micros, unknown) = cost_summary(&legacy, ModelProtocol::Legacy, &card(None, None, None));
    assert_eq!(micros, None);
    assert_eq!(unknown, Some(BillingUnknownReason::Unsupported));

    // Ollama（本机自托管）只有 prompt/eval 两个数：零价卡绝不等于“免费调用”。
    let ollama = normalize(
        ModelProtocol::OllamaChat,
        json!({"model": "served-ollama", "prompt_eval_count": 10, "eval_count": 5}),
    );
    assert_eq!(chargeable_units(&ollama, ModelProtocol::OllamaChat), 15);
    let mut zero = RateCard::new(
        RateCardId::new(),
        "provider-fixture",
        "requested-model",
        "USD",
        Some(0),
        Some(0),
        1,
        None,
        1,
        "fixture-rate-card",
    )
    .expect("zero rate card");
    zero.request_price = Some(0);
    zero.rate_card_digest = zero.digest();
    zero.validate().expect("valid zero card");
    let settlement = ProviderUsageSettlement::from_usage_with_protocol(
        &ollama,
        ModelProtocol::OllamaChat,
        Some(&zero),
        1,
        100,
    )
    .expect("settlement");
    assert!(matches!(
        settlement.cost,
        SettlementCost::Unknown {
            reason: BillingUnknownReason::Unsupported
        }
    ));

    // 明确标了非零价格才产生 estimate。
    let (micros, unknown) =
        cost_summary(&ollama, ModelProtocol::OllamaChat, &card(None, None, None));
    assert_eq!(micros, Some(10 * 2 + 5 * 3));
    assert_eq!(unknown, None);
}

#[test]
fn known_tokens_without_a_rate_card_never_become_a_bill() {
    let usage = normalize(
        ModelProtocol::OpenAiChat,
        json!({
            "model": "served-openai",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
                "prompt_tokens_details": {"cached_tokens": 4}
            }
        }),
    );
    let settlement = ProviderUsageSettlement::from_usage_with_protocol(
        &usage,
        ModelProtocol::OpenAiChat,
        None,
        1,
        100,
    )
    .expect("settlement");
    // token 统计可得，不等于账单可得：没有价目表就没有金额。
    assert!(matches!(
        settlement.cost,
        SettlementCost::Unknown {
            reason: BillingUnknownReason::RateCardMissing
        }
    ));
    assert_eq!(
        ProviderUsageContainment::declared(ModelProtocol::OpenAiChat)
            .chargeable(&usage.vector)
            .expect("chargeable")
            .total_units,
        15
    );
}
