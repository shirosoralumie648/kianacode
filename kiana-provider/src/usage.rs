//! Provider-neutral usage normalization at the admitted model boundary.
//!
//! The adapter only turns a completed provider reply into a domain observation.  It does not
//! charge a project, reserve a permit, retry a request or append an EventLog fact.
//!
//! ## 这个文件在系统里的位置
//!
//! ```text
//!   transport::send() 拿到 ModelReply（对端回包已解析）
//!        │
//!        ↓
//!   【本文件 normalize_model_reply()】← 把各家不同的 usage 字段归一成 UsageVector
//!        │
//!        ↓
//!   NormalizedUsage → ControlPlane 落成事件 → 预算/成本账本
//! ```
//!
//! ## 上游 / 下游
//!
//! - 上游：`ModelReply`，由 `response.rs` 解析上游回包得到。
//! - 下游：`NormalizedUsage`，供 `kiana-core` 的成本/预算链路消费。
//! - ⚠ `rg -n "normalize_model_reply"` 显示：**除本文件与 `lib.rs` 的重新导出外，
//!   仓库中没有其它调用点**。当前它是已就位、待接入的归一化边界。
//!
//! ## 本文件最重要的一条规则
//!
//! **“对端没报 usage” ⇒ Unknown，而不是 0。**
//! 把“没报”当成“0”会让成本账本凭空少算，用户看到的账单就是错的。
//! Kiana 宁可显示“不知道花了多少”，也不显示一个看起来精确的 0。

use kiana_domain::{
    json_digest, AttemptId, BillingUnknownReason, ModelReply, NormalizedUsage, PreparedModelCall,
    UsageConfidence, UsageId, UsageObservation, UsageSource, UsageVector,
};
use serde_json::json;

