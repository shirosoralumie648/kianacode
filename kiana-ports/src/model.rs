//! Runner 使用的异步模型客户端端口。
//!
//! # 这个文件在系统里的位置
//!
//! 这是 Kiana 与「模型本身」之间唯一的接口。Kiana 不自己实现模型推理，
//! 而是通过本端口把请求交给某个具体的模型客户端，再收回结果。
//!
//! ```text
//! kiana-runner::KianaHarness        规范 Agent 循环：组装消息、决定何时调模型
//!        ↓  构造 ModelRequest（消息 + 工具清单 + 预算）
//!        ↓  调用冻结方法确定请求
//! 【本文件：ModelClient trait】    ← 端口，只声明接口
//!        ↓
//!    ProviderGateway（kiana-provider）      ← 生产实现
//!    ScriptedModel / UnavailableModel        ← 测试替身
//!        ↓
//!    真实模型服务 / 本地脚本
//! ```
//!
//! # 关键设计：调用被拆成「冻结」和「执行」两步
//!
//! 初学者最容易困惑的地方：为什么不直接一个方法把请求发出去？
//!
//! 因为 Kiana 要求**每一次模型调用都先经过授权和预算检查**。两步拆法是：
//!
//! ```text
//! prepare_call()      freeze：只做本地计算，不发网络请求
//!   ├─ 算出请求上下文（模型名、预算）
//!   ├─ 序列化请求体
//!   ├─ 计算工具清单哈希
//!   └─ 封存 + 自检
//!        ↓
//!   【控制面在这里签发调用许可 ModelCallPermit】
//!        ↓
//! 已准入方法          execute：带上许可，真正执行
//!   ├─ 消费预算
//!   └─ 真正调用模型
//! ```
//!
//! 分开的好处是：**在"准备好"和"真的花钱"之间，
//! 有一个控制面可以插进来做决策的缝隙。**
//! 如果只有一个方法，授权就只能发生在方法内部，
//! 而调用方无从得知这次调用到底是否已被批准。
//!
//! ⚠ 还有一个容易踩的坑：执行阶段的方法注释里写死了 "Exactly one attempt"
//! （恰好一次尝试）。**重试不属于模型客户端。**
//! 重试由上层已获准的 attempt driver 负责。
//! 如果模型客户端自己偷偷重试，一次用户请求可能消耗数倍预算，
//! 而控制面的预算账本只记了一次 —— 账实不符，审计对不上。
//!
//! # 术语
//!
//! - **prepare（冻结/准备）**：把一次调用所需的全部输入确定下来，
//!   算出不可变的调用描述，但**不发出任何请求**。
//! - **admission（准入）**：控制面批准这次调用并发放预算的过程。
//! - **permit（许可）**：准入凭证，类型 [`ModelCallPermit`]。没有它无法执行已冻结的调用。
//! - **sealed（封存）**：给调用对象打上"此后不可修改"的标记。封存后再改内容，校验会失败。
//! - **delta（增量）**：流式场景下逐步产生的一小片结果，比如一小段文本。
//! - **backpressure（背压）**：下游消费不过来时，向上游施加的减速压力。
//!
//! # 上游契约
//!
//! 实现只负责将消息交给模型并返回声明；它不得直接执行工具或修改工作区。

use async_trait::async_trait;
use kiana_domain::*;

