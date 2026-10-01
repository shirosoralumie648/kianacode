//! `ControlPlane` 的**构造**与**命令入口**。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! 入口（CLI / Workbench / Web / Desktop）
//!    ↓  versioned command（kiana-protocol 的 CommandIntent）
//! 【本文件 ControlPlane::handle_command】   唯一命令入口：分派 + 前置校验
//!    ↓
//! ControlPlane 的其它模块（lifecycle / collaboration / company / …）或
//! authorize_and_execute → policy → gates → approval → broker
//!    ↓
//! kiana-eventlog（EventStore）→ Receipt
//! ```
//!
//! **它是「所有命令的唯一入口」**。任何入口想产生副作用，都必须把命令送到这里。
//! 仓库宪法明确禁止在 entrypoint、UI、MCP、workflow 或 artifact 里另起一条执行路径——
//! 本文件就是那条唯一路径的起点。
//!
//! # 两件事
//!
//! 1. **构造 `ControlPlane`**：把策略引擎、关卡引擎、事件存储、能力 broker、审批存储、
//!    runner 六个依赖装配进来。构造器分成四层是历史演进 + 测试便利的折中，见下方说明。
//! 2. **分派命令**：[`ControlPlane::handle_command`] 是一个大 `if/match` 链，
//!    把命令名路由到对应的处理函数。路由**不等于**授权——真正的授权在下游。
//!
//! # 数据流
//!
//! ```text
//! RequestContext（我是谁：actor / role / project / cell / request_id）
//! CommandIntent（要做什么：name + arguments）
//!        ↓  handle_command 按 name 分派
//!        ↓  ① 少数命令有「前置硬校验」（如 operator 身份、扩展变更必填字段）
//!        ↓  ② 其余命令转交对应处理函数
//!        ↓  ③ 需要副作用的命令统一走 authorize_and_execute
//! 策略（policy）→ 关卡（gates）→ 审批（approval）→ 执行（broker）
//!        ↓
//! 事件写入 EventStore → 生成 Receipt → 返回 CoreResponse
//! ```
//!
//! # 为什么「路由」和「授权」要分开
//!
//! 因为它们回答的是两个不同问题。路由回答「这个命令归谁管」，授权回答「**你**能不能做」。
//! 如果把授权写进路由分支，就会出现「某个新命令忘了写授权检查」这种致命漏洞。
//! 所以本文件里的分支只做**形状与前置条件**校验，任何真正产生副作用的路径最终都必须
//! 汇入 `authorize_and_execute`。

use super::*;

impl ControlPlane {
    /// Inject the artifact persistence adapter used by Company artifact commands. This is an
    /// optional composition-root dependency so existing embedders keep their legacy text-only
    /// behavior until they explicitly provide a store.
    pub fn with_artifact_store(mut self, store: Arc<dyn kiana_ports::ArtifactStorePort>) -> Self {
        self.artifact_store = Some(store);
        self
    }

    /// 设置「按角色区分的单轮最大步数」覆盖值。
    ///
    /// 【作用】
    /// `ControlPlane` 内部有两处步数限制：全局的 `max_steps_per_turn`，以及按角色目录
    /// 查出来的每轮上限。这里只改后者。
    ///
    /// 【输入】
    /// - `max_steps_override`：`Some(n)` 表示**信任调用方**（通常是受控的组合根 `DaemonHost`）
    ///   已经算好了角色上限；`None` 表示「按角色目录现算」。
    ///
    /// 【为什么用 `Option` 而不是直接传数字】
    /// 因为 `None` 和 `Some(0)` 语义完全不同：`None` 是「我不知道，你去查」，
    /// `Some(0)` 是「这个角色一步都不能走」。用 `Option` 能把两者区分开，
    /// 不会在某个调用点不小心把 0 当成「不限制」。
    ///
    /// 【⚠ 安全边界】
    /// 这是 builder 链上的**内部旋钮**，不是对外的权限开关。它只能影响「跑多少步」，
    /// 不能新增工具、不能改变授权结果。真正的信任来源是组合根——所以它被放在
    /// `with_*` 构造链的**最后**一步，调用方无法在中间插入。
    ///
    /// 【为什么是 `mut self` 消耗式 builder】
    /// 返回 `Self` 而不是 `&mut Self`，是为了让「忘记赋值」在编译期就失败。
    /// Select a per-run role limit unless the trusted composition root supplies an override.
    pub fn with_role_step_limits(mut self, max_steps_override: Option<u32>) -> Self {
        self.max_steps_per_turn = max_steps_override;
        self
    }

    /// 最简构造：六个核心依赖 + 一个「全部放行」的工具前置钩子。
    ///
    /// 【⚠ 这里的默认值有讲究】
    /// `AllowAllPreToolHooks` 意味着**不做任何工具前置拦截**。它是默认值而不是「更安全的默认」，
    /// 是因为工具前置钩子（pre-tool hook）属于项目本地扩展能力，默认开启会让
    /// 「没配 hook 的项目」也走一遍钩子判定路径。真正的安全网不在这里——
    /// 在 policy / gates / approval 那条链上。
    ///
    /// 【上层谁来调】
    /// `kiana-daemon::DaemonHost` 在生产装配时不会用这个简版，而是用
    /// 带 hook、带 runtime config、带 cell registry 的完整版。
    pub fn new(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
    ) -> Self {
        Self::with_pre_tool_hooks(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            Arc::new(AllowAllPreToolHooks),
        )
    }