/// 把一次已准入调用的回包，归一化成一条最终的 provider 用量观测。
///
/// 【作用】
/// 各家服务商的 usage 字段名和位置都不一样（Anthropic 放 `usage.input_tokens`，
/// OpenAI 放 `usage.prompt_tokens`，…）。本函数把它们收敛成同一个 `UsageVector`，
/// 并补上 `UsageConfidence`（已知 / 未知）与未知原因。
///
/// 【调用者】
/// 未在仓库中找到生产调用点；仅由 `kiana-provider/src/lib.rs` 重新导出。
///
/// 【输入】
/// - `prepared`：冻结的调用描述，用来取 provider_id / model_id / route。
/// - `reply`：解析后的回包，`reply.output.usage` 是 `Option`——`None` 即“对端没报”。
/// - `usage_id` / `attempt_id` / `run_id`：**由调用方提供的服务端身份**。
///
///   【⚠ 为什么不从 `reply` 里取 run_id】
///   因为回包是外部数据。让外部数据决定“这次调用属于哪次运行”，等于让对端可以自称
///   “我这次是 run-7 的”。所以身份一律由服务端注入。
///
/// 【输出】 `NormalizedUsage`（已 `validate()` 过）。
///
/// 【副作用】 无。纯函数，不写事件、不扣预算、不重试。
///
/// 【失败情况】
/// - `prepared.validate()` 失败 → 转成字符串错误返回。
/// - `provider_usage_run_mismatch`：`prepared` 里记录的 run 与传入的 `run_id` 不一致。
///   这通常意味着调用方把 A 运行的回包挂到了 B 运行上，属于严重错配。
/// - `NormalizedUsage::new(...)` / 末尾 `validate()` 失败 → 领域层拒绝。
/// Convert one admitted reply into a final provider usage observation.
///
/// Missing provider usage remains `Unknown`; it is never converted to zero.  The caller supplies
/// the server-owned attempt/run identity because provider wire data cannot become an authority for
/// those fields.
pub fn normalize_model_reply(
    prepared: &PreparedModelCall,
    reply: &ModelReply,
    usage_id: UsageId,
    attempt_id: AttemptId,
    run_id: kiana_domain::RunId,
) -> Result<NormalizedUsage, String> {
    prepared.validate().map_err(|error| error.to_string())?;
    if prepared
        .spec
        .assignment
        .as_ref()
        .is_some_and(|assignment| assignment.run_id != run_id)
    {
        return Err("provider_usage_run_mismatch".to_owned());
    }
    // 归一化的核心分叉：有 usage ⇒ Known；没 usage ⇒ Unknown + 明确原因。
    //
    // ⚠ 这里**不能**写 `usage.map(|u| (u.input_tokens, u.output_tokens)).unwrap_or((0, 0))`。
    // 那样“没报”就变成了“报了 0”，成本账本会静默少算，而且是**不可察觉**的少算——
    // 这是比报错更糟的失败模式。

    let (input_tokens, output_tokens, confidence, unknown_reason) = match &reply.output.usage {
        Some(usage) => (
            Some(usage.input_tokens),
            Some(usage.output_tokens),
            UsageConfidence::Known,
            None,
        ),
        None => (
            None,
            None,
            UsageConfidence::Unknown,
            Some(BillingUnknownReason::ProviderUnreported),
        ),
    };
    // UsageVector 是“所有用量维度”的并集：不是每家 provider 都会填满所有字段。
    // 下面这一串 `None` 不是“忘记赋值”，而是**明确的“本次观测没有这个维度”**，
    // 与 `Some(0)` 语义完全不同。

    let vector = UsageVector {
        schema: kiana_domain::USAGE_VECTOR_SCHEMA.to_owned(),
        input_tokens,
        output_tokens,
        cache_read_tokens: None,
        cache_write_tokens: None,
        reasoning_output_tokens: None,
        audio_input_tokens: None,
        audio_output_tokens: None,
        tool_calls: 0,
        effect_count: 0,
        wall_time_ms: 0,
        output_bytes: 0,
        artifact_bytes: 0,
        storage_bytes: 0,
    };
    // 原始摘要：把“上游实际返回了什么”与“用的是哪条路由”一起哈希。
    //
    // 【为什么只哈希不落原文】
    // usage 数据本身不算敏感，但审计需要能证明“这份用量数字确实来自那次回包”。
    // 存摘要即可验证，不需要把原始 JSON 再存一份。
    //
    // 里面刻意**不含** prompt、响应正文、endpoint、凭据——那些永远不进账本。

    let raw_digest = json_digest(&json!({
        "provider": prepared.route.provider_id,
        "protocol": prepared.route.protocol,
        "request_id": reply.provider_request_id,
        "response_id": reply.provider_response_id,
        "usage": reply.output.usage,
        "model": reply.output.model_id,
    }));
    let mut normalized = NormalizedUsage::new(
        usage_id,
        attempt_id,
        run_id,
        Some(prepared.route.provider_id.clone()),
        prepared.route.model_id.clone(),
        prepared.route.connection_id.clone(),
        vector,
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        unknown_reason,
        format!("provider:{}", protocol_name(prepared.route.protocol)),
        raw_digest,
    )?;
    // A provider that omits its served model has not proven that it served the requested model.
    // Keep the observation unknown instead of collapsing requested and served identities.
    // 【⚠ 这里是一个刻意的“不对称”，初学者极易看漏】
    //
    //   请求的模型：NormalizedUsage::new(...) 的第 5 个参数，用的是 prepared.route.model_id
    //   实际服务的模型：下面这行，用的是 reply.output.model_id
    //
    // 为什么要分开？因为**请求什么 ≠ 对方给什么**。
    // 你请求 gpt-4o，对端完全可能返回一个它自己降级后的模型。
    // 如果把两者合并成一个字段，就等于宣称“我们确认对方按请求的模型执行了”——这是没证据的。
    // 保留两个字段，差异本身就是可审计的事实。
    //
    // 源码英文注释 “A provider that omits its served model has not proven that it served
    // the requested model.” 说的就是这个意思。

    normalized.served_model_id = reply.output.model_id.clone();
    normalized.retry_ordinal = 0;
    // 用填好的完整内容重算一次摘要。
    // 顺序很重要：必须**在**设置 served_model_id / retry_ordinal 之后算，
    // 否则摘要对应的就是一份半成品。

    normalized.usage_digest = normalized.digest();
    normalized.validate()?;
    Ok(normalized)
}

/// 把协议枚举翻译成可读的短名，写进 `NormalizedUsage` 的来源说明里。
///
/// 【为什么需要它】
/// 上游数据里只有 `ModelProtocol` 枚举，序列化成 JSON 后是 `"AnthropicMessages"` 这种
/// 大驼峰写法。账本/报表里希望看到 `anthropic_messages` 这种下划线短名。
/// 这个 match 是**穷举式**的：将来 `ModelProtocol` 新增一个变体，编译器会在这里报错，
/// 强制你补上新分支——这正是我们想要的（不会静默漏掉一个新协议）。

fn protocol_name(protocol: kiana_domain::ModelProtocol) -> &'static str {
    match protocol {
        kiana_domain::ModelProtocol::Legacy => "legacy",
        kiana_domain::ModelProtocol::AnthropicMessages => "anthropic_messages",
        kiana_domain::ModelProtocol::OpenAiChat => "openai_chat",
        kiana_domain::ModelProtocol::OpenAiResponses => "openai_responses",
        kiana_domain::ModelProtocol::OllamaChat => "ollama_chat",
        kiana_domain::ModelProtocol::GeminiInteractions => "gemini_interactions",
    }
}
