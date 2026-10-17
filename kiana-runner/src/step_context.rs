//! 单个模型步的不可变上下文编译与解释。
//!
//! 【职责】把当前步已经冻结的 PromptBundle 与消息快照编译为一次 `ContextPlan`，
//! 并绑定路由/目录/workspace/data 与预算，产出 `ResolvedStepContext`（provider 侧
//! 别名 `PreparedModelRequest`）。harness 用同一个对象完成「估算」和「发送」，避免
//! `request_context()` 与 `complete()` 各自读取不同环境。
//!
//! 【分层规则】产品 system/role 只能来自 PromptBundle 的 Product 段；用户消息、
//! 助手历史与工具观测只能作为不可信 Context 物料进入计划（Task / History /
//! LiveResult），任何来源都不能通过本模块升格为 Product。尝试把工作区文本声明为
//! Product 会被 domain 合同以 `context_product_source_untrusted` 拒绝。
//!
//! 【边界】本模块不发请求、不执行工具、不读取工作区、不查询 Memory，也不授予
//! 任何权限：`ScopeSet` 字段是 runner 侧占位（真实授权与 scope 交集仍在 ControlPlane）。
//! workspace/data 绑定是由请求内容与 bundle 派生的确定性本地摘要，不是 EventStore
//! 的 revision/epoch 权威。
//!
//! 【为什么 requirement 是「bundle 路径」】只有结构化 instructions（PromptBundle）
//! 才能让 runner 独立重建不可信分层；legacy 纯文本 instructions 没有 bundle，
//! 由调用方与 Core 负责，不在此编译。

use kiana_domain::{
    json_digest, ContextCandidate, ContextMaterialType, ContextPlan, EvidenceStatus, Freshness,
    ModelContent, ModelMessage, ModelRole, ModelUsage, PreparedModelCall, PromptAuthority,
    PromptBundle, RequestUsageDelta, ResolvedStepContext, ScopeSet, SourceKind, SourceRef,
    SourceSnapshot, StablePrefix, StablePrefixSegment, StepIdentity, TokenAccounting, WireBudget,
    WireBudgetInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 解释对象的稳定 schema；`ModelTurn` metadata 的 `step_context` 字段使用它。
pub const STEP_CONTEXT_EXPLANATION_SCHEMA: &str = "kiana.step-context-explanation.v1";

/// 一次编译最多把最近多少条历史消息作为候选；更旧的素材不进入本切片计划。
pub const STEP_CONTEXT_MAX_MESSAGE_CANDIDATES: usize = 200;

/// 无精确 tokenizer 时的显式回退会计：1 wire byte ≥ 1 token，另加固定安全余量。
///
/// 这是本地保守估计，不是任何 provider tokenizer 的硬上界或计费依据；当适配器能提供
/// tokenizer digest 时，应改用 domain 的 [`TokenAccounting::ExactTokenizer`]。
pub const CONSERVATIVE_WIRE_ACCOUNTING: TokenAccounting = TokenAccounting::ConservativeUtf8 {
    bytes_per_token: 1,
    safety_margin_tokens: 4_096,
};

fn default_wire_accounting() -> TokenAccounting {
    CONSERVATIVE_WIRE_ACCOUNTING.clone()
}

/// 单个模型步的编译输入。所有字段由服务端流程产生，模型文本无法直接写入。
#[derive(Clone, Debug)]
pub struct StepContextInputs<'a> {
    /// 当前步解码后的 PromptBundle（产品语气与角色来源）。
    pub bundle: &'a PromptBundle,
    /// 即将发送的消息快照；用户/助手/工具消息成为不可信 Context 候选。
    pub messages: &'a [ModelMessage],
    /// sandbox 展示值，仅参与 data 绑定摘要，不是执行授权。
    pub sandbox: &'a str,
    /// 服务端分配的步身份。
    pub step_identity: StepIdentity,
    /// provider/model profile（通常来自 bundle 或路由）。
    pub model_profile: String,
    /// 该步冻结的路由摘要。
    pub route_digest: String,
    /// 该步冻结的工具目录摘要（`PreparedModelCall::tool_catalog_hash`）。
    pub catalog_digest: String,
    /// 已按最终 wire 体计算的预算快照；其 `token_limit` 同时作为计划上限。
    pub wire_budget: WireBudget,
}