    /// 构造器第 2 层：显式注入工具前置钩子。
    ///
    /// 【为什么要分这么多层】
    /// 因为依赖是逐步长出来的：先有六个核心端口，后来加了 hook，再后来加了 runtime config
    /// 和 cell registry。Rust 没有默认参数，于是每一层加依赖就多一个构造器。
    ///
    /// 【⚠ 这是一个已知的可维护性代价】
    /// 更好的做法是一个 `ControlPlaneDeps` 结构体 + `Default`。现在这样写的好处是
    /// **每个构造器的能力集合在签名里一目了然**，坏处是层数会继续增长。
    /// 后续如果再加依赖，应当考虑引入依赖结构体，而不是继续加第 5、第 6 个构造器。
    ///
    /// 【本层额外做了什么】
    /// 用**默认 runtime config**（`max_steps_per_turn = 32`）构造，然后调用
    /// `.with_role_step_limits(None)`，也就是「角色步数不覆盖，按角色目录现算」。
    ///
    /// 【32 这个数字】
    /// 32 是「一个对话轮次内模型最多被驱动多少步」的兜底上限。
    /// 为什么要有一个总上限：模型可能在两个工具之间反复横跳，没有硬上限就会一直烧钱。
    /// 为什么是 32 而不是更大：32 步足够完成绝大多数真实任务（读几个文件、改一个函数、
    /// 跑一次测试），而超出通常意味着模型卡住了。与其让它烧到上限，
    /// 不如早点停下来把「未完成」如实暴露出来。
    pub fn with_pre_tool_hooks(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
        pre_tool_hooks: Arc<dyn PreToolHookPort>,
    ) -> Self {
        Self::with_pre_tool_hooks_and_runtime_config(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            pre_tool_hooks,
            ControlPlaneRuntimeConfig {
                max_steps_per_turn: 32,
            },
        )
        .with_role_step_limits(None)
    }

    /// 构造器第 3 层：额外注入运行时配置（当前只有单轮步数上限）。
    ///
    /// 【注意它做了什么、没做什么】
    /// 做了：把调用方给的 `runtime_config.max_steps_per_turn` 传下去。
    /// 没做：**没有**在这里校验这个值是否合理（比如是否超过某个硬上限）。
    /// 校验放在更下游的运行时闸门，这样即使有人绕过本构造器直接构造 `ControlPlane`，
    /// 也不会拿到一个无上限的运行时。
    ///
    /// 【额外装配了什么】
    /// 用 `MemoryCellRegistry`（**进程内**的 Cell 注册表）作为默认实现。
    /// ⚠ 它是内存实现：不跨进程、不持久化。重启后 Cell 状态会丢。
    /// 需要持久化语义时，组合根必须显式注入别的实现——这正是本构造器存在的意义。
    pub fn with_pre_tool_hooks_and_runtime_config(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
        pre_tool_hooks: Arc<dyn PreToolHookPort>,
        runtime_config: ControlPlaneRuntimeConfig,
    ) -> Self {
        Self::with_pre_tool_hooks_and_cell_registry(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            pre_tool_hooks,
            ControlPlaneRuntimeConfig {
                max_steps_per_turn: runtime_config.max_steps_per_turn,
            },
            Arc::new(MemoryCellRegistry::new()),
        )
    }

    /// 构造器第 4 层（最终层）：**唯一真正干活的地方**，前三层都转发到这里。
    ///
    /// 【作用】
    /// 把八个依赖 + 运行时配置写进 `ControlPlane`，并初始化十张进程内状态表。
    ///
    /// 【那些 `Mutex<HashMap<..>>` 是什么】
    /// 它们是控制面的**进程内运行态**，不是事实来源：
    /// - `sessions`：活跃会话 → 会话状态；
    /// - `invocation_projections` / `invocation_projection_event_ids`：Invocation 的投影缓存
    ///   （可重建，丢了能从事件重算）；
    /// - `pending_invocations`：已发起但还没收到结果的调用（**结果未知**的那一类）；
    /// - `cancellations` / `capability_stops`：取消请求与停止信号；
    /// - `active_terminal_scopes`：已经进入终态的作用域，防止重复终结；
    /// - `path_locks` / `durable_path_locks`：进程内路径锁 + 持久化路径锁；
    /// - `admission_scheduler`：能力准入调度器（并发上限、排队）。
    ///
    /// 【⚠ 为什么它们是 `Mutex<HashMap>` 而不是 `RwLock`/`DashMap`】
    /// 控制面的写操作远少于读操作，而且这些表都很小。用最简单的 `Mutex` 换来的是
    /// 「不可能写出数据竞争」这个强保证。当某张表真的成为瓶颈时再换，
    /// 比一开始就引入更复杂的并发原语更容易证明正确。
    ///
    /// 【⚠ 关键边界】
    /// 这些表**不是事实**。它们丢了，理论上可以从 EventLog 重建。
    /// 任何把「重启后表空了所以放行」当策略的代码都是错的——正确做法是重建或拒绝。
    pub fn with_pre_tool_hooks_and_cell_registry(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
        pre_tool_hooks: Arc<dyn PreToolHookPort>,
        runtime_config: ControlPlaneRuntimeConfig,
        cell_registry: Arc<dyn kiana_ports::CellRegistryPort>,
    ) -> Self {
        let max_steps_per_turn = Some(runtime_config.max_steps_per_turn);
        Self {
            policy,
            gates,
            events,
            artifact_store: None,
            capabilities,
            approvals,
            runner,
            max_steps_per_turn,
            workspace_checkpoints: None,
            pre_tool_hooks,
            cell_registry,
            sessions: Mutex::new(HashMap::new()),
            invocation_projections: Mutex::new(HashMap::new()),
            invocation_projection_event_ids: Mutex::new(HashMap::new()),
            pending_invocations: Mutex::new(HashMap::new()),
            cancellations: Mutex::new(HashMap::new()),
            capability_stops: Mutex::new(HashMap::new()),
            active_terminal_scopes: Mutex::new(HashMap::new()),
            path_locks: Mutex::new(HashMap::new()),
            durable_path_locks: Mutex::new(HashMap::new()),
            admission_scheduler: Arc::new(CapabilityAdmissionScheduler::default()),
        }
    }