#[async_trait]
/// Runner 使用的异步模型客户端端口。
///
/// 【作用】
/// 定义 Kiana 怎样"和模型说话"。实现者负责把 [`ModelRequest`] 交给某个真实的模型，
/// 把结果翻译回 [`ModelOutput`]。
///
/// 【实现者一览】
/// - **生产实现**：`kiana-provider/src/lib.rs` 的 `ProviderGateway` —— 真正访问上游服务商。
/// - **测试替身**：`kiana-runner/src/model.rs` 的 `ScriptedModel`（按脚本回放）、
///   `UnavailableModel`（永远失败）；`harness.rs` 内联 5 个仅用于测试的实现。
/// - **集成测试替身**：例如 `kiana-runner/tests/h19_steer_inject.rs` 的 `StreamingSteerModel`。
///
/// 【调用者】
/// 主要调用者是 `kiana-runner::KianaHarness`（规范 Agent 循环）：
/// 在 `harness.rs` 里先冻结构造一次调用，拿到控制面签发的许可后再执行。
///
/// 【全局不变量 —— 本文件最重要的一条】
/// **实现者绝对不能直接执行工具或修改工作区。**
///
/// 这一点值得反复强调：模型可能会"请求"调用某个工具，
/// 但模型客户端的职责仅仅是**把请求报回去**。
/// 工具真正执行要走完全不同的路径 —— 转成能力请求（CapabilityRequest），
/// 回到控制面过审，再由能力代理（Broker）执行。
///
/// 如果模型客户端能自己执行工具，整个授权层就被绕过了 ——
/// 而"每次执行都经过授权"正是 Kiana 存在的核心理由。
pub trait ModelClient: Send + Sync {
    /// Compile once; only the frozen request may be admitted and sent.
    ///
    /// 【作用 —— 冻结阶段】
    /// 把一次模型调用**确定下来**：计算所有派生信息，产出不可变的
    /// [`PreparedModelCall`]。**这个方法不发任何网络请求。**
    ///
    /// 【输入】
    /// - `request`：消息内容 + 可用工具清单 + 预算上限。
    /// - `spec`：这次调用的规格（超时、响应格式、截止时间等）。
    ///
    /// 【输出】
    /// 成功返回 [`PreparedModelCall`]，它包含：
    /// - 冻结的请求体（`wire_body`，已序列化成 JSON 值）；
    /// - 路由信息（`route`：用哪个服务商、哪个模型、是否流式）；
    /// - 预算快照（`budget`）；
    /// - 工具清单哈希（`tool_catalog_hash`）—— 证明"冻结的工具集没被中途换过"；
    /// - 请求哈希（`request_hash`）。
    ///
    /// 【核心流程】
    /// 1. 调 `request_context()` 算出上下文（含模型名和预算）；
    /// 2. 组装 `PreparedModelCall`；
    /// 3. 封存 —— 此后内容不可修改；
    /// 4. 自检 —— 封存后立刻验证，不合格就地失败。
    ///
    /// 【⚠ 为什么必须"先封存再自检"，顺序不能反】
    /// 封存（seal）的意义是"盖上不可变的章"。如果先自检再封存，
    /// 两者之间就存在一个窗口：校验通过后、封存前，内容仍可被改。
    /// 先封存再自检，校验的就是最终形态，没有缝隙。
    ///
    /// 【关于路由默认值填 `"legacy"`】
    /// 这个默认实现是给**老式/离线客户端**兜底的。路由被填成
    /// provider 为 "legacy"、协议为 `Legacy`、模型名回退到 "scripted"。
    /// 换句话说：一个没有实现自己冻结逻辑的客户端，会得到一个明确标记为
    /// "我是 legacy"的调用，而不是被悄悄路由到某个真实服务商。
    ///
    /// ⚠ 关于 `"scripted"` 这个回退：模型名为空时用 "scripted" 顶上。
    /// 这不是随意选的默认值 —— 它让脚本化测试在省略模型名时仍能跑通。
    /// 但也意味着**如果生产环境的请求缺了模型名，会被当成脚本请求处理**。
    /// 上游必须保证模型名总是有值。
    ///
    /// 【失败情况】
    /// 序列化失败 → `ModelError::invalid("model_request_invalid")`。
    /// 封存后自检不通过 → 由 `validate()` 返回具体原因。
    ///
    /// 【副作用】
    /// 无网络、无文件写入。纯本地计算。
    fn prepare_call(
        &self,
        request: ModelRequest,
        spec: ModelCallSpec,
    ) -> Result<PreparedModelCall, ModelError> {
        let context = self.request_context(&request);
        let mut prepared = PreparedModelCall {
            schema: MODEL_CALL_SCHEMA.to_owned(),
            spec,
            route: ModelRoute {
                provider_id: "legacy".to_owned(),
                protocol: ModelProtocol::Legacy,
                connection_id: "in_process".to_owned(),
                model_id: context
                    .model_id
                    .clone()
                    .unwrap_or_else(|| "scripted".to_owned()),
                profile: "legacy".to_owned(),
                configuration_revision: "legacy.v1".to_owned(),
                streaming: false,
            },
            wire_body: serde_json::to_value(&request)
                .map_err(|_| ModelError::invalid("model_request_invalid"))?,
            budget: context.budget(),
            tool_catalog_hash: kiana_domain::tool_catalog_hash(&request.tools),
            request,
            request_hash: String::new(),
            provider_account: None,
            credential_revision: None,
        };
        prepared.seal();
        prepared.validate()?;
        Ok(prepared)
    }