/// 计划中每个被剔除条目的原因，供审计与 `ModelTurn` 解释使用。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepContextOmission {
    /// 计划条目名（Prompt 段名或 `message-<index>`）。
    pub name: String,
    /// 稳定剔除原因（当前只可能是来源预算或总预算超限）。
    pub reason: String,
}

/// 前缀缓存的复用决策：`reused=false` 且带稳定原因表示旧条目不适用于当前请求。
///
/// 它只记录决策，不建立 provider 缓存、也不授予任何权限；`None`（未记录）表示本 run
/// 首次观察该前缀，没有可复用的旧条目。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StablePrefixReuse {
    pub reused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl StablePrefixReuse {
    /// 把 domain 的复用判定转成可记录的解释。
    pub fn from_decision(decision: Result<(), String>) -> Self {
        match decision {
            Ok(()) => Self {
                reused: true,
                reason: None,
            },
            Err(reason) => Self {
                reused: false,
                reason: Some(reason),
            },
        }
    }
}

/// 预算分项的观测字节数（解释用；不是新增授权或计费证据）。
///
/// `system`/`history_content_blocks`/`tool_schemas`/`attachments` 四项相加（附件引用已在
/// 消息字节之外单列）不超过 wire 体字节数；`provider_framing` 是其余包装开销。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepBudgetPart {
    pub item: String,
    pub bytes: u64,
}

/// 一个模型步的上下文解释：摘要与计数，不包含完整消息文本。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepContextExplanation {
    /// 解释 schema，固定为 [`STEP_CONTEXT_EXPLANATION_SCHEMA`]。
    pub schema: String,
    /// `ContextPlan` 自摘要。
    pub plan_digest: String,
    /// `ResolvedStepContext` 请求摘要（估算/发送复用的同一对象）。
    pub request_digest: String,
    /// PromptBundle 摘要（产品语气与角色的来源身份）。
    pub prompt_bundle_digest: String,
    /// provider/model profile。
    pub model_profile: String,
    /// 冻结的路由摘要。
    pub route_digest: String,
    /// 冻结的工具目录摘要。
    pub catalog_digest: String,
    /// 渲染后 prompt 的摘要。
    pub rendered_prompt_digest: String,
    /// 计划 token 上限。
    pub token_limit: u64,
    /// 计划估算 token 数（保守 UTF-8 计数）。
    pub estimated_tokens: u64,
    /// 输出预留 token 数。
    pub output_reserved_tokens: u64,
    /// 计入预算的输入字节数（= wire 体字节数，或在分项之和更大时取分项之和）。
    pub wire_input_bytes: u64,
    /// 真正被 provider 序列化的 wire 体字节数。
    #[serde(default)]
    pub wire_body_bytes: u64,
    /// wire 预算的保守输入估算（含安全余量与 provider framing）。
    #[serde(default)]
    pub wire_estimated_input_tokens: u64,
    /// wire 预算的输入估算加输出预留。
    #[serde(default)]
    pub wire_total_reserved_tokens: u64,
    /// 显式会计标记：精确 tokenizer 或保守本地估计。
    #[serde(default = "default_wire_accounting")]
    pub budget_accounting: TokenAccounting,
    /// 预算覆盖的固定组成部分（domain 契约列表）。
    #[serde(default)]
    pub budget_coverage: Vec<String>,
    /// 观察到的请求分项字节数。
    #[serde(default)]
    pub budget_parts: Vec<StepBudgetPart>,
    /// 本步编译的稳定前缀；legacy/纯文本路径为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_prefix: Option<StablePrefix>,
    /// 相对本 run 上一次前缀的复用决策；首次观察为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_prefix_reuse: Option<StablePrefixReuse>,
    /// 估算/实际用量差值；发送后由 harness 补齐，未回报用量时 `usage_complete=false`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_delta: Option<RequestUsageDelta>,
    /// 由消息内容派生的 workspace 绑定值（非 EventStore revision）。
    pub workspace_revision: u64,
    /// 由 bundle 摘要与 sandbox 派生的 data 绑定值（非 EventStore epoch）。
    pub data_epoch: u64,
    /// 采纳条目数。
    pub included_items: u64,
    /// 剔除条目数。
    pub omitted_items: u64,
    /// 逐条剔除原因。
    pub omission_reasons: Vec<StepContextOmission>,
}