    /// **所有命令的唯一入口**：把一个 `CommandIntent` 路由到处理它的地方。
    ///
    /// 【作用】
    /// 1. 识别少数「特权命令」（operator 命令、运行轮次、通信、swarm、automation、company…），
    ///    为它们做**前置硬校验**，然后转交对应处理函数；
    /// 2. 其余命令统一走 `authorize_and_execute` 这条受控通道。
    ///
    /// 【调用者】
    /// `kiana-daemon::DaemonHost` 在收到 versioned command 后调用。CLI、Workbench、
    /// Web、Desktop 全部经过这里——这正是「唯一执行脊柱」的落地方式。
    ///
    /// 【输入】
    /// - `context: RequestContext`：**服务端解析出来的**调用者身份（actor / role /
    ///   project / cell / request_id）。⚠ 它不是客户端自称的，客户端自称的字段会被忽略或拒绝；
    /// - `intent: CommandIntent`：命令名 + JSON 参数。
    ///
    /// 【输出】
    /// `Result<CoreResponse, CoreError>`。注意有两种「失败」表达方式，含义不同：
    /// - `Ok(CoreResponse::blocked(..))`：**请求本身合法，但当前条件不满足**
    ///   （例如需要 operator 身份而调用方是 cell 内的 worker）。这是一次正常的业务拒绝，
    ///   应当如实展示给用户；
    /// - `Err(..)`：请求不合法（参数缺失、id 非法、扩展变更字段不全等）。
    ///
    /// 【副作用】
    /// 取决于被路由到的分支：有的分支只读（快照查询），有的会真正执行能力、
    /// 写入 EventStore、生成 Receipt。**本函数自己不直接做副作用**，
    /// 它只是决定「这件事该由谁去做」，实际执行必须过 policy/gates/approval/broker。
    ///
    /// 【核心流程】
    /// ```text
    /// 1. 特权命令组：要求 operator 身份（非 cell 内的、有 actor_id、参数是对象）
    ///    ├─ workspace.transaction / execution.output.read / environment.inspect
    ///    ├─ tool.search / process.*（启动、轮询、写 stdin、改尺寸、停止）
    ///    ├─ run.turn.v2（一轮对话）
    ///    ├─ communication.* / swarm.* / automation.* / company.*
    ///    └─ extension.*（扩展安装/升级/回滚）
    /// 2. 每一个都转交给专用处理函数，**不**在本函数里实现业务逻辑
    /// 3. 未识别的命令 → 由最后的兜底分支拒绝（fail-closed）
    /// ```
    ///
    /// 【为什么用一长串 `if` 而不是 `match`】
    /// 因为大部分判断是「命令名等于某个常量」的相等性判断，`matches!` 比 `match` 更省事；
    /// 而少数分支需要读参数才能决定（例如 `run.turn.v2` 要从参数里取 `run_id`）。
    /// 代价是这个函数很长——**这是已知的可维护性代价**。
    /// 将来如果命令数量继续增长，应当抽成「命令注册表 + 分派表」，
    /// 但要注意：**注册表只能做路由，不能携带授权逻辑**，否则就等于给「新增命令」开了一条
    /// 默认放行的后门。
    pub async fn handle_command(
        &self,
        context: RequestContext,
        intent: CommandIntent,
    ) -> Result<CoreResponse, CoreError> {
        // ────────────────────────────────────────────────────────────────────
        // 第一组：operator（运维）特权命令
        //
        // 这一组命令能碰工作区文件、长驻进程、宿主环境，**不允许**由 cell 内部的
        // worker 触发。三个硬条件，缺一即拒：
        //   1. `context.cell_id.is_none()` —— 调用方不是某个受限 Cell；
        //   2. `actor_id` 非空             —— 必须有具名主体，匿名操作无法追责；
        //   3. `arguments` 是 JSON 对象     —— 参数形状必须可判定。
        //
        // ⚠ 这里返回 `Ok(blocked)` 而不是 `Err`：「你不是 operator」是一个**正常的
        //    业务拒绝**，界面应如实显示；而 `Err` 留给「你的请求本身是错的」。
        //    两种表达混用，上层就分不清「你不能做」和「你请求错了」。
        //
        // ⚠ 这段是**纵深防御**，不是唯一防线。即使放行，命令仍要走
        //    `authorize_and_execute` 完整过一遍 policy/gates/approval。
        //    删掉它不会让命令变得可用，只会让「谁能用」这条边界变模糊。
        // ────────────────────────────────────────────────────────────────────

        if matches!(
            intent.name.as_str(),
            "workspace.transaction"
                | "execution.output.read"
                | "environment.inspect"
                | "tool.search"
                | "process.start"
                | "process.poll"
                | "process.stdin"
                | "process.resize"
                | "process.stop"
        ) {
            if context.cell_id.is_some()
                || context.actor_id.as_deref().is_none_or(str::is_empty)
                || !intent.arguments.is_object()
            {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "execution_operator_required",
                ));
            }
            let mut arguments = intent.arguments;
            if intent.name == "process.start" {
                let sandbox = crate::lifecycle::authorized_harness_sandbox(
                    &context,
                    arguments["sandbox"].as_str(),
                )
                .map_err(|reason| PortError::Failed(reason.to_owned()))?;
                arguments["sandbox"] = json!(sandbox);
            }
            arguments["operator_authorized"] = json!(true);
            // 打上「已经过 operator 校验」的标记。
            //
            // ⚠ 这是一个**在服务端**写入的标记，不是客户端能传的字段。
            //    如果允许客户端自己带 `operator_authorized: true`，等于把特权校验架空。
            //    下游的 sandbox 解析、风险分级都依赖它，所以它必须在这里被无条件覆盖。
            //
            // 紧接着的 `match` 是「命令名 -> 授权强度」的翻译层，
            // **必须与实际副作用一致**，否则就会出现「一个 Critical 操作被标成
            // ReadOnly 从而跳过审批」这种致命漏洞。
            //
            //   workspace.transaction : list/inspect -> 只读；其余 -> Critical（要审批）
            //   process.start         : sandbox=read-only -> 只读；否则 -> 本地写
            //   process.stdin/resize/stop : 一律本地写（能影响已启动的进程就是写）
            //   其余（tool.search / environment.inspect / output.read）-> 只读
            //
            // ⚠ 新增命令时必须在这里显式落一个分支。兜底的 `_ => (Query, ReadOnly)`
            //    看起来安全，但它只在「确实只读」时才安全——有人加了有副作用的命令
            //    却忘了改这里，就会被静默降级成只读。这是本函数最需要 reviewer 盯住的一处。

