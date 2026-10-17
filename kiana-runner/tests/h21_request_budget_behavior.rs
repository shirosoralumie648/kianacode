//! H21 行为夹具：真实请求预算与稳定缓存前缀。
//!
//! 夹具只驱动进程内 fake `ModelClient`：不访问网络、不执行工具、不构造 DaemonHost。
//! 工具结果通过协议命令 `CapabilityResult` 回灌，保持与产品主路径相同的 Runner
//! 挂起/恢复数据流（本测试扮演 Core/Broker 的调用方角色）。
//!
//! 三条夹具对应卡片命名验收，拒绝路径在前：
//! - `h21_huge_schema_or_system_prompt_exhausts_request_budget`（先拒绝）
//! - `h21_cache_hit_cannot_bypass_data_revocation`（先拒绝）
//! - `h21_context_fits_actual_wire_budget_and_preserves_stable_prefix`（再成功）
//!
//! 预算断言只针对本夹具选定的输入与适配器自报的有效窗口/用量：不声称任何 provider 的
//! tokenizer 精度、缓存命中率或节省比例，也不把本地保守估计当作任意模型的硬上界。

use async_trait::async_trait;
use kiana_domain::{
    json_digest, tool_schemas, ModelCallSpec, ModelError, ModelMessage, ModelOutput, ModelProtocol,
    ModelRequest, ModelRequestContext, ModelRoute, ModelToolCall, ModelUsage, PreparedModelCall,
    PromptAuthority, PromptBundle, PromptSection, RoleSpec, RunId, StablePrefix, StepId,
    StepIdentity, TokenAccounting, TurnId, WireBudget, MODEL_CALL_SCHEMA,
    REQUEST_BUDGET_COVERAGE_ITEMS,
};
use kiana_runner::{
    compile_step_context_with_prefix, compile_wire_budget_with_accounting,
    decide_stable_prefix_reuse, KianaHarness, ModelClient, StablePrefixReuse,
    StepContextExplanation, StepContextInputs, STEP_CONTEXT_EXPLANATION_SCHEMA,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// 适配器夹具：按步给出有效窗口与实际编译出的工具面，并记录 prepared / 真正发送的请求。
///
/// `tight_window` 表示适配器自报的窗口只比自身字节预算多 512：同一份有效配置既给出输出
/// 预留也给出上限。`tool_plan` 每一步可为 `None`（原样沿用 harness 请求的工具面）或一份
/// 收窄后的工具面（模拟适配器在该步编译出的有效目录变化）。
struct BudgetProbeModel {
    outputs: Mutex<Vec<ModelOutput>>,
    route: ModelRoute,
    tight_window: bool,
    tool_plan: Mutex<Vec<Option<Vec<Value>>>>,
    prepared_requests: Mutex<Vec<ModelRequest>>,
    prepared_calls: Mutex<Vec<PreparedModelCall>>,
    sent_requests: Mutex<Vec<ModelRequest>>,
}

impl BudgetProbeModel {
    fn new(outputs: Vec<ModelOutput>) -> Self {
        Self {
            outputs: Mutex::new(outputs),
            route: fixture_route(),
            tight_window: false,
            tool_plan: Mutex::new(Vec::new()),
            prepared_requests: Mutex::new(Vec::new()),
            prepared_calls: Mutex::new(Vec::new()),
            sent_requests: Mutex::new(Vec::new()),
        }
    }

    fn with_tight_window(outputs: Vec<ModelOutput>) -> Self {
        let mut model = Self::new(outputs);
        model.tight_window = true;
        model
    }

    fn set_tool_plan(&self, plan: Vec<Option<Vec<Value>>>) {
        *self.tool_plan.lock().expect("h21 fixture tool plan lock") = plan;
    }
}

#[async_trait]
impl ModelClient for BudgetProbeModel {
    fn request_context(&self, request: &ModelRequest) -> ModelRequestContext {
        let mut context = ModelRequestContext::for_request(request);
        if self.tight_window {
            let total = context.budget().total;
            context.context_window = total + 512;
        }
        context
    }

    fn prepare_call(
        &self,
        mut request: ModelRequest,
        spec: ModelCallSpec,
    ) -> Result<PreparedModelCall, ModelError> {
        {
            let mut plan = self
                .tool_plan
                .lock()
                .map_err(|_| ModelError::invalid("h21_fixture_lock_poisoned"))?;
            if !plan.is_empty() {
                let planned = plan.remove(0);
                if let Some(tools) = planned {
                    request.tools = tools;
                }
            }
        }
        let context = self.request_context(&request);
        let wire_body = serde_json::to_value(&request)
            .map_err(|_| ModelError::invalid("model_request_invalid"))?;
        let mut prepared = PreparedModelCall {
            schema: MODEL_CALL_SCHEMA.to_owned(),
            spec,
            route: self.route.clone(),
            request: request.clone(),
            wire_body,
            request_hash: String::new(),
            budget: context.budget(),
            tool_catalog_hash: kiana_domain::tool_catalog_hash(&request.tools),
            provider_account: None,
            credential_revision: None,
        };
        prepared.seal();
        prepared.validate()?;
        self.prepared_requests
            .lock()
            .map_err(|_| ModelError::invalid("h21_fixture_lock_poisoned"))?
            .push(request);
        self.prepared_calls
            .lock()
            .map_err(|_| ModelError::invalid("h21_fixture_lock_poisoned"))?
            .push(prepared.clone());
        Ok(prepared)
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        self.sent_requests
            .lock()
            .map_err(|_| "h21_fixture_lock_poisoned".to_owned())?
            .push(request);
        let mut outputs = self
            .outputs
            .lock()
            .map_err(|_| "h21_fixture_lock_poisoned".to_owned())?;
        if outputs.is_empty() {
            return Err("harness_script_exhausted".to_owned());
        }
        Ok(outputs.remove(0))
    }
}

fn fixture_route() -> ModelRoute {
    ModelRoute {
        provider_id: "h21-fixture".to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: "h21-fixture-connection".to_owned(),
        model_id: "h21-fixture-model".to_owned(),
        profile: "h21-fixture-profile".to_owned(),
        configuration_revision: "h21.fixture.v1".to_owned(),
        streaming: false,
    }
}

fn fixture_bundle(extra: Option<PromptSection>) -> PromptBundle {
    let mut bundle = PromptBundle::for_role(&RoleSpec::builder());
    if let Some(section) = extra {
        bundle.sections.push(section);
    }
    bundle.validate().expect("fixture bundle is valid");
    bundle
}

fn tool_call_output(text: &str) -> ModelOutput {
    ModelOutput {
        text: text.to_owned(),
        tool_calls: vec![ModelToolCall {
            id: "h21-call-1".to_owned(),
            name: "memory.search".to_owned(),
            arguments: json!({"query": "h21 request budget"}),
        }],
        ..ModelOutput::default()
    }
}

fn expect_completed(events: &[RunnerEvent]) {
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })),
        "expected a Completed event"
    );
}