impl StepContextExplanation {
    /// 从已验证的冻结对象构造解释；只复制摘要与计数，不暴露消息文本。
    pub fn from_resolved(resolved: &ResolvedStepContext) -> Self {
        let omission_reasons = resolved
            .context_plan
            .omission_reasons()
            .into_iter()
            .map(|(name, reason)| StepContextOmission { name, reason })
            .collect::<Vec<_>>();
        Self {
            schema: STEP_CONTEXT_EXPLANATION_SCHEMA.to_owned(),
            plan_digest: resolved.plan_digest.clone(),
            request_digest: resolved.request_digest.clone(),
            prompt_bundle_digest: resolved.prompt_bundle_digest.clone(),
            model_profile: resolved.model_profile.clone(),
            route_digest: resolved.route_digest.clone(),
            catalog_digest: resolved.catalog_digest.clone(),
            rendered_prompt_digest: resolved.rendered_prompt_digest.clone(),
            token_limit: resolved.context_plan.token_limit,
            estimated_tokens: resolved.context_plan.estimated_tokens,
            output_reserved_tokens: resolved.wire_budget.output_reserved_tokens,
            wire_input_bytes: resolved.wire_budget.input_bytes,
            wire_body_bytes: resolved.wire_budget.input_bytes,
            wire_estimated_input_tokens: resolved.wire_budget.estimated_input_tokens,
            wire_total_reserved_tokens: resolved.wire_budget.total_reserved_tokens,
            budget_accounting: resolved.wire_budget.accounting.clone(),
            budget_coverage: resolved
                .wire_budget
                .coverage_items()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            budget_parts: Vec::new(),
            stable_prefix: None,
            stable_prefix_reuse: None,
            usage_delta: None,
            workspace_revision: resolved.workspace_revision,
            data_epoch: resolved.data_epoch,
            included_items: resolved
                .context_plan
                .items
                .iter()
                .filter(|item| item.included)
                .count() as u64,
            omitted_items: resolved
                .context_plan
                .items
                .iter()
                .filter(|item| !item.included)
                .count() as u64,
            omission_reasons,
        }
    }

    /// 记录实际 wire 体字节数与预算分项观测。
    pub fn with_wire_parts(mut self, wire_body_bytes: u64, parts: Vec<StepBudgetPart>) -> Self {
        self.wire_body_bytes = wire_body_bytes;
        self.budget_parts = parts;
        self
    }

    /// 记录稳定前缀身份与相对上一次的缓存复用决策。
    pub fn with_stable_prefix(
        mut self,
        prefix: &StablePrefix,
        reuse: Option<StablePrefixReuse>,
    ) -> Self {
        self.stable_prefix = Some(prefix.clone());
        self.stable_prefix_reuse = reuse;
        self
    }

    /// 发送后补齐估算/实际用量差值；未回报用量时记录 `usage_complete=false`。
    pub fn record_usage(&mut self, usage: Option<&ModelUsage>) {
        self.usage_delta = Some(RequestUsageDelta::from_usage(
            self.wire_estimated_input_tokens,
            self.output_reserved_tokens,
            usage,
        ));
    }
}

/// 编译单个模型步的不可变上下文快照。
///
/// 输入是调用方已冻结的 bundle、消息、步身份、路由/目录摘要与 wire 预算；输出是
/// 通过 domain 自检的 `ResolvedStepContext`。失败即拒绝该步（fail closed），
/// 不会退化成「没有计划也发送」。
pub fn compile_step_context(inputs: StepContextInputs<'_>) -> Result<ResolvedStepContext, String> {
    compile_step_context_with_prefix(inputs).map(|(resolved, _prefix)| resolved)
}

/// 用同一份输入快照同时编译 `ResolvedStepContext` 与稳定前缀。
///
/// 稳定前缀段只包含 Product 权威的（system/role）计划条目，按计划顺序冻结；动态资料
/// （任务、历史、工具观测、检索等）只进入 `dynamic_suffix_digest`，不参与前缀或缓存 key
/// 身份。authority/data epoch 是内容派生的本地绑定（见 [`derive_data_binding`]），
/// 不是 EventStore 的权威 revision；跨进程撤销仍由 Core 判定。
pub fn compile_step_context_with_prefix(
    inputs: StepContextInputs<'_>,
) -> Result<(ResolvedStepContext, StablePrefix), String> {
    let StepContextInputs {
        bundle,
        messages,
        sandbox,
        step_identity,
        model_profile,
        route_digest,
        catalog_digest,
        wire_budget,
    } = inputs;
    bundle.validate()?;
    wire_budget.validate()?;
    let token_limit = wire_budget.token_limit;
    let candidates = message_candidates(bundle, messages)?;
    let plan = ContextPlan::compile(bundle, candidates, token_limit)?;
    let workspace_revision = derive_workspace_binding(messages);
    let data_epoch = derive_data_binding(&plan.prompt_bundle_digest, sandbox);
    let source_snapshots = included_source_snapshots(&plan)?;
    let stable_prefix = compile_stable_prefix(
        &plan,
        messages,
        sandbox,
        &route_digest,
        &catalog_digest,
        &model_profile,
    )?;
    let resolved = ResolvedStepContext::new(
        &plan,
        ScopeSet::unrestricted(),
        step_identity,
        model_profile,
        route_digest,
        catalog_digest,
        source_snapshots,
        wire_budget,
        workspace_revision,
        data_epoch,
    )?;
    Ok((resolved, stable_prefix))
}