    /// Exactly one attempt. Retries belong to the admitted Harness attempt driver.
    ///
    /// 【作用 —— 执行阶段】
    /// 执行一次**已经冻结**的模型调用。调用前会重新校验一次封存状态。
    ///
    /// 【输入】
    /// - `prepared`：冻结好的调用。
    /// - `on_delta`：流式增量回调，见文件头术语说明。
    ///
    /// 【输出】
    /// [`ModelReply`] —— 模型的结构化回复。
    ///
    /// 【失败情况】
    /// 流式调用失败 → 转为 `ModelError`。**不要伪造空的成功结果** ——
    /// 一个假的空回复会被上层当成"模型说了没什么"从而继续往下走，
    /// 掩盖真实的故障。
    ///
    /// 【关键约束：恰好一次】
    /// 注释原文写的是 "Exactly one attempt"。**这里不做重试。**
    /// 重试由上层（已获准的 attempt driver）负责。
    /// 原因：重试意味着额外消耗预算。如果模型客户端自己重试，
    /// 实际花费会是预算的数倍，而控制面的账本只记了一次 —— 预算失真，
    /// 审计也对不上。
    ///
    /// 【⚠ 不要改成内部重试】
    /// 这是本文件最容易被"顺手优化"的地方。看到网络失败就加个 for 循环重试，
    /// 看起来很自然，但会同时破坏预算记账和重试策略的可控性。
    ///
    /// 【为什么开头要再校验一次】
    /// 调用方理论上可以直接构造一个没封存、或封存后被改过的对象传进来。
    /// 这次校验是最后一道防线：确认手里的东西确实是冻结时那个。
    async fn complete_prepared(
        &self,
        prepared: PreparedModelCall,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        prepared.validate()?;
        let output = self
            .complete_streaming(prepared.request, on_delta)
            .await
            .map_err(ModelError::invalid)?;
        ModelReply::legacy(output)
    }

    /// 带准入许可执行调用 —— **先消费预算，再执行**。
    ///
    /// 【作用】
    /// 在上面那个执行方法的基础上，增加一道预算闸门。
    /// 没有 [`ModelCallPermit`]（调用许可），这次调用就不允许发出。
    ///
    /// 【核心流程】
    /// 1. 用许可调 `admission.consume_prepared()` 扣减预算；
    /// 2. 扣减成功后才执行调用。
    ///
    /// 【⚠ 为什么先扣预算再执行，顺序不能反】
    /// 因为**预算是硬约束，执行是副作用**。
    /// 如果先执行再扣预算，那么在"调用已发出、预算不足才发现"这个顺序下，
    /// 超额消耗已经发生了 —— 你只是事后知道了，但钱已经花出去。
    /// 先扣预算意味着预算不足时请求根本不会发出。
    ///
    /// 【失败情况】
    /// 预算不足或许可无效 → `consume_prepared` 报错，**请求不发出**。
    async fn complete_admitted(
        &self,
        prepared: PreparedModelCall,
        permit: ModelCallPermit,
        admission: &dyn crate::ModelBudgetPort,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        admission
            .consume_prepared(&prepared, &permit)
            .await
            .map_err(|e| ModelError::invalid(e.to_string()))?;
        self.complete_prepared(prepared, on_delta).await
    }