fn model_turn_metadata(events: &[RunnerEvent], step: u32) -> Value {
    events
        .iter()
        .find_map(|event| match event {
            RunnerEvent::ModelTurn {
                step: event_step,
                metadata,
                ..
            } if *event_step == step => Some(metadata.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("model turn metadata for step {step}"))
}

fn recorded_explanation(metadata: &Value) -> StepContextExplanation {
    let explanation: StepContextExplanation = serde_json::from_value(
        metadata
            .get("step_context")
            .cloned()
            .expect("model turn metadata carries step_context"),
    )
    .expect("step_context explanation decodes");
    assert_eq!(explanation.schema, STEP_CONTEXT_EXPLANATION_SCHEMA);
    explanation
}

/// 用 runner 公开的单步编译入口生成一份稳定前缀；夹具用它比较「同一输入」与
/// 「数据删除/内容撤销后的输入」两种缓存身份。
fn compiled_prefix(bundle: &PromptBundle, sandbox: &str) -> StablePrefix {
    let messages = vec![ModelMessage::user("explain the request budget")];
    let run_id = RunId::new();
    let turn_id = TurnId::from_uuid(run_id.as_uuid());
    let step_identity =
        StepIdentity::new(run_id, turn_id, StepId::new(), 1).expect("fixture step identity");
    let wire_budget = WireBudget::from_final_wire(
        4_096,
        256,
        512,
        1_000_000,
        TokenAccounting::Exact {
            input_tokens: 1_024,
        },
    )
    .expect("fixture wire budget");
    let (_, prefix) = compile_step_context_with_prefix(StepContextInputs {
        bundle,
        messages: &messages,
        sandbox,
        step_identity,
        model_profile: bundle.model_profile.clone(),
        route_digest: json_digest(&json!({"route": "h21-fixture-route"})),
        catalog_digest: json_digest(&json!({"catalog": "h21-fixture-catalog"})),
        wire_budget,
    })
    .expect("fixture step context compiles");
    prefix
}

fn start_command(run_id: RunId, prompt: &str, root: &str, bundle: &PromptBundle) -> RunnerCommand {
    RunnerCommand::start_in_with_instructions(
        run_id,
        prompt,
        root,
        "read-only",
        bundle.encode().expect("fixture bundle encodes"),
        true,
    )
}

/// 先拒绝：巨大的 system/role 产品段 + 只留 512 余量的适配器窗口必须在发送前拒绝，
/// 不能把超预算请求交给 provider。
#[tokio::test]
async fn h21_huge_schema_or_system_prompt_exhausts_request_budget() {
    let huge = "H21 huge system prompt boundary text. ".repeat(2_048);
    let bundle = fixture_bundle(Some(PromptSection {
        name: "role_policy".to_owned(),
        order: 200,
        text: huge,
        source: "role:h21-fixture".to_owned(),
        authority: PromptAuthority::Product,
    }));
    let model = Arc::new(BudgetProbeModel::with_tight_window(vec![
        ModelOutput::text("must never be sent"),
    ]));
    let harness = KianaHarness::new(model.clone());
    let events = harness
        .send(start_command(
            RunId::new(),
            "budget boundary",
            "/h21-huge",
            &bundle,
        ))
        .await
        .expect("start command");

    assert!(
        events.iter().any(|event| matches!(
            event,
            RunnerEvent::Failed { error, .. } if error == "step_context_wire_budget:wire_budget_exceeded"
        )),
        "an over-budget request must fail with the explicit wire budget reason"
    );
    assert!(
        model.sent_requests.lock().unwrap().is_empty(),
        "the rejected request never reaches the provider"
    );
    assert_eq!(
        model.prepared_requests.lock().unwrap().len(),
        1,
        "the request was frozen first and then rejected before sending"
    );
}

/// 先拒绝：权限收紧（有效工具目录变化 → authority epoch 变化）或数据删除/内容撤销
/// （bundle 摘要变化 → data epoch 变化）之后不能复用旧前缀，即使可见稳定段完全相同。
#[tokio::test]
async fn h21_cache_hit_cannot_bypass_data_revocation() {
    // 路径 A：同一 run 的第二步在工具面收窄后不得复用第一步的前缀；harness 记录稳定原因。
    let canonical = tool_schemas();
    let narrowed = canonical[1..].to_vec();
    let bundle = fixture_bundle(None);
    let model = Arc::new(BudgetProbeModel::new(vec![
        tool_call_output("searching"),
        ModelOutput::text("finished"),
    ]));
    model.set_tool_plan(vec![None, Some(narrowed)]);
    let harness = KianaHarness::new(model.clone());
    let run_id = RunId::new();
    let started = harness
        .send(start_command(run_id, "revocation", "/h21-revoke", &bundle))
        .await
        .expect("start command");
    let request_id = started
        .iter()
        .find_map(|event| match event {
            RunnerEvent::CapabilityRequested { request, .. } => Some(request.request_id),
            _ => None,
        })
        .expect("first step requests exactly one capability");
    let resumed = harness
        .send(RunnerCommand::CapabilityResult {
            run_id,
            result: kiana_domain::CapabilityResult::success(request_id, json!({"ok": true})),
        })
        .await
        .expect("capability result command");
    expect_completed(&resumed);

    let step_one = recorded_explanation(&model_turn_metadata(&started, 1));
    let step_two = recorded_explanation(&model_turn_metadata(&resumed, 2));
    let first = step_one
        .stable_prefix
        .clone()
        .expect("first step records its stable prefix");
    let second = step_two
        .stable_prefix
        .clone()
        .expect("second step records its stable prefix");
    assert_eq!(
        step_one.stable_prefix_reuse, None,
        "the first observation has no previous cache entry"
    );
    assert_eq!(
        first.segments, second.segments,
        "tool narrowing does not change the visible product segments"
    );
    assert_ne!(
        first.catalog_digest, second.catalog_digest,
        "the adapter compiled a different effective tool catalog"
    );
    assert_ne!(
        first.cache_key_digest, second.cache_key_digest,
        "a changed catalog yields a different cache key"
    );
    assert_eq!(
        step_two.stable_prefix_reuse,
        Some(StablePrefixReuse {
            reused: false,
            reason: Some("stable_prefix_authority_revoked".to_owned()),
        }),
        "permission tightening is recorded as a refusal, not a cache hit"
    );
    assert_eq!(
        model.sent_requests.lock().unwrap().len(),
        2,
        "the narrowed step still runs; only cache reuse was refused"
    );

    // 路径 B：数据删除/内容撤销后 data epoch 变化，同一决策函数拒绝复用；可见稳定段相同。
    let before = compiled_prefix(&fixture_bundle(None), "read-only");
    let after = compiled_prefix(
        &fixture_bundle(Some(PromptSection {
            name: "project_instruction".to_owned(),
            order: 400,
            text: "revoked source material: keep only the current revision".to_owned(),
            source: "project:.kiana/h21-revoked.md".to_owned(),
            authority: PromptAuthority::Context,
        })),
        "read-only",
    );
    assert_eq!(
        before.prefix_digest, after.prefix_digest,
        "the visible stable segments are unchanged"
    );
    assert_ne!(
        before.data_epoch, after.data_epoch,
        "the deleted/changed source moves the data epoch"
    );
    assert_eq!(
        decide_stable_prefix_reuse(Some(&before), &after),
        Some(StablePrefixReuse {
            reused: false,
            reason: Some("stable_prefix_data_epoch_changed".to_owned()),
        })
    );

    // 成功对照：同一份输入重编译得到同一 cache key，可以复用同一前缀。
    let before_again = compiled_prefix(&fixture_bundle(None), "read-only");
    assert_eq!(before.cache_key_digest, before_again.cache_key_digest);
    assert_eq!(
        decide_stable_prefix_reuse(Some(&before), &before_again),
        Some(StablePrefixReuse {
            reused: true,
            reason: None,
        })
    );
}

/// 再成功：同一有效配置下请求在 wire 预算内，稳定前缀保持固定顺序与缓存身份，
/// 估算/实际用量差值可解释。
#[tokio::test]
async fn h21_context_fits_actual_wire_budget_and_preserves_stable_prefix() {
    let bundle = fixture_bundle(None);
    let model = Arc::new(BudgetProbeModel::new(vec![
        tool_call_output("searching"),
        ModelOutput {
            text: "budget answered".to_owned(),
            usage: Some(ModelUsage {
                input_tokens: 64,
                output_tokens: 32,
            }),
            ..ModelOutput::default()
        },
    ]));
    let harness = KianaHarness::new(model.clone());
    let run_id = RunId::new();
    let started = harness
        .send(start_command(run_id, "budget fit", "/h21-fit", &bundle))
        .await
        .expect("start command");
    let request_id = started
        .iter()
        .find_map(|event| match event {
            RunnerEvent::CapabilityRequested { request, .. } => Some(request.request_id),
            _ => None,
        })
        .expect("first step requests exactly one capability");
    let resumed = harness
        .send(RunnerCommand::CapabilityResult {
            run_id,
            result: kiana_domain::CapabilityResult::success(request_id, json!({"ok": true})),
        })
        .await
        .expect("capability result command");
    expect_completed(&resumed);

    let sent = model.sent_requests.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    let canonical_tools = tool_schemas();
    assert_eq!(
        sent[0].tools, canonical_tools,
        "the tool order is the fixed catalog order"
    );
    assert_eq!(
        sent[1].tools, canonical_tools,
        "the tool order does not depend on the step"
    );

    let coverage = REQUEST_BUDGET_COVERAGE_ITEMS.map(str::to_owned).to_vec();
    let mut steps = Vec::new();
    for (step, events) in [(1u32, &started), (2u32, &resumed)] {
        let explanation = recorded_explanation(&model_turn_metadata(events, step));
        assert_eq!(
            explanation.budget_coverage, coverage,
            "the budget declares its fixed coverage items"
        );
        assert!(
            explanation.wire_total_reserved_tokens <= explanation.token_limit,
            "the reserved input+output stays within the same effective config"
        );
        assert_eq!(
            explanation.wire_input_bytes, explanation.wire_body_bytes,
            "the observed wire input is the body actually compiled for sending"
        );
        let part_names = explanation
            .budget_parts
            .iter()
            .map(|part| part.item.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            [
                "system",
                "history_content_blocks",
                "tool_schemas",
                "attachments",
                "provider_framing",
            ]
        );
        let parts_sum: u64 = explanation.budget_parts.iter().map(|part| part.bytes).sum();
        assert_eq!(
            parts_sum, explanation.wire_body_bytes,
            "budget parts plus provider framing partition the wire body"
        );
        let sent_bytes = serde_json::to_vec(&serde_json::to_value(&sent[(step - 1) as usize]))
            .unwrap()
            .len() as u64;
        assert_eq!(
            explanation.wire_body_bytes, sent_bytes,
            "the recorded wire bytes match the request that was sent"
        );
        steps.push(explanation);
    }
    let (first, second) = (&steps[0], &steps[1]);

    // 稳定前缀：固定顺序、同一缓存身份；动态历史只进后缀。
    let first_prefix = first
        .stable_prefix
        .as_ref()
        .expect("first step records its stable prefix");
    let second_prefix = second
        .stable_prefix
        .as_ref()
        .expect("second step records its stable prefix");
    let mut expected_segments = bundle
        .sections
        .iter()
        .filter(|section| section.authority == PromptAuthority::Product)
        .map(|section| (section.order, section.name.clone()))
        .collect::<Vec<_>>();
    expected_segments.sort();
    assert_eq!(
        first_prefix
            .segments
            .iter()
            .map(|segment| (segment.order, segment.name.clone()))
            .collect::<Vec<_>>(),
        expected_segments
    );
    assert_eq!(first_prefix.segments, second_prefix.segments);
    assert_eq!(
        first_prefix.cache_key_digest, second_prefix.cache_key_digest,
        "the cache key ignores dynamic history"
    );
    assert_eq!(first_prefix.profile, bundle.model_profile);
    assert_ne!(
        first_prefix.dynamic_suffix_digest, second_prefix.dynamic_suffix_digest,
        "history material only moves the dynamic suffix"
    );
    assert_eq!(first.stable_prefix_reuse, None);
    assert_eq!(
        second.stable_prefix_reuse,
        Some(StablePrefixReuse {
            reused: true,
            reason: None,
        })
    );

    // 用量差值：第一步适配器未回报 usage（Unknown，绝不填 0）；第二步回报后逐项可解释。
    let first_delta = first
        .usage_delta
        .as_ref()
        .expect("usage delta is recorded for every sent step");
    assert!(!first_delta.usage_complete);
    assert_eq!(first_delta.actual_input_tokens, None);
    assert_eq!(first_delta.input_delta_tokens, None);
    let second_delta = second
        .usage_delta
        .as_ref()
        .expect("usage delta is recorded for every sent step");
    assert!(second_delta.usage_complete);
    assert_eq!(second_delta.actual_input_tokens, Some(64));
    assert_eq!(second_delta.actual_output_tokens, Some(32));
    assert_eq!(
        second_delta.input_delta_tokens,
        Some(64i64 - second.wire_estimated_input_tokens as i64)
    );
    assert_eq!(
        second_delta.output_delta_tokens,
        Some(32i64 - second.output_reserved_tokens as i64)
    );
    assert!(
        second.wire_estimated_input_tokens >= 64,
        "the explicit conservative estimate is above the fixture's reported usage"
    );

    // 精确计数入口：适配器能给出 tokenizer 摘要时，runner 用同一份冻结请求按声明计数重建
    // wire 预算；计数超限同样在发送前拒绝。这里只验证 runner 侧口径与预留绑定，不声称
    // 任何真实 provider 的 tokenizer 精度。
    let prepared = model.prepared_calls.lock().unwrap()[1].clone();
    let tokenizer_digest = json_digest(&json!({"tokenizer": "h21-fixture-v1"}));
    let (exact, parts) = compile_wire_budget_with_accounting(
        &prepared,
        TokenAccounting::ExactTokenizer {
            input_tokens: 512,
            tokenizer_digest: tokenizer_digest.clone(),
        },
    )
    .expect("exact accounting compiles for the frozen request");
    assert!(matches!(
        exact.accounting,
        TokenAccounting::ExactTokenizer { .. }
    ));
    assert_eq!(exact.estimated_input_tokens, 512);
    assert_eq!(exact.output_reserved_tokens, second.output_reserved_tokens);
    assert_eq!(exact.token_limit, second.token_limit);
    assert_eq!(parts.wire_body_bytes, second.wire_body_bytes);
    assert_eq!(
        compile_wire_budget_with_accounting(
            &prepared,
            TokenAccounting::ExactTokenizer {
                input_tokens: second.token_limit,
                tokenizer_digest,
            },
        )
        .unwrap_err(),
        "wire_budget_exceeded"
    );
}