/// 编译稳定前缀身份：固定顺序的 Product 段 + profile/prompt/catalog/配置/epoch 绑定。
fn compile_stable_prefix(
    plan: &ContextPlan,
    messages: &[ModelMessage],
    sandbox: &str,
    route_digest: &str,
    catalog_digest: &str,
    model_profile: &str,
) -> Result<StablePrefix, String> {
    let segments = plan
        .items
        .iter()
        .filter(|item| item.included && item.authority == PromptAuthority::Product)
        .map(|item| StablePrefixSegment {
            name: item.name.clone(),
            order: item.priority,
            content_digest: json_digest(&json!({
                "name": item.name,
                "text": item.text,
                "material_type": item.material_type,
            })),
        })
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Err("step_context_stable_prefix_missing_product".to_owned());
    }
    // 权限收紧（路由/目录/sandbox 变化）改变 authority 绑定；数据删除/内容撤销改变
    // prompt bundle 摘要派生的 data 绑定。两者都参与缓存 key，旧条目不因此被复用。
    let authority_epoch = binding_from_digest(&json_digest(&json!({
        "route_digest": route_digest,
        "catalog_digest": catalog_digest,
        "sandbox": sandbox,
    })));
    let data_epoch = derive_data_binding(&plan.prompt_bundle_digest, sandbox);
    let dynamic_suffix_digest = json_digest(&json!({
        "messages": messages,
        "dynamic_items": plan
            .items
            .iter()
            .filter(|item| item.included && item.authority != PromptAuthority::Product)
            .map(|item| json!({
                "name": item.name,
                "revision": item.revision,
                "digest": json_digest(&json!({"text": item.text})),
            }))
            .collect::<Vec<_>>(),
    }));
    StablePrefix::new(
        model_profile,
        &plan.prompt_bundle_digest,
        catalog_digest,
        route_digest,
        authority_epoch,
        data_epoch,
        segments,
        dynamic_suffix_digest,
    )
}

/// 观测 provider 请求分项字节数；附件引用从消息字节中拆出，单独计入附件部分。
///
/// 返回的字节数与 `WireBudgetInput` 的字段一一对应（`attachment_bytes` 与消息部分互补，
/// 不会重复计数），供预算与解释共同使用。
pub fn wire_budget_parts(prepared: &PreparedModelCall) -> Result<WireBudgetParts, String> {
    let mut system_messages: Vec<&ModelMessage> = Vec::new();
    let mut message_parts: Vec<ModelMessage> = Vec::new();
    let mut attachments: Vec<ModelContent> = Vec::new();
    for message in &prepared.request.messages {
        if message.role == ModelRole::System {
            system_messages.push(message);
            continue;
        }
        let (message_part, message_attachments) = split_message_attachments(message);
        message_parts.push(message_part);
        attachments.extend(message_attachments);
    }
    let system_bytes = serialized_len(&system_messages)?;
    let message_bytes = serialized_len(&message_parts)?;
    let tool_schema_bytes = serialized_len(&prepared.request.tools)?;
    let attachment_bytes = serialized_len(&attachments)?;
    let wire_body_bytes = serialized_value_len(&prepared.wire_body)?;
    Ok(WireBudgetParts {
        system_bytes,
        message_bytes,
        tool_schema_bytes,
        attachment_bytes,
        wire_body_bytes,
    })
}

/// provider 请求分项观测（解释用，不是授权或计费证据）。
#[derive(Clone, Debug, Default)]
pub struct WireBudgetParts {
    pub system_bytes: u64,
    pub message_bytes: u64,
    pub tool_schema_bytes: u64,
    pub attachment_bytes: u64,
    pub wire_body_bytes: u64,
}