            let (kind, risk) = match intent.name.as_str() {
                "workspace.transaction" => (
                    CapabilityKind::Filesystem,
                    if matches!(arguments["action"].as_str(), Some("list" | "inspect")) {
                        RiskLevel::ReadOnly
                    } else {
                        RiskLevel::Critical
                    },
                ),
                "process.start" => (
                    CapabilityKind::Process,
                    if arguments["sandbox"] == "read-only" {
                        RiskLevel::ReadOnly
                    } else {
                        RiskLevel::LocalWrite
                    },
                ),
                "process.stdin" | "process.resize" | "process.stop" => {
                    (CapabilityKind::Process, RiskLevel::LocalWrite)
                }
                _ => (CapabilityKind::Query, RiskLevel::ReadOnly),
            };
            return self
                .authorize_and_execute(
                    &context,
                    CapabilityRequest::new(context.request_id, kind, intent.name, arguments)
                        .with_risk(risk),
                )
                .await;
        }
        if intent.name == "effect.reconcile" {
            // DEP-25 的接线：一次「外部 effect 结果未知时，四种诚实动作里哪一种可被记录」的判定。
            //
            // 同样不走 `authorize_and_execute`：判定只回答「能不能这样记」，不执行任何补偿、
            // 不查询 provider、不签发审批。
            //
            // 为什么要求 operator：这条判定决定的是「一个已经发出去的请求算什么」——
            // 重发、放弃还是补偿，每一个都会改变真实世界的后果。让 cell 内部的 worker 参与
            // 裁定自己发出去的请求算什么，等于让它为自己的行为定性。
            if context.cell_id.is_some() || context.actor_id.as_deref().is_none_or(str::is_empty) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "effect_reconcile_operator_required",
                ));
            }
            let object = match intent.arguments.as_object() {
                Some(object) => object,
                None => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "effect_reconcile_payload_required",
                    ))
                }
            };
            let (Some(raw_observation), Some(raw_resolution)) =
                (object.get("observation"), object.get("resolution"))
            else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "effect_reconcile_payload_required",
                ));
            };
            let observation: kiana_domain::EffectObservation =
                match serde_json::from_value(raw_observation.clone()) {
                    Ok(observation) => observation,
                    Err(_) => {
                        return Ok(CoreResponse::blocked(
                            context.request_id,
                            "effect_reconcile_payload_invalid",
                        ))
                    }
                };
            // 未知动作与缺失动作要分开：前者要改字段，后者要补字段，合并就没法行动了。
            let Some(raw_resolution) = raw_resolution.as_str() else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "effect_reconcile_payload_required",
                ));
            };
            let resolution = match EffectResolution::parse(raw_resolution) {
                Ok(resolution) => resolution,
                Err(_) => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "effect_reconcile_resolution_unknown",
                    ))
                }
            };
            let optional_object = |key: &str| object.get(key).cloned();
            let optional_text =
                |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
            let external_receipt = match optional_object("external_receipt") {
                Some(raw) => match serde_json::from_value(raw) {
                    Ok(receipt) => Some(receipt),
                    Err(_) => {
                        return Ok(CoreResponse::blocked(
                            context.request_id,
                            "effect_reconcile_payload_invalid",
                        ))
                    }
                },
                None => None,
            };
            let consumed_approvals = object
                .get("consumed_approvals")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            // 构造本身就会封口并自检（形状、审批一次性、字段互斥）。它失败时给出的 reason
            // 已经是稳定的判定码，所以直接以 blocked 交出去，而不是压成一句「请求无效」。
            let request = match EffectReconciliationRequest::new(
                observation,
                resolution,
                external_receipt,
                optional_text("compensation_ref"),
                optional_text("abandon_reason"),
                optional_text("approval_ref").unwrap_or_default(),
                object
                    .get("authority_epoch")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                optional_text("fence_token").unwrap_or_default(),
                consumed_approvals,
                optional_text("idempotency_key").unwrap_or_default(),
            ) {
                Ok(request) => request,
                Err(reason) => return Ok(CoreResponse::blocked(context.request_id, reason)),
            };
            // 不可记录是一个**结论**：调用方需要区分「不能这样记」和「你的请求坏了」，
            // 所以连同模块自己的稳定 reason 一起以 blocked 返回。
            return match reconcile_effect(&request) {
                Ok(receipt) => Ok(CoreResponse::completed(
                    context.request_id,
                    json!({
                        "schema": EFFECT_RECONCILIATION_RECEIPT_SCHEMA,
                        "receipt": receipt,
                    }),
                )),
                Err(reason) => Ok(CoreResponse::blocked(context.request_id, reason)),
            };
        }
        if intent.name == "capacity.envelope" {
            // SC-40 的接线：一次「故障期间系统是否仍在有界 latency/queue/bytes 之内」的只读判定。
            //
            // 与前两条路线同样不走 `authorize_and_execute`：判定不写事件、不改状态、不发能力。
            //
            // 为什么要求 operator：这份判定的结论会被用来解释「当时有没有越界」。
            // 让 cell 内部的 worker 参与裁定系统是否守住了边界，等于让它为自己的行为作证。
            if context.cell_id.is_some() || context.actor_id.as_deref().is_none_or(str::is_empty) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "capacity_envelope_operator_required",
                ));
            }
            let object = match intent.arguments.as_object() {
                Some(object) => object,
                None => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "capacity_envelope_payload_required",
                    ))
                }
            };
            let (Some(raw_sample), Some(raw_budget)) = (object.get("sample"), object.get("budget"))
            else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "capacity_envelope_payload_required",
                ));
            };
            let (sample, budget) = match (
                serde_json::from_value::<CapacityFaultSample>(raw_sample.clone()),
                serde_json::from_value::<CapacityBudget>(raw_budget.clone()),
            ) {
                (Ok(sample), Ok(budget)) => (sample, budget),
                _ => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "capacity_envelope_payload_invalid",
                    ))
                }
            };
            // 越界是一个**结论**，不是协议错误：调用方需要区分「越界了，界是这个」和
            // 「你给的数据我读不懂」，而这两者的下一步完全不同。
            return match evaluate_capacity_fault(&sample, &budget) {
                Ok(report) => Ok(CoreResponse::completed(
                    context.request_id,
                    json!({
                        "schema": CAPACITY_FAULT_REPORT_SCHEMA,
                        "report": report,
                    }),
                )),
                Err(reason) => Ok(CoreResponse::blocked(context.request_id, reason)),
            };
        }
        if intent.name == "promotion.check" {
            // BQ-30 的接线：一次「这个主张能不能被叫得比它的证据更强」的只读判定。
            //
            // 和 `security.incident.evaluate` 一样，它不产生副作用，所以不走 `authorize_and_execute`——
            // 一条 promotion 记录不是能力请求，把纯判定塞进执行通道会同时浪费配额并在审计里
            // 留下一个从未发生的 effect。
            //
            // 为什么仍然要求 operator 身份：promotion 是决定「对外声称什么」的动作。让 cell 内部的
            // worker 自行决定一条主张能不能升到 `opt_in_live`，等于给了它一个自我提权的口子。
            if context.cell_id.is_some() || context.actor_id.as_deref().is_none_or(str::is_empty) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "promotion_operator_required",
                ));
            }
            let object = match intent.arguments.as_object() {
                Some(object) => object,
                None => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "promotion_payload_required",
                    ))
                }
            };
            let (Some(raw_evidence), Some(raw_level)) =
                (object.get("evidence"), object.get("claimed_level"))
            else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "promotion_payload_required",
                ));
            };
            let evidence: EvidenceManifest = match serde_json::from_value(raw_evidence.clone()) {
                Ok(evidence) => evidence,
                Err(_) => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "promotion_payload_invalid",
                    ))
                }
            };
            // 缺 `claimed_level` 与写错它是两件事：前者是「没说要升到哪一档」，
            // 后者是「说了但系统不认识」。分开才能让调用方知道该补字段还是该改字段。
            let Some(raw_level) = raw_level.as_str() else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "promotion_payload_required",
                ));
            };
            let claimed_level = match ClaimedLevel::parse(raw_level) {
                Ok(level) => level,
                Err(_) => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "promotion_level_unknown",
                    ))
                }
            };
            let string_field =
                |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
            let request = PromotionGateRequest::new(
                object
                    .get("claim_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                claimed_level,
                evidence,
                string_field("provider_receipt_ref"),
                string_field("opt_in_ref"),
                string_field("restart_replay_ref"),
            );
            // 「证据够不着这个级别」是一个**业务结论**，不是协议错误，所以带上门自己的稳定 reason
            // 一起以 blocked 返回：调用方需要区分「证据不足」和「你的请求坏了」，
            // 而这两者的下一步动作完全不同。
            return match evaluate_promotion(&request) {
                Ok(decision) => Ok(CoreResponse::completed(
                    context.request_id,
                    json!({
                        "schema": PROMOTION_GATE_DECISION_SCHEMA,
                        "decision": decision,
                    }),
                )),
                Err(reason) => Ok(CoreResponse::blocked(context.request_id, reason)),
            };
        }
        if intent.name == "security.incident.evaluate" {
            // SC-33 的接线：一次「这个动作对这份事件是否可被记录」的只读判定。
            //
            // 为什么它不走 `authorize_and_execute`：那个通道是为**有副作用**的能力请求准备的，
            // 而这里不写事件、不改状态、不发能力——判定结果本身就是答案。把它塞进执行通道
            // 会让一次纯查询占掉一个能力配额，并在审计里留下一条并不存在的 effect。
            //
            // 为什么仍然要检查身份与信任：判定要指名 owner 与 reviewer，匿名调用等于让任何人
            // 替别人签署一次响应；而项目未信任时，项目本地的配置可以注入指令，不该由它来驱动
            // 一次安全响应。
            if context.actor_id.as_deref().is_none_or(str::is_empty) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "security_incident_actor_required",
                ));
            }
            if !context.project_trusted {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "security_incident_project_untrusted",
                ));
            }
            let object = match intent.arguments.as_object() {
                Some(object) => object,
                None => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "security_incident_payload_required",
                    ))
                }
            };
            let (Some(raw_incident), Some(raw_state), Some(raw_action)) = (
                object.get("incident"),
                object.get("state"),
                object.get("action"),
            ) else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "security_incident_payload_required",
                ));
            };
            // 形状错误是 blocked 而不是 Err：调用方需要知道的是「你给的这份记录不能用来判定」，
            // 而不是「你的请求把协议说坏了」。两者的界面表现不同，混用会让上层分不清。
            let (incident, state, action) = match (
                serde_json::from_value::<SecurityIncident>(raw_incident.clone()),
                serde_json::from_value::<SecurityIncidentState>(raw_state.clone()),
                serde_json::from_value::<SecurityIncidentActionRequest>(raw_action.clone()),
            ) {
                (Ok(incident), Ok(state), Ok(action)) => (incident, state, action),
                _ => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "security_incident_payload_invalid",
                    ))
                }
            };
            // 判定不可被记录是一个**业务结论**，不是协议错误，所以同样走 blocked，并把模块的
            // 稳定 reason code 原样带出去——调用方要靠它区分「跳过了步骤」和「证据被重放」。
            return match incident.evaluate(&state, &action) {
                Ok(report) => Ok(CoreResponse::completed(
                    context.request_id,
                    json!({
                        "schema": SECURITY_INCIDENT_REPORT_SCHEMA,
                        "report": report,
                    }),
                )),
                Err(reason) => Ok(CoreResponse::blocked(context.request_id, reason)),
            };
        }
        if intent.name == "run.turn.v2" {
            let prompt = intent.arguments["prompt"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let sandbox = intent.arguments["sandbox"].as_str().map(str::to_owned);
            let previous = intent.arguments["run_id"]
                .as_str()
                .map(|id| {
                    RunId::parse_str(id)
                        .ok_or_else(|| PortError::Failed("run_id_invalid".to_owned()))
                })
                .transpose()?;
            return self
                .continue_new_turn(context, prompt, sandbox, previous)
                .await;
        }
        if matches!(
            intent.name.as_str(),
            "communication.send"
                | "communication.ack"
                | "communication.reject"
                | "communication.escalate"
        ) {
            return self
                .handle_communication_command(context, &intent.name, intent.arguments)
                .await;
        }
        if matches!(
            intent.name.as_str(),
            "trace.capture" | "trace.replay" | "version.drift"
        ) {
            return self
                .handle_version_command(context, &intent.name, intent.arguments)
                .await;
        }
        if intent.name.starts_with("workspace.checkpoint.") {
            return self
                .handle_checkpoint_command(context, &intent.name, intent.arguments)
                .await;
        }
        if matches!(
            intent.name.as_str(),
            "human.inbox"
                | "human.resolve"
                | "failure.incidents"
                | "failure.reconcile"
                | "failure.recovery"
                | "failure.release"
                | "feedback.list"
                | "feedback.submit"
                | "feedback.review"
        ) {
            return self
                .handle_platform_command(context, &intent.name, intent.arguments)
                .await;
        }
        if intent.name == kiana_domain::SWARM_COMMAND {
            return self.handle_swarm_command(context, intent.arguments).await;
        }
        if intent.name == kiana_domain::SWARM_SNAPSHOT {
            return self.swarm_snapshot(context).await;
        }
        if intent.name == kiana_domain::AUTOMATION_COMMAND {
            return self
                .handle_workflow_command(context, intent.arguments)
                .await;
        }
        if intent.name == kiana_domain::AUTOMATION_SNAPSHOT {
            return self.workflow_snapshot(context).await;
        }
        if intent.name == "company.reclaim_expired.v1" {
            return self.reclaim_packet_leases(context, intent.arguments).await;
        }
        if intent.name == kiana_domain::COMPANY_COMMAND {
            return self.handle_company_command(context, intent.arguments).await;
        }
        if intent.name == kiana_domain::COMPANY_SNAPSHOT || intent.name == "company.next.v1" {
            return self.company_snapshot(context).await;
        }
        if intent.name == kiana_domain::COMPANY_GOVERNANCE {
            let Some(project_id) = intent.arguments["project_id"].as_str() else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "company_governance_project_required",
                ));
            };
            if project_id.trim().is_empty() || project_id.len() > 256 {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "company_governance_project_invalid",
                ));
            }
            return self.company_governance(&context, project_id).await;
        }
        if intent.name == kiana_domain::MEMORY_DISTILL_COMMAND {
            return self
                .handle_memory_distillation(context, intent.arguments)
                .await;
        }
        if matches!(intent.name.as_str(), "quality.promote" | "quality.rollback") {
            return self
                .handle_quality_mutation(context, intent.name, intent.arguments)
                .await;
        }
        if matches!(
            intent.name.as_str(),
            kiana_domain::CONNECTOR_MANAGE_OPERATION
                | kiana_domain::CONNECTOR_INVOKE_OPERATION
                | kiana_domain::CONNECTOR_HEALTH_OPERATION
                | kiana_domain::CONNECTOR_RECONCILE_OPERATION
                | kiana_domain::CONNECTOR_MCP_HANDSHAKE_OPERATION
        ) {
            return self.handle_connector_command(context, intent).await;
        }
        if intent.name == kiana_domain::EXTENSION_MANAGE_OPERATION
            || kiana_domain::ExtensionCommand::from_wire_name(&intent.name).is_some()
        {
            return self
                .handle_extension_management(&intent.name, context, intent.arguments)
                .await;
        }
        if intent.name == CONTEXT_QUERY_COMMAND {
            return self.handle_context_query(context, intent.arguments).await;
        }
        if intent.name == "data.governance" {
            if context.cell_id.is_some() || context.actor_id.as_deref().is_none_or(str::is_empty) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "governance_operator_required",
                ));
            }
            let mut arguments = intent.arguments;
            if !arguments.is_object() {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "governance_arguments_invalid",
                ));
            }
            arguments["operator_authorized"] = json!(true);
            let risk = if arguments["action"] == "list" {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::ExternalSideEffect
            };
            return self
                .authorize_and_execute(
                    &context,
                    CapabilityRequest::new(
                        context.request_id,
                        CapabilityKind::Filesystem,
                        "data.governance",
                        arguments,
                    )
                    .with_risk(risk),
                )
                .await;
        }
        if intent.name == "memory.review" {
            if context.cell_id.is_some() || context.actor_id.as_deref().is_none_or(str::is_empty) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "memory_review_operator_required",
                ));
            }
            let mut arguments = intent.arguments;
            self.prepare_memory_review(&context, &mut arguments).await?;
            let collection = arguments["collection"].as_str().map(str::to_owned);
            let Some(object) = arguments.as_object_mut() else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "memory_review_arguments_invalid",
                ));
            };
            object.insert("operator_authorized".to_owned(), json!(true));
            object.insert("project_root".to_owned(), json!(context.project_root));
            object.insert("session_id".to_owned(), json!(context.session_id));
            object.insert("actor_id".to_owned(), json!(context.actor_id));
            object.insert("role_id".to_owned(), json!(context.role_id));
            object.insert("department_id".to_owned(), json!(context.department_id));
            let risk = if arguments["action"] == "list" {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::ExternalSideEffect
            };
            let mut request = CapabilityRequest::new(
                context.request_id,
                CapabilityKind::Filesystem,
                "memory.review",
                arguments,
            );
            request.risk = risk;
            let mut response = self.authorize_and_execute(&context, request).await?;
            if risk == RiskLevel::ReadOnly && response.status == ExecutionStatus::Completed {
                response.output["proposals"] = json!(
                    self.pending_memory_proposals(&context, collection.as_deref())
                        .await?
                );
            }
            return Ok(response);
        }
        let request_id = context.request_id;
        self.append_event(
            request_id,
            1,
            "request.accepted",
            json!({
                "command": &intent.name,
            }),
        )
        .await?;

        if !context.project_trusted {
            let reason = "project_untrusted";
            self.append_event(
                request_id,
                2,
                "command.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        if intent.name != "system.architecture" {
            let reason = "command_unregistered";
            self.append_event(
                request_id,
                2,
                "command.rejected",
                json!({ "reason": reason, "command": &intent.name }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        let output = json!({
            "schema": "kiana.architecture-status.v1",
            "control_plane": "kiana-core",
            "composition_root": "kiana-daemon",
            "runner": "kiana-runner",
            "harness": HARNESS_ID,
            "capability_mode": "brokered",
            "legacy_prompt_loop": false,
            "legacy_edges_remaining": LEGACY_EDGES_REMAINING,
        });
        self.append_event(request_id, 2, "command.completed", output.clone())
            .await?;
        Ok(CoreResponse::completed(request_id, output))
    }

    async fn handle_extension_management(
        &self,
        command_name: &str,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let normalized = normalize_extension_command(command_name, &context, arguments);
        let (arguments, risk) = match normalized {
            Ok(value) => value,
            Err(reason) => {
                self.append_event(
                    context.request_id,
                    1,
                    "request.accepted",
                    json!({"command":kiana_domain::EXTENSION_MANAGE_OPERATION}),
                )
                .await?;
                self.append_event(
                    context.request_id,
                    2,
                    "command.rejected",
                    json!({"reason":reason}),
                )
                .await?;
                return Ok(CoreResponse::blocked(context.request_id, reason));
            }
        };
        let request = CapabilityRequest::new(
            context.request_id,
            CapabilityKind::Filesystem,
            kiana_domain::EXTENSION_MANAGE_OPERATION,
            arguments,
        )
        .with_risk(risk);
        self.authorize_and_execute(&context, request).await
    }
}

fn normalize_extension_command(
    command_name: &str,
    context: &RequestContext,
    arguments: Value,
) -> Result<(Value, RiskLevel), &'static str> {
    let mut object = arguments
        .as_object()
        .cloned()
        .ok_or("extension_arguments_invalid")?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "action"
                | "schema"
                | "version"
                | "extension_id"
                | "package_path"
                | "package_sha256"
                | "expected_registry_version"
                | "idempotency_key"
                | "reason"
                | "query"
                | "max_results"
        )
    }) {
        return Err("extension_arguments_invalid");
    }
    if let Some(schema) = object.get("schema").and_then(Value::as_str) {
        if schema != kiana_domain::EXTENSION_COMMAND_SCHEMA {
            return Err("extension_command_schema_invalid");
        }
    }
    if object.get("version").is_some_and(|version| {
        serde_json::from_value::<kiana_domain::SchemaVersion>(version.clone())
            .map(|version| !version.is_compatible_with(&kiana_domain::EXTENSION_COMMAND_VERSION))
            .unwrap_or(true)
    }) {
        return Err("extension_command_version_invalid");
    }
    let action = kiana_domain::ExtensionCommand::from_wire_name(command_name)
        .map(kiana_domain::ExtensionCommand::action)
        .or_else(|| object.get("action").and_then(Value::as_str))
        .ok_or("extension_action_invalid")?
        .to_owned();
    object.insert("action".to_owned(), json!(&action));
    if !matches!(
        action.as_str(),
        "inspect"
            | "list"
            | "search"
            | "install"
            | "upgrade"
            | "enable"
            | "disable"
            | "revoke"
            | "rollback"
    ) {
        return Err("extension_action_invalid");
    }
    let risk = if matches!(action.as_str(), "list" | "inspect") {
        RiskLevel::ReadOnly
    } else {
        RiskLevel::ExternalSideEffect
    };
    if matches!(action.as_str(), "install" | "upgrade")
        && object
            .get("package_path")
            .and_then(Value::as_str)
            .is_none_or(|s| !kiana_domain::valid_extension_path(s))
    {
        return Err("extension_package_path_must_be_project_relative");
    }
    if matches!(action.as_str(), "search")
        && object
            .get("query")
            .and_then(Value::as_str)
            .is_some_and(|query| query.len() > 256 || query.contains('\0'))
    {
        return Err("extension_visibility_query_invalid");
    }
    if object
        .get("max_results")
        .and_then(Value::as_u64)
        .is_some_and(|value| value == 0 || value > 512)
    {
        return Err("extension_visibility_max_results_invalid");
    }
    // 扩展（extension）变更的额外闸门。
    //
    // 只读操作（列出/查询已装扩展）不受这里约束；**任何非只读的扩展变更**
    // 都必须同时满足四个条件，缺一不可：
    //
    //   1. operator 身份（同上：非 cell 内、actor_id 非空）；
    //   2. `extension_id` 合法 + `expected_registry_version` 存在
    //      —— 后者是**乐观并发（optimistic concurrency）**字段：调用方声明
    //      「我看到的注册表版本是 N」，服务端比对，不一致就拒绝。
    //      没有它，两个人同时装扩展会后写覆盖先写；
    //   3. `idempotency_key` 非空且 <= 128 字符
    //      —— 幂等键让重试不会重复执行同一次安装；
    //   4. `reason` 非空且 <= 4096 字符
    //      —— 装一个能注入指令和能力的扩展属于高风险动作，必须留下书面理由。
    //
    // 另外：install / upgrade / rollback 还额外要求 `package_sha256` 是合法十六进制哈希。
    // 理由很直接——**扩展就是可执行代码**，不知道它是什么字节就执行它，
    // 等于放弃了供应链安全。
    //
    // ⚠ 这些校验放在这里而不是 registry 内部，是因为它们校验的是「请求形状」；
    //    「这个扩展该不该被信任」是另一回事，由 trust / provenance 机制回答。

    if risk != RiskLevel::ReadOnly {
        if context.cell_id.is_some()
            || context
                .actor_id
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err("extension_operator_required");
        }
        if object
            .get("extension_id")
            .and_then(Value::as_str)
            .is_none_or(|s| !kiana_domain::valid_extension_identifier(s))
            || object
                .get("expected_registry_version")
                .and_then(Value::as_u64)
                .is_none()
            || object
                .get("idempotency_key")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty() || s.len() > 128)
            || object
                .get("reason")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty() || s.len() > 4096)
        {
            return Err("extension_mutation_fields_required");
        }
        if matches!(action.as_str(), "install" | "upgrade" | "rollback")
            && object
                .get("package_sha256")
                .and_then(Value::as_str)
                .is_none_or(|s| !kiana_domain::is_sha256_hex(s))
        {
            return Err("extension_final_package_hash_required");
        }
    }
    object.insert("operator_authorized".to_owned(), json!(true));
    object.insert("project_root".to_owned(), json!(context.project_root));
    object.insert("actor_id".to_owned(), json!(context.actor_id));
    object.insert("role_id".to_owned(), json!(context.role_id));
    object.insert("department_id".to_owned(), json!(context.department_id));
    Ok((Value::Object(object), risk))
}