    /// Cancellation-aware model admission.  The default wraps the single admitted attempt so
    /// every provider implementation inherits the same fence; a provider may additionally pass
    /// the signal into its transport, but dropping the future is never interpreted as success.
    ///
    /// 【作用】
    /// 可被取消的已准入调用。给执行路径加一道**取消栅栏（cancellation fence）**。
    ///
    /// 【核心流程】
    /// 用 `tokio::select!` 在两件事之间赛跑：
    /// - 收到取消信号 → 立刻返回错误；
    /// - 调用正常完成 → 返回结果。
    ///
    /// 【⚠ `biased;` 是什么，为什么必须有】
    /// `biased;` 让 `select!` **按书写顺序**优先选择分支，而不是随机挑一个就绪的。
    /// 写在第一位的取消分支因此拥有优先权。
    ///
    /// 为什么这至关重要？如果不加 `biased`，当"取消信号已就绪"和"调用也刚好完成"
    /// 同时成立时，Tokio 会**随机**挑一个。结果就是同一个取消操作，
    /// 有时返回错误、有时返回成功 —— 行为不可复现，且无法写稳定的测试。
    /// 有了 `biased`，只要取消先到，就一定返回错误。
    ///
    /// 【⚠ 关键语义：丢弃 future 不等于成功】
    /// 注释里那句 "dropping the future is never interpreted as success"
    /// 是本方法最重要的约定。
    ///
    /// 取消时，`select!` 会**丢弃**仍在运行的那个 future。这个丢弃动作
    /// 本身不代表"调用没发生" —— 网络请求可能已经发出去了。
    /// 因此**绝不能**把取消当成干净的回滚。返回值必须是一个明确的错误
    /// （`model_cancelled_before_response`），让上层知道"结果未知"，
    /// 而不是"成功返回空"。
    ///
    /// `ModelRetryClass::Never` 的含义：这个错误**不允许重试**。
    /// 最后一个布尔参数 `false` 表示这不是传输层的临时故障。
    /// 取消后重试是危险的 —— 用户可能已经主动放弃了，重试只会白花预算。
    ///
    /// 【为什么每个 provider 都自动获得这道栅栏】
    /// 注释说 "every provider implementation inherits the same fence" ——
    /// 因为这是 trait 的**默认方法**，具体实现只要不覆盖它就自动获得取消能力。
    /// 这是一种很稳的设计：把安全属性放进默认实现，
    /// 新 provider 不会因为"忘记处理取消"而出问题。
    ///
    /// provider **可以**额外把取消信号传给自己的传输层（比如中断底层 HTTP 请求），
    /// 但那只是优化。上面的栅栏保证：即使传输层完全不响应取消，
    /// 方法本身也一定会返回错误。
    ///
    /// 【⚠ 不要把这个方法移到后面】
    /// 有一个 guard 测试（`kiana-runner/tests/h08_cancellation_guard.rs`）
    /// 用文本位置断言"取消栅栏必须定义在已准入路径之前"。
    /// 这个约束表达的是"栅栏包住执行路径"的结构关系，
    /// 编译器无法验证，所以测试改用源码文本位置来检查。
    /// 调整本文件中方法的排列顺序可能让该测试失败。
    async fn complete_admitted_cancellable(
        &self,
        prepared: PreparedModelCall,
        permit: ModelCallPermit,
        admission: &dyn crate::ModelBudgetPort,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
        mut cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<ModelReply, ModelError> {
        tokio::select! {
            biased;
            _ = crate::wait_for_cancellation(&mut cancellation) => {
                Err(ModelError::transport("model_cancelled_before_response", ModelRetryClass::Never, false))
            }
            result = self.complete_admitted(prepared, permit, admission, on_delta) => result,
        }
    }

    /// Cancellation-aware non-admitted model attempt for offline/legacy adapters.
    ///
    /// 【作用】
    /// 可取消的、**不需要预算许可**的调用。专门服务于离线/legacy 适配器
    /// （比如 `ScriptedModel` —— 它们不花钱，所以不需要许可）。
    ///
    /// 【与上面那个方法的区别】
    /// 唯一区别是**没有许可、没有预算扣减**。取消栅栏的逻辑完全一致。
    ///
    /// 【为什么分成两个方法，而不是加个 `Option` 参数】
    /// 保持两条路径的类型安全 —— 不需要许可的调用者手上不会有许可可传，
    /// 编译期就不可能误用。传 `None` 意味着两种意图挤在一个方法里，
    /// 每次调用都要分支判断，而类型系统帮不上忙。
    ///
    /// 【⚠ 同样遵守"丢弃 future 不等于成功"】
    /// 取消时的返回值和上面一致：明确的错误，且标记为不可重试。
    async fn complete_prepared_cancellable(
        &self,
        prepared: PreparedModelCall,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
        mut cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<ModelReply, ModelError> {
        tokio::select! {
            biased;
            _ = crate::wait_for_cancellation(&mut cancellation) => {
                Err(ModelError::transport("model_cancelled_before_response", ModelRetryClass::Never, false))
            }
            result = self.complete_prepared(prepared, on_delta) => result,
        }
    }

    /// Describe the exact system prompt and wire accounting before any network call.
    fn request_context(&self, request: &ModelRequest) -> ModelRequestContext {
        ModelRequestContext::for_request(request)
    }

    /// 完成一次模型轮次。
    ///
    /// 返回错误时 Runner 应按 `model_unavailable`/脚本错误处理，不应伪造空的成功结果。
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String>;

    /// 完成一次模型轮次，并在文本生成时交付增量。
    ///
    /// 默认实现保持所有现有客户端的对象安全兼容性：先调用 [`ModelClient::complete`]，
    /// 再把完整文本作为一条 [`ModelDelta::Text`] 交给 `on_delta`；文本为空时不调用回调。
    ///
    /// `on_delta` 是同步回调，不创建后台任务。调用方可在回调中转发到自己的 channel，并通过
    /// 返回错误表达背压或取消。原生流式实现可沿用该回调交付工具调用、usage 和
    /// stop_reason 增量，最终仍返回完整聚合的 [`ModelOutput`]。
    ///
    /// 【作用】
    /// 支持流式输出（token 逐段吐出）的方法。
    ///
    /// 【默认实现做了什么】
    /// 调 `complete()` 拿到完整结果，**然后**把整段文本作为**一条**增量交给回调。
    /// 换句话说：默认实现并不真的流式，只是把"一次性到达的完整文本"
    /// 包装成"一个增量事件"发给回调，以便不破坏回调这条通路。
    ///
    /// 【⚠ 为什么文本为空时不调用回调】
    /// 因为一次空回调对调用方没有意义，却会让"收到过增量"这个信号变得不可靠。
    /// 调用方常用"有没有收到过 delta"来判断流是否正常启动。
    /// 空文本还发一个空 delta，会让这个判断永远为真，掩盖真实的空响应。
    ///
    /// 【`on_delta` 为什么是同步回调（`&mut dyn FnMut`）—— 三个约束】
    ///
    /// 1. **不创建后台任务** —— 如果回调在后台执行，调用方无法知道增量是否
    ///    处理完，方法返回时可能还有数据在飞，顺序无法保证。
    /// 2. **返回 `Result` 表达背压或取消** —— 回调返回 `Err` 会让整个流式调用
    ///    失败。这给了下游"我处理不过来了，叫停"的能力。
    ///    如果是纯 `()` 返回值，下游只能被动接收，无法反压。
    /// 3. **对象安全（object safety）** —— 用 `&mut dyn FnMut` 而不是泛型
    ///    `F: FnMut`，这样 `ModelClient` 仍然可以用 `dyn` trait object，
    ///    保持与所有现有客户端的兼容性。
    ///
    /// 【真正的流式实现应该怎么做】
    /// 原生流式实现（比如真实的 provider）应当在这个回调里交付
    /// 工具调用增量、用量增量和停止原因增量，**但最终仍然返回完整聚合的
    /// [`ModelOutput`]。**
    ///
    /// 这一点很重要：流式只是**传输方式**的差异，**返回值语义不变**。
    /// 这样上层就不需要写两套处理逻辑 —— 流式与非流式走同一条收尾路径。
    /// 如果流式实现改成"只返回增量、不返回完整结果"，
    /// 上层每条分支都得改一遍，漏一条就是 bug。
    async fn complete_streaming(
        &self,
        request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        let output = self.complete(request).await?;
        if !output.text.is_empty() {
            on_delta(ModelDelta::Text {
                text: output.text.clone(),
            })?;
        }
        Ok(output)
    }
}