impl WireBudgetParts {
    /// 分项之和（附件已单列，不与消息部分重复）。
    pub fn parts_total(&self) -> u64 {
        self.system_bytes
            .saturating_add(self.message_bytes)
            .saturating_add(self.tool_schema_bytes)
            .saturating_add(self.attachment_bytes)
    }

    /// provider 包装/头部开销：wire 体减去可识别分项；分项更大时取 0。
    pub fn provider_framing_bytes(&self) -> u64 {
        self.wire_body_bytes.saturating_sub(self.parts_total())
    }
}

/// 用最终 wire 体与有效预算配置构造一次性 wire 预算。
///
/// 输出预留与输出 cap 取自同一份有效 [`kiana_domain::TokenBudget`]（`reserved_output` /
/// `limit`）；输入会计使用显式标记的保守回退 [`CONSERVATIVE_WIRE_ACCOUNTING`]，不声称
/// provider tokenizer 精度。超限时返回 `wire_budget_exceeded`，调用方必须在发送前拒绝。
pub fn compile_wire_budget(
    prepared: &PreparedModelCall,
) -> Result<(WireBudget, WireBudgetParts), String> {
    let parts = wire_budget_parts(prepared)?;
    let budget = WireBudget::from_input(WireBudgetInput {
        system_bytes: parts.system_bytes,
        message_bytes: parts.message_bytes,
        tool_schema_bytes: parts.tool_schema_bytes,
        attachment_bytes: parts.attachment_bytes,
        provider_framing_bytes: parts.provider_framing_bytes(),
        output_reserved_tokens: prepared.budget.reserved_output,
        output_cap_tokens: prepared.budget.limit,
        token_limit: prepared.budget.limit,
        accounting: CONSERVATIVE_WIRE_ACCOUNTING.clone(),
    })?;
    Ok((budget, parts))
}

/// 解释用的预算分项观测（顺序稳定；附件引用已在消息字节之外单列）。
pub fn wire_budget_part_items(parts: &WireBudgetParts) -> Vec<StepBudgetPart> {
    vec![
        StepBudgetPart {
            item: "system".to_owned(),
            bytes: parts.system_bytes,
        },
        StepBudgetPart {
            item: "history_content_blocks".to_owned(),
            bytes: parts.message_bytes,
        },
        StepBudgetPart {
            item: "tool_schemas".to_owned(),
            bytes: parts.tool_schema_bytes,
        },
        StepBudgetPart {
            item: "attachments".to_owned(),
            bytes: parts.attachment_bytes,
        },
        StepBudgetPart {
            item: "provider_framing".to_owned(),
            bytes: parts.provider_framing_bytes(),
        },
    ]
}

fn split_message_attachments(message: &ModelMessage) -> (ModelMessage, Vec<ModelContent>) {
    if message.content.is_empty() {
        return (message.clone(), Vec::new());
    }
    let attachments = message
        .content
        .iter()
        .filter(|block| matches!(block, ModelContent::AttachmentRef { .. }))
        .cloned()
        .collect::<Vec<_>>();
    if attachments.is_empty() {
        return (message.clone(), Vec::new());
    }
    let mut rest = message.clone();
    rest.content = message
        .content
        .iter()
        .filter(|block| !matches!(block, ModelContent::AttachmentRef { .. }))
        .cloned()
        .collect();
    (rest, attachments)
}

fn serialized_len<T: Serialize>(value: &T) -> Result<u64, String> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() as u64)
        .map_err(|_| "step_context_wire_parts_invalid".to_owned())
}

fn serialized_value_len(value: &Value) -> Result<u64, String> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() as u64)
        .map_err(|_| "step_context_wire_parts_invalid".to_owned())
}

/// 在真正预留预算/发送之前复核冻结绑定的漂移。
///
/// 路由摘要与「编译时的钉住摘要」不一致时返回 `step_context_fence:resolved_context_route_changed`；
/// catalog/scope/workspace/data 漂移返回各自的 domain 稳定错误码。错误文本带
/// `step_context_fence:` 前缀，便于与普通校验失败区分。
pub fn recheck_step_context_bindings(
    resolved: &ResolvedStepContext,
    route_digest: &str,
    catalog_digest: &str,
    messages: &[ModelMessage],
    sandbox: &str,
) -> Result<(), String> {
    resolved
        .recheck_bindings(
            route_digest,
            catalog_digest,
            &ScopeSet::unrestricted().scope_digest,
            derive_workspace_binding(messages),
            derive_data_binding(&resolved.prompt_bundle_digest, sandbox),
        )
        .map_err(|error| format!("step_context_fence:{error}"))
}

/// 由消息快照派生 workspace 绑定。
///
/// 这是内容派生的本地绑定（同一快照得到同一值，内容变化即失效），不是 EventStore 的
/// workspace revision；跨进程权威校验仍由 Core 负责。
pub fn derive_workspace_binding(messages: &[ModelMessage]) -> u64 {
    binding_from_digest(&json_digest(
        &serde_json::to_value(messages).unwrap_or(Value::Null),
    ))
}

/// 由 PromptBundle 摘要与 sandbox 派生 data 绑定。
///
/// 同样是内容派生的本地绑定，不是 EventStore 的 data epoch；这里不声称任何删除/撤销
/// 传播已经完成，只保证同一输入产生同一值、输入变化即失效。
pub fn derive_data_binding(prompt_bundle_digest: &str, sandbox: &str) -> u64 {
    binding_from_digest(&json_digest(&json!({
        "prompt_bundle_digest": prompt_bundle_digest,
        "sandbox": sandbox,
    })))
}

fn binding_from_digest(digest: &str) -> u64 {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    let prefix = &hex[..hex.len().min(16)];
    u64::from_str_radix(prefix, 16).unwrap_or(1).max(1)
}

/// 把消息历史映射为不可信 Context 候选；产品段已经由 bundle 单独进入计划。
///
/// - `User` → Task / `UserImport`（任务与 steering 文本）
/// - `Assistant` → History / `Event`（已产生的助手文本）
/// - `Tool` → LiveResult / `Event`（Broker 观测；永远不是执行授权）
///
/// System 消息一律跳过（bundle 编码或产品提示已经由 Prompt 段代表），与
/// `bundle.system_prompt()` / `bundle.context_prompt()` 完全相同的正文也不重复采纳。
/// 只取最近 [`STEP_CONTEXT_MAX_MESSAGE_CANDIDATES`] 条消息，避免无界计划条目。
fn message_candidates(
    bundle: &PromptBundle,
    messages: &[ModelMessage],
) -> Result<Vec<ContextCandidate>, String> {
    let system_prompt = bundle.system_prompt();
    let context_prompt = bundle.context_prompt();
    let start = messages
        .len()
        .saturating_sub(STEP_CONTEXT_MAX_MESSAGE_CANDIDATES);
    let mut candidates = Vec::new();
    for (index, message) in messages.iter().enumerate().skip(start) {
        let (material_type, source_kind) = match message.role {
            ModelRole::System => continue,
            ModelRole::User => (ContextMaterialType::Task, SourceKind::UserImport),
            ModelRole::Assistant => (ContextMaterialType::History, SourceKind::Event),
            ModelRole::Tool => (ContextMaterialType::LiveResult, SourceKind::Event),
        };
        if message.text.trim().is_empty() {
            continue;
        }
        if (!system_prompt.is_empty() && message.text == system_prompt)
            || (!context_prompt.is_empty() && message.text == context_prompt)
        {
            continue;
        }
        let source = SourceRef::new(
            format!("message:{index}"),
            source_kind,
            format!("run-message://{index}"),
            format!("message-index:{index}"),
            json_digest(&json!({"role": &message.role, "text": &message.text})),
            None,
            EvidenceStatus::Attributed,
        )?;
        candidates.push(ContextCandidate {
            name: format!("message-{index}"),
            text: message.text.clone(),
            source,
            authority: PromptAuthority::Context,
            material_type,
            permission_scope: "run-context".to_owned(),
            revision: format!("message-index:{index}"),
            priority: 1_000u32.saturating_add(index as u32),
        });
    }
    Ok(candidates)
}

/// 为每个被采纳的计划条目生成来源快照；未采纳条目不进入发送内容，因此不生成快照。
fn included_source_snapshots(plan: &ContextPlan) -> Result<Vec<SourceSnapshot>, String> {
    plan.items
        .iter()
        .filter(|item| item.included)
        .map(|item| {
            SourceSnapshot::new(
                item.source.clone(),
                Freshness::Current,
                item.source.evidence,
                None,
            )
        })
        .collect()
}
