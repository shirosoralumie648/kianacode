//! 一次 Run 的生命周期：开始、继续、注入、取消。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! CLI 等 CommandIntent ─> commands.rs ─┐
//! DaemonHost 的 Run/Continue/Steer/Inject/Cancel 请求 ─┤
//!                                                  ↓
//!                                   【本文件】ControlPlane 生命周期协调
//!                         校验上下文、Run/Turn 身份并记录生命周期事件
//!                                                  ↓
//!                       RunnerPort ── RunnerEvent ──┐
//!                          │                        │
//!                          └─模型循环                └─工具意图回到 ControlPlane
//!                                                        ↓
//!                                    policy → gates → approval → CapabilityBroker
//!                                                        ↓
//! EventStore（事件事实）→ Receipt / run 与 invocation 投影 → CoreResponse
//! ```
//!
//! **上游**：`commands.rs::handle_command()` 路由命令；`kiana-daemon::DaemonHost` 也会把
//! Workbench 等协议请求直接映射到本文件的公开生命周期方法。
//! **下游**：本文件通过 EventStore 端口写入事件、通过 RunnerPort 驱动/取消 Harness，并将
//! runner 产生的 capability 意图送回 ControlPlane 的授权与 Broker 路径。它不在此处直接调用模型
//! provider 或文件执行器。
//!
//! EventLog 是跨请求恢复与投影的持久事实来源；`sessions`、取消 watch 和 pending invocation 等
//! 进程内索引只为当前生命周期协调服务，不能代替 EventLog 判定一个 run 是否完成或副作用是否停止。
//!
//! # 三层入口是有意的，不是历史遗留
//!
//! `start_run` 和 `start_run_with_history` 是两个并列的公开便捷入口，都会委托给
//! `start_run_with_id`；后者仅供内部场景指定 run ID 和 predecessor。拆开签名是有意的：
//! 不带历史、带历史、以及新 turn 连接前一 run 是三种不同调用语义，不应让外部调用方靠
//! 多个可选参数组合来猜哪种关系有效。
//!
//! # 乐观并发：turn id 是「我以为现在轮到谁」
//!
//! `steer_run` 要求调用方给出 `expected_turn_id`。这不是装饰：如果 run 已经推进到下一个
//! turn，而 UI 还按旧认知插话，那么这条 steer 会被写到错误的 turn 上，模型看到的是
//! 「上一轮和这一轮之间」的顺序错乱。要求调用方声明它以为的 turn，让不匹配变成拒绝，
//! 是比事后修正便宜得多的做法。
use super::events::*;
use super::redaction::*;
use super::*;
use kiana_domain::CapabilityErrorCode;

/// 把生命周期里的字符串理由翻译成结构化的能力错误码。
///
/// 【为什么需要这一层】
/// 生命周期内部大量用字符串表达拒绝理由（`run_not_found`、`cancel_not_confirmed` 之类），
/// 因为它们要进事件载荷、要在 UI 上显示、要能被测试断言。
/// 但**下游**（receipt、投影、重试策略）需要的是可枚举、可穷举匹配的错误码。
///
/// 直接把字符串丢给下游，下游就只能做字符串匹配——漏一个拼写就静默走错分支。
/// 集中在这里翻译一次，`CapabilityErrorCode::from_reason` 就是唯一认识这些字符串的地方。
fn lifecycle_error_code(error: &str) -> CapabilityErrorCode {
    CapabilityErrorCode::from_reason(error)
}

impl ControlPlane {
    /// 开始一次新的 Run。
    ///
    /// 【作用】
    /// 以最简形式开始一个新 run：不携带旧对话历史，也不要求调用方预先分配 run ID。
    ///
    /// 【调用者】`collaboration.rs` 为首轮 speaker 创建 run 时调用；带历史的 DaemonHost 请求
    /// 使用 `start_run_with_history()`，`run.turn.v2` 协议命令则由 `handle_command()` 路由到
    /// `continue_new_turn()`。已有 run 内继续使用 `continue_run()`。
    ///
    /// 【输入】调用身份/项目等 `RequestContext`、用户 prompt、可选 sandbox 名称。
    /// 【输出】返回 `CoreResponse`：可完成、被阻止或失败；底层端口/事件存储故障返回 `CoreError`。
    /// 【副作用】委托 `start_run_with_id()` 写请求/授权/prompt 事件并向 RunnerPort 发起新 run。
    /// 【设计原因】将常见的“空历史、由控制面选择 run ID”形态固定在一个小入口，避免调用方
    /// 自己拼 Option 参数时遗漏前驱关系或复用错误的 run ID。
    pub async fn start_run(
        &self,
        context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        self.start_run_with_id(context, prompt, Vec::new(), sandbox, None, None)
            .await
    }

    /// 带既有协议对话历史开始一个新 run。
    ///
    /// 【调用者】`kiana-daemon::DaemonHost` 将带 `history` 的 Run 请求映射到此入口。
    /// 【输入】身份上下文、当前新 prompt、既有 `ConversationMessage` 列表和可选 sandbox。
    /// 【输出】与 `start_run()` 相同，交由统一的 run 驱动路径返回 `CoreResponse`。
    /// 【数据流】history 与新 prompt 一起传给 `start_run_with_id()`，再进入 RunnerCommand；此处不
    /// 自行折叠历史或调用模型。新输入仍会作为独立 `run.prompt` 事实记录。
    /// 【设计原因】把“新 run + 既有历史”与“既有 run 继续”分开，避免把协议历史误当成可修改的
    /// EventLog 事实或把前一 run 的终态生命周期重新打开。
    pub async fn start_run_with_history(
        &self,
        context: RequestContext,
        prompt: String,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        self.start_run_with_id(context, prompt, history, sandbox, None, None)
            .await
    }

    /// 统一的新 run 创建路径：确定 run/turn 身份、记录授权上下文并启动 Runner。
    ///
    /// 【调用者】`start_run()`、`start_run_with_history()` 和 `continue_new_turn()` 调用；仅限
    /// `kiana-core` crate 内部使用。
    ///
    /// 【输入】可变 `RequestContext`（用于绑定 run scope）、prompt、已有对话历史、sandbox 请求、
    /// 可选显式 run ID，以及可选的前驱 run ID。
    ///
    /// 【输出】Runner 事件经 `drive_run()` 消费后形成 `CoreResponse`；配置/存储/runner 端口失败
    /// 可能返回 `CoreError`，常见的用户输入或授权前置条件失败则写拒绝事实并返回 Blocked response。
    ///
    /// 【副作用】写入 predecessor、request.accepted、run.authorized、run.prompt 等 EventLog 事实；
    /// 绑定 session/cancel/watch 索引和模型 assignment；成功绑定后向 RunnerPort 发送 start 命令。
    /// 不直接运行模型或工具。
    ///
    /// 【核心流程】推导 ID → 固化 `TurnIdentity` → 校验 prompt/role/department/distillation/sandbox
    /// → 计算步数、runtime budget 与 authority 版本 → 持久化授权和 prompt → 安装取消/终态守卫
    /// → 绑定模型 assignment → 启动 Runner → 统一消费 RunnerEvent。
    ///
    /// 【`TurnSemantics` 为什么有两种取值】
    /// `Start` 表示这是这条历史的第一个 turn；`NewTurn` 表示它前面有一个 `predecessor`。
    /// 区别不是措辞：重放时它决定了折叠是从零开始还是从 predecessor 续上，
    /// 弄错会让同一段历史被折叠两次或一次都不折叠。
    ///
    /// 【为什么不向外公开】外部入口应通过 `start_run` 让 session 对 run ID 的默认绑定规则生效；
    /// 只有明确的 predecessor/new-turn 场景才由内部调用方指定 run ID 和前驱。
    ///
    /// 【失败语义】输入/权限前置条件通常作为结构化 Blocked response 返回；EventStore、公司
    /// scope、runtime 配置或模型绑定出错时会走错误/终态路径。若取消已先提交 `result_unknown`，
    /// 迟到的启动错误不得覆盖它，`record_drive_terminal()` 会优先返回已有未知结果。
    pub(crate) async fn start_run_with_id(
        &self,
        mut context: RequestContext,
        prompt: String,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
        requested_run_id: Option<RunId>,
        predecessor: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        // request_id 是本次命令的稳定关联键；显式 run_id 优先，否则尝试把 session_id 当作 UUID，
        // 解析失败才生成新 ID。这样兼顾可预测的首个 session run 与普通随机运行。
        let request_id = context.request_id;
        let run_id = requested_run_id.unwrap_or_else(|| {
            RunId::parse_str(context.session_id.as_str()).unwrap_or_else(RunId::new)
        });
        let turn_id = kiana_domain::TurnId::from_uuid(request_id.as_uuid());
        // TurnIdentity 固化 session、run、turn 和前驱关系；NewTurn/Start 由 predecessor 是否存在决定。
        let turn = kiana_domain::TurnIdentity::new(
            context.session_id.as_str(),
            run_id,
            turn_id,
            predecessor,
            if predecessor.is_some() {
                kiana_domain::TurnSemantics::NewTurn
            } else {
                kiana_domain::TurnSemantics::Start
            },
            // 每个新 Run 的首个 turn 从 1 开始；TurnIdentity 校验禁止 0，避免与“尚无 turn”混淆。
            1,
        )
        .map_err(|error| PortError::Failed(format!("turn_identity_invalid:{error}")))?;
        // 这是当前 request_id 下事件的命令内序号，不是 run stream 的版本号。
        // append_event 会另行读取并 CAS stream_version；两种顺序标识不能互相替代。
        let mut sequence = 1u64;
        if let Some(previous_run_id) = predecessor {
            // 新 turn 先写 predecessor 关联，恢复/投影时才能知道它从哪一个 terminal run 续接。
            self.record_event(
                request_id,
                &mut sequence,
                "run.predecessor",
                json!({
                    "run_id": run_id,
                    "previous_run_id": previous_run_id,
                    "turn_id": turn_id,
                    "turn": turn,
                    "session_id": context.session_id,
                    "semantics": "new_turn_v2",
                }),
            )
            .await?;
        }
        let command = if predecessor.is_some() {
            "run.turn.v2"
        } else {
            "run.start"
        };
        // request.accepted 记录授权前的请求事实；后续 reject/authorized 事件共享同一 sequence。
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": command,
                "run_id": run_id,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
                "project_root": context.project_root,
                "role_id": context.role_id,
                "harness": HARNESS_ID,
            }),
        )
        .await?;

        if prompt.trim().is_empty() {
            // 空 prompt 没有可执行的用户意图；仍写 rejection 事件，使调用方获得可审计的拒绝原因。
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "prompt_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "prompt_required"));
        }

        // 角色目录是模型 prompt、模型 profile 和步数上限的权威来源，未知 role 不能进入 runner。
        let Some(role) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if !context.department_id.trim().is_empty() && context.department_id != role.department_id {
            // 如果请求带 department_id，它必须与角色目录一致，避免仅凭上下文字符串越权切换职责。
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_department_mismatch" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "role_department_mismatch",
            ));
        }

        // 蒸馏/普通 run 的前置 claim 同时检查已有状态和 sandbox 约束；失败只产生 blocked response。
        let distillation = match self
            .claim_distillation_start(
                &context,
                run_id,
                &prompt,
                history.is_empty(),
                sandbox.as_deref(),
            )
            .await
        {
            Ok(value) => value,
            Err(error) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({"run_id":run_id,"reason":error.to_string()}),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, error.to_string()));
            }
        };
        // 运行步数先取 ControlPlane 配置/角色默认值，再受公司级 runtime 限制；蒸馏最多运行一步。
        let max_steps_per_turn = self
            .company_runtime_step_limit(&context, self.max_steps_per_turn.unwrap_or(role.max_steps))
            .await?;
        let max_steps_per_turn = if distillation {
            max_steps_per_turn.min(1)
        } else {
            max_steps_per_turn
        };
        if max_steps_per_turn == 0 {
            // 零步运行无法产生模型结果，拒绝而不是创建一个永远没有 terminal 的 run。
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "runtime_config_invalid:max_steps_per_turn" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "runtime_config_invalid:max_steps_per_turn",
            ));
        }

        // sandbox 由授权 helper 统一解析；不能直接相信入口传入的字符串。
        let sandbox = match authorized_harness_sandbox(&context, sandbox.as_deref()) {
            Ok(sandbox) => sandbox,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        // 将项目/公司 run scope 绑定到本次上下文，后续 capability 检查会读取该绑定。
        if let Err(error) = self
            .bind_company_run_scope(&mut context, sandbox, true)
            .await
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({"reason":error.to_string(),"run_id":run_id}),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, error.to_string()));
        }

        // 这些预算和 authority 版本会写入 run.authorized，供 runner、broker 和恢复逻辑使用。
        let runtime_budget = self
            .runtime_budget_for_run(&context, max_steps_per_turn)
            .await?;
        let authority_revision = self.authority_revision(&context.project_root).await?;
        let authority_epoch = self
            .authority_epoch(&context.project_root)
            .await?
            // 没有历史授权记录时从第一个有效 epoch 开始；账本里的 0 表示尚未初始化。
            .unwrap_or(1);
        self.record_event(
            request_id,
            &mut sequence,
            "run.authorized",
            json!({
                "run_id": run_id,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
                "project_root": context.project_root,
                "role_id": context.role_id,
                "department_id": context.department_id,
                "harness": HARNESS_ID,
                "sandbox": sandbox,
                "capability_mode": "brokered",
                "max_steps_per_turn": max_steps_per_turn,
                "runtime_budget":runtime_budget,
                "authority_revision":authority_revision,
                "authority_epoch":authority_epoch,
                "turn_id":kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
                "turn":turn,
                "role_prompt_hash": role.prompt_hash,
                "model_profile": role.model_profile,
                "role_spec_schema": role.schema,
                "role_version": role.version,
                "role_catalog_schema": kiana_domain::ROLE_CATALOG_SCHEMA,
                "role_catalog_version": kiana_domain::SchemaVersion::new(1, 0),
                "role_input_schema": role.input_schema,
                "role_output_schema": role.output_schema,
            }),
        )
        .await?;

        // 写完授权事实后才能检查恢复 checkpoint；checkpoint 返回响应时表示已有状态接管了本次输入。
        if let Some(response) = self
            .checkpoint_before_input(&context, run_id, &mut sequence)
            .await?
        {
            return Ok(response);
        }

        // prompt 是 EventLog 中的事实输入，模型实际收到的 PromptBundle 在之后随 runner 命令传入。
        self.record_event(
            request_id,
            &mut sequence,
            "run.prompt",
            json!({
                "run_id": run_id,
                "session_id": context.session_id,
                "turn_id": turn_id,
                "turn": turn.clone(),
                "text": &prompt,
            }),
        )
        .await?;

        // 第一次模型/工具步骤前先登记 session→run 索引并建立取消 watch，
        // 并发到来的取消请求才能按 session 找到 run，并把取消信号送给正在运行的 shell.exec。
        // terminal scope guard 则保证提前返回时也执行本次生命周期的清理。
        self.remember_session(&context, run_id);
        let _cancel_rx = self.watch_cancel(run_id);
        let _terminal_scope = self.begin_terminal_scope(run_id);

        let mut prompt_bundle = kiana_domain::PromptBundle::for_role(&role);
        if distillation {
            // PromptSection 按 (order, name, source) 稳定排序；0 是安全提示，100 是角色提示，
            // 180 让蒸馏指令排在角色提示之后。它只补充模型上下文，不扩大工具授权集合。
            prompt_bundle.sections.push(kiana_domain::PromptSection {
                name: "memory_distillation".to_owned(),
                order: 180,
                text: kiana_domain::MEMORY_DISTILL_SYSTEM.to_owned(),
                source: kiana_domain::MEMORY_DISTILL_PROMPT_SOURCE.to_owned(),
                authority: kiana_domain::PromptAuthority::Product,
            });
        }

        // runner 固化本轮角色、profile、预算和 authority 代次；后续模型请求从这份 assignment 读取。
        let assignment_binding = self.runner.bind_model_assignment(
            run_id,
            kiana_domain::ModelAssignment {
                schema: "kiana.model-assignment.v1".to_owned(),
                run_id,
                turn_id: kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
                role_id: role.role_id.clone(),
                role_version: Some(role.version),
                catalog_version: Some(kiana_domain::SchemaVersion::new(1, 0)),
                prompt_hash: Some(role.prompt_hash.clone()),
                input_schema: Some(role.input_schema.clone()),
                output_schema: Some(role.output_schema.clone()),
                profile: role.model_profile.clone(),
                project_root: context.project_root.clone(),
                project_trusted: context.project_trusted,
                authority_revision: authority_revision.clone(),
                max_wall_time_ms: runtime_budget.max_wall_time_ms,
                runtime_budget: Some(runtime_budget.clone()),
            },
        );

        // 模型 assignment 绑定失败与 Runner 启动失败共用终态清理；绑定被拒绝时绝不能发送 Start。
        // 将历史、prompt bundle 和 sandbox 一次性送入 runner；这里仍未直接调用模型，loop 由 runner 执行。
        let start_result = match assignment_binding {
            Ok(()) => {
                self.runner
                    .send(RunnerCommand::start_in_with_history_and_turn(
                        run_id,
                        prompt,
                        history,
                        context.project_root.clone(),
                        sandbox.to_owned(),
                        prompt_bundle.encode().map_err(|error| {
                            CoreError::from(PortError::Failed(format!(
                                "prompt_bundle_invalid:{error}"
                            )))
                        })?,
                        context.project_trusted,
                        max_steps_per_turn,
                        Some(turn_id),
                    ))
                    .await
            }
            Err(error) => Err(error),
        };
        let pending_events = match start_result {
            Ok(events) => events,
            Err(error) => {
                // 迟到端口错误不能覆盖取消已提交的 Unknown，也不能提前释放其 session/watch fence。
                let reason = redact_event_text(&error.to_string());
                if let Some(response) = self
                    .record_drive_terminal(
                        &context,
                        run_id,
                        sandbox,
                        &mut sequence,
                        "run.failed",
                        json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                    )
                    .await?
                {
                    return Ok(response);
                }
                self.forget_session(context.session_id.as_str(), run_id);
                self.clear_cancel(run_id);
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, sandbox),
                    error: Some(reason),
                });
            }
        };

        // 首批 runner 事件交给统一 drive_run，后者消费工具事件并推进到 terminal。
        self.drive_run(&context, run_id, sandbox, pending_events, &mut sequence)
            .await
    }

    /// 新版协议语义：已进入终态的 Run 不重新打开；新 turn 创建新的 Run，并记录前驱关系。
    /// 在已有 Run 上开一个**新 turn**。
    ///
    /// 【和 `continue_run` 的区别】
    /// 这里要求前一个 Run 已有确定终态，再创建新 Run 并把前一个 Run 记为 predecessor；
    /// `continue_run` 是兼容 v1 的同一 Run 内 continuation，保留旧协议的逐轮解释。
    /// 两条路径对身份、turn 编号和历史连接方式不同，不能合并成靠 flag 切换的入口，
    /// 否则调用方可能把旧 Run 重新打开，或丢失新 Run 与前驱之间的审计关系。
    ///
    /// 【调用者】`kiana-core::commands::handle_command()` 将 `run.turn.v2` 路由到这里。
    ///
    /// 【输入】请求上下文、用户 prompt、可选 sandbox，以及可选的前一 Run ID；未提供前驱时
    /// 会按 session 解析当前 Run。
    ///
    /// 【输出】复用 `start_run_with_id()` 的 `CoreResponse`；缺少或不适合的前驱返回 Blocked，
    /// 存储、历史绑定或 Runner 端口故障返回 `CoreError`。
    ///
    /// 【副作用与数据流】检查蒸馏会话和前驱终态 → 读取该 session 的模型协议历史 → 为新 Run
    /// 分配 ID 并绑定历史 → 写 predecessor 与新 prompt/授权事实 → 由 Runner 产生事件，最终
    /// 经 `drive_run()` 转成响应。
    ///
    /// 【为什么拒绝 `ResultUnknown`】这表示此前的外部副作用结果还未能确认；把它直接当作
    /// 已完成历史继续，可能让用户误以为未知操作已经安全结束。
    ///
    /// 【memory distill 的特殊分支】
    /// 蒸馏会话有自己的一套生命周期，不走普通 run 的继续路径，所以在前置就被挡住。
    /// 挡住而不是放行再走特殊处理，是为了避免普通路径和蒸馏路径互相污染状态。
    pub async fn continue_new_turn(
        &self,
        context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
        previous: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        // 蒸馏会话有独立生命周期，禁止把它伪装成普通的新 turn。
        if context
            .session_id
            .as_str()
            .starts_with(kiana_domain::MEMORY_DISTILL_SESSION_PREFIX)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "memory_distillation_continue_denied",
            ));
        }
        // previous 缺省时按 session 解析；解析失败以 blocked response 返回，不创建孤立新 run。
        let previous = match self.resolve_run_id(&context, previous).await? {
            Ok(id) => id,
            Err(reason) => return Ok(CoreResponse::blocked(context.request_id, reason)),
        };
        // 新 turn 只能从已有终态的 Run 开始；仍在执行的 Run 必须由原生命周期继续推进，不能另开分支。
        let state = self.run_state(previous).await?;
        if state.outcome.is_none() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_not_terminal_use_resume",
            ));
        }
        if state.outcome == Some(RunOutcome::ResultUnknown) {
            // result_unknown 代表外部结果尚未核对，不能把未知状态当作可安全继承的历史。
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_unknown_requires_reconciliation",
            ));
        }
        // 协议历史先绑定到新 Run 的模型上下文；EventLog 中的 predecessor 关系随后由统一启动路径记录。
        let history = self
            .model_protocol_history(context.session_id.as_str())
            .await?;
        let run_id = RunId::new();
        self.runner.bind_model_history(run_id, history)?;
        self.start_run_with_id(
            context,
            prompt,
            Vec::new(),
            sandbox,
            Some(run_id),
            Some(previous),
        )
        .await
    }

    /// 兼容 v1 的同一 Run continuation，保留旧协议的逐轮解释。
    ///
    /// 【调用者】旧版命令/入口由 `harness_run.rs` 调用；DaemonHost 的 Continue 请求也映射到这里。
    /// 新版 `run.turn.v2` 命令应走 `continue_new_turn()`。
    ///
    /// 【输入】请求上下文、继续输入、可选 sandbox 和可选 Run ID；缺少 Run ID 时按 session 解析。
    /// 【输出】成功、等待审批、澄清等待或失败等 `CoreResponse`；底层端口故障返回 `CoreError`。
    /// 【副作用】保留同一个 run stream，记录本次 turn/prompt 并发送
    /// `RunnerCommand::continue_run`；产生的 RunnerEvent 交给 `drive_run()` 统一消费。
    /// 【边界】ResultUnknown、待审批以及输入/权限校验失败不会被当作可安全继续的状态。
    /// 【设计原因】保留兼容协议行为，同时让 v1/v2 最终共享同一事件消费和能力授权路径。
    pub async fn continue_run(
        &self,
        mut context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
        run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        // v1 continue 保留旧 run 内追加 turn 的语义；memory distill 仍必须走专用入口。
        if context
            .session_id
            .as_str()
            .starts_with(kiana_domain::MEMORY_DISTILL_SESSION_PREFIX)
        {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({"reason":"memory_distillation_continue_denied"}),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "memory_distillation_continue_denied",
            ));
        }
        let mut sequence = 1u64;
        // 解析 run id 后，错误也要记录 request.accepted/rejected，保持旧协议的事件形状。
        let run_id = match self.resolve_run_id(&context, run_id).await? {
            Ok(run_id) => run_id,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "request.accepted",
                    json!({ "command": "run.continue" }),
                )
                .await?;
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        // 旧语义的 turn number 从已有 prompt 数量推导，LegacyContinue 明确区别于 v2 NewTurn。
        let prior = self.events.read_stream("run", &run_id.to_string()).await?;
        let state = project_run_state(run_id, &prior)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        if state.outcome == Some(RunOutcome::ResultUnknown) {
            return Ok(CoreResponse::blocked(
                request_id,
                "run_unknown_requires_reconciliation",
            ));
        }
        let legacy_turn_number = prior
            .iter()
            .filter(|event| event.kind == "run.prompt")
            .count()
            .saturating_add(1) as u64;
        let legacy_turn = kiana_domain::TurnIdentity::new(
            context.session_id.as_str(),
            run_id,
            kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
            None,
            kiana_domain::TurnSemantics::LegacyContinue,
            legacy_turn_number,
        )
        .map_err(|error| PortError::Failed(format!("turn_identity_invalid:{error}")))?;

        let has_pending = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|pending| pending.run_id == run_id);
        // 有待审批 invocation 或 AwaitingApproval 状态时不能并行注入新 prompt，避免绕过审批顺序。
        if has_pending
            || self
                .run_state(run_id)
                .await
                .is_ok_and(|state| state.phase == RunPhase::AwaitingApproval)
        {
            return Ok(CoreResponse::blocked(request_id, "run_awaiting_approval"));
        }
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.continue",
                "run_id": run_id,
                "turn_id": legacy_turn.turn_id,
                "turn": legacy_turn,
                "harness": HARNESS_ID,
            }),
        )
        .await?;

        if prompt.trim().is_empty() {
            // 空输入仍写拒绝事件，调用方可用 request_id 对应到明确原因。
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "prompt_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "prompt_required"));
        }

        // 继续路径仍重新解析 sandbox，不沿用不可信的入口字符串。
        let sandbox = match authorized_harness_sandbox(&context, sandbox.as_deref()) {
            Ok(sandbox) => sandbox,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        // 旧 run 的 scope 绑定为 false，表示这是现有 run 的继续而非新 run 初始化。
        if let Err(error) = self
            .bind_company_run_scope(&mut context, sandbox, false)
            .await
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({"reason":error.to_string(),"run_id":run_id}),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, error.to_string()));
        }

        // checkpoint 可能发现 cancel/terminal 状态，命中时直接返回其 response。
        if let Some(response) = self
            .checkpoint_before_input(&context, run_id, &mut sequence)
            .await?
        {
            return Ok(response);
        }

        // 继续输入也要写入 run.prompt，历史投影才能区分用户新增 turn 与模型事件。
        if let Err(error) = self
            .record_event(
                request_id,
                &mut sequence,
                "run.prompt",
                json!({
                    "run_id": run_id,
                    "session_id": context.session_id,
                    "turn_id": legacy_turn.turn_id,
                    "turn": legacy_turn.clone(),
                    "text": &prompt,
                }),
            )
            .await
        {
            if matches!(&error, CoreError::Port(PortError::Conflict(code)) if code == "run_unknown_requires_reconciliation")
            {
                return Ok(CoreResponse::blocked(
                    request_id,
                    "run_unknown_requires_reconciliation",
                ));
            }
            return Err(error);
        }

        // TerminalScopeGuard 在函数离开时负责终态清理；runner 只负责产生事件，不直接写 EventLog。
        let _terminal_scope = self.begin_terminal_scope(run_id);
        let pending_events = match self
            .runner
            .send(RunnerCommand::continue_run(run_id, prompt))
            .await
        {
            Ok(events) => events,
            Err(error) => {
                // 失败原因先脱敏，再写统一 run.failed 终态，避免 provider/工具内部文本泄漏。
                let reason = redact_event_text(&error.to_string());
                if let Some(response) = self
                    .record_drive_terminal(
                        &context,
                        run_id,
                        sandbox,
                        &mut sequence,
                        "run.failed",
                        json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                    )
                    .await?
                {
                    return Ok(response);
                }
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, sandbox),
                    error: Some(reason),
                });
            }
        };

        // v1 和 v2 最终复用同一个 drive_run 事件驱动循环。
        self.drive_run(&context, run_id, sandbox, pending_events, &mut sequence)
            .await
    }

    /// 从 EventLog 中查出当前 run 最近一次 `run.prompt` 对应的 turn ID。
    ///
    /// 【调用者】`queue_run_input()` 在接受 steer 或确定 next-step 目标前调用。
    /// 【输入】稳定的 Run ID。
    /// 【输出】最新 prompt 携带的 TurnId；没有 prompt 或旧事件载荷无法解析时返回 `None`。
    /// 【副作用/失败】不写事件、不修改状态；EventStore 读取失败会作为 `CoreError` 传回输入入口。
    /// steer 的并发前置条件必须与 Core 已记录的 turn 比较，不能依赖入口或 UI 的本地缓存，
    /// 否则旧 UI 可能把消息错误地附加到另一轮。
    async fn current_run_turn(
        &self,
        run_id: RunId,
    ) -> Result<Option<kiana_domain::TurnId>, CoreError> {
        // 从 run stream 反向寻找最近的 prompt；事件载荷可能是字符串或结构化 TurnId，兼容两种历史编码。
        let events = self.events.read_stream("run", &run_id.to_string()).await?;
        Ok(events
            .iter()
            .rev()
            .find(|event| event.kind == "run.prompt")
            .and_then(|event| {
                event.data["turn_id"]
                    .as_str()
                    .and_then(kiana_domain::TurnId::parse_str)
                    .or_else(|| {
                        serde_json::from_value::<kiana_domain::TurnId>(
                            event.data["turn_id"].clone(),
                        )
                        .ok()
                    })
            }))
    }

    /// 校验并排队一条 steer/inject 输入，再生成表示“已接受排队”的 InputReceipt。
    ///
    /// 【调用者】公开的 `steer_run()` 和 `inject_run()` 共用此实现，区别由目标 turn 与 expected turn 参数表达。
    ///
    /// 【输入】请求上下文、Run ID、来源标签、消息文本、投递目标、可选目标 turn，以及可选的
    /// 乐观并发前置 turn。`expected_turn_id` 只由 steer 提供；inject 留空且不会在此处启动空闲 run。
    ///
    /// 【输出】排队成功时返回含 `InputReceipt` 的 completed response。Receipt 证明 Core/Runner
    /// 接受了排队请求，不证明模型已经读取或处理了文本。
    ///
    /// 【副作用】先把脱敏后的文本写成 `run.input.accepted`，再把原始文本发给 Runner inbox；
    /// 若 Runner 明确返回 Failed，则追加 `run.input.claimed` 的 rejected 记录并返回 Blocked。
    /// EventStore 或 Runner 端口故障向调用方返回 `CoreError`。
    ///
    /// 【边界与设计原因】先验证消息大小、来源、目标和 turn 归属，防止无界输入、来源不明的
    /// 上下文或过期 steer 进入执行链；日志文本经过脱敏，而模型仍收到原始文本。
    async fn queue_run_input(
        &self,
        context: RequestContext,
        run_id: RunId,
        target: &str,
        source: String,
        text: String,
        target_turn_id: Option<kiana_domain::TurnId>,
        expected_turn_id: Option<kiana_domain::TurnId>,
    ) -> Result<CoreResponse, CoreError> {
        // String::len() 按 UTF-8 字节计数：消息最多 256 KiB，避免单条 inbox 输入无界占用内存；
        // trim 后为空也不能形成有意义的引导，因此在写事件和调用 Runner 前拒绝。
        if text.trim().is_empty() || text.len() > 256 * 1024 {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "inbox_message_invalid",
            ));
        }
        // source 是上下文来源标签，最多 256 个 UTF-8 字节且不能含 NUL，便于审计和后续展示。
        if source.trim().is_empty() || source.len() > 256 || source.contains('\0') {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "inbox_source_invalid",
            ));
        }
        // 只接受协议规定的下一步/下一轮目标；下划线别名在验证后归一化为 wire canonical 值。
        if !matches!(
            target,
            "next-step" | "next_step" | "next-turn" | "next_turn"
        ) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "inbox_target_invalid",
            ));
        }
        let run_id = match self.resolve_run_id(&context, Some(run_id)).await? {
            Ok(run_id) => run_id,
            Err(reason) => return Ok(CoreResponse::blocked(context.request_id, reason)),
        };
        // 读取当前 prompt 的 turn，用于 optimistic concurrency 检查和 next-step 默认目标。
        let current_turn = self.current_run_turn(run_id).await?;
        if expected_turn_id.is_some() && expected_turn_id != current_turn {
            // 调用方持有旧 turn 时拒绝 steer，避免消息落到错误的模型轮次。
            return Ok(CoreResponse::blocked(
                context.request_id,
                "stale_turn_steer",
            ));
        }
        let target = if target == "next_step" {
            "next-step"
        } else if target == "next_turn" {
            "next-turn"
        } else {
            target
        };
        let target_turn_id = if target == "next-step" {
            target_turn_id.or(current_turn)
        } else {
            target_turn_id
        };
        if target == "next-step" && target_turn_id != current_turn {
            // next-step 只能指向当前 turn；显式传入另一个 turn 等同于过期 steer。
            return Ok(CoreResponse::blocked(
                context.request_id,
                "stale_turn_steer",
            ));
        }
        let input_id = kiana_domain::InputId::new();
        // received_sequence 关联本请求产生的 accepted/claimed 事实，不代表模型实际处理顺序。
        let mut sequence = 1u64;
        let accepted_sequence = sequence;
        // 事件里的文本先经过统一脱敏；原始 text 只在随后 runner 注入，供模型实际使用。
        let safe_text = kiana_domain::redact_text(&text);
        self.record_event(
            context.request_id,
            &mut sequence,
            "run.input.accepted",
            json!({
                "run_id": run_id,
                "input_id": input_id,
                "source": source,
                "target": target,
                "target_turn_id": target_turn_id,
                "text": safe_text,
                "received_sequence": accepted_sequence,
                "disposition": "accepted",
            }),
        )
        .await?;
        // runner 的 Inject 才把输入放进 inbox；Core 先写 accepted，失败时再追加 claimed=rejected。
        let events = self
            .runner
            .send(RunnerCommand::Inject {
                run_id,
                input_id,
                source,
                target: target.to_owned(),
                target_turn_id,
                text,
            })
            .await?;
        if let Some(RunnerEvent::Failed { error, .. }) = events.first() {
            // 只看首事件的 Failed 是因为 Inject 返回的是本次命令产生的事件批次；失败时没有 receipt。
            self.record_event(
                context.request_id,
                &mut sequence,
                "run.input.claimed",
                json!({"run_id":run_id,"input_id":input_id,"source":"control_plane","target":target,
                    "target_turn_id":target_turn_id,"received_sequence":accepted_sequence,"disposition":"rejected","error":error}),
            )
            .await?;
            return Ok(CoreResponse::blocked(context.request_id, error.clone()));
        }
        // InputReceipt 绑定 input/run/target/sequence，调用方可据此确认消息已排入而非已被模型消费。
        let receipt = kiana_domain::InputReceipt::new(
            input_id,
            run_id,
            "control_plane",
            target,
            accepted_sequence,
            kiana_domain::InputDisposition::Accepted,
        )
        .map_err(PortError::Failed)?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"run_id":run_id,"input_id":input_id,"target":target,"target_turn_id":target_turn_id,"receipt":receipt,"queued":true}),
        ))
    }

    /// 将一条绑定当前 turn 的引导排入 Runner inbox。
    /// 在**当前 turn 进行中**插入一条引导。
    ///
    /// 【作用】
    /// 用户在模型还在跑的时候追加一句「顺便也看看 X」。这条输入进入当前 turn，
    /// Runner 会在适合接收 inbox 的安全步骤边界处理它，通常由下一次模型调用看到。
    /// 此函数返回只表示已排队，不表示模型已消费。
    ///
    /// 【为什么必须带 `expected_turn_id`】
    /// 见文件头：这是乐观并发。调用方声明「我以为现在轮到这一轮」，
    /// 不匹配就拒绝，否则 steer 会被写进错误的 turn。
    ///
    /// 【注意它传的是 `Some(expected_turn_id)` 两次】
    /// 一次是「目标 turn」（写进哪一轮），一次是「前置条件」（必须还是这一轮）。
    /// 对 steer 来说两者相同，因为它只对当前轮生效。
    ///
    /// 【调用者/失败】DaemonHost 的 Steer 请求映射到这里；如果 EventLog 当前 turn 已推进，
    /// 返回 `stale_turn_steer`。Runner 明确拒绝时返回 Blocked，存储或通道故障返回 `CoreError`。
    pub async fn steer_run(
        &self,
        context: RequestContext,
        run_id: RunId,
        expected_turn_id: kiana_domain::TurnId,
        text: String,
    ) -> Result<CoreResponse, CoreError> {
        self.queue_run_input(
            context,
            run_id,
            "next-step",
            "steer".to_owned(),
            text,
            Some(expected_turn_id),
            Some(expected_turn_id),
        )
        .await
    }

    /// 注入一条**带来源标签**的上下文输入，且**不唤醒**空闲的 run。
    ///
    /// 【和 `steer_run` 的三点区别】
    /// 1. `target_turn_id` 是 `Option`，可以指定某一轮，也可以不指定；
    /// 2. **没有** `expected_turn_id`，也就是不做前置条件校验——注入的语义是
    ///    「把这条信息放进去」，不是「插进正在跑的那一轮」；
    /// 3. **不唤醒空闲的 run**。空闲时注入的内容会在 run 下次被驱动时读到，
    ///    但不会为了这一条输入去启动一次模型调用。
    ///
    /// 【为什么 `source` 是必填的】
    /// 因为注入的内容会进入模型的上下文。来源决定了它在上下文里意味着什么
    /// （检索结果、用户补充、外部事实）。没有来源标签，模型无法区分
    /// 「这是检索到的资料」和「这是用户刚刚说的话」，而这两者的可信度完全不同。
    ///
    /// 【调用者】DaemonHost 的 Inject 请求映射到这里。
    /// 【输出】经 `queue_run_input()` 完成校验、EventLog 记录、Runner 入队，并返回 InputReceipt。
    /// 【失败情况】空文本、来源/目标无效、run 无法解析或 Runner 明确拒绝时返回 Blocked；
    /// EventStore/Runner 端口故障返回 `CoreError`。
    pub async fn inject_run(
        &self,
        context: RequestContext,
        run_id: RunId,
        target: String,
        source: String,
        text: String,
        target_turn_id: Option<kiana_domain::TurnId>,
    ) -> Result<CoreResponse, CoreError> {
        self.queue_run_input(context, run_id, &target, source, text, target_turn_id, None)
            .await
    }

    /// 以幂等方式提交当前 turn 的取消意图。
    ///
    /// 【为什么返回值是 `bool` 而不是 `()`】
    /// `true` 表示同一取消命令已有持久提交（包括幂等重放），`false` 表示当前 turn 在提交前
    /// 已经结束。它不表示“这是第一次取消”；冲突或存储错误使用 `CoreError` 返回。
    ///
    /// 【取消不等于已停止】
    /// 这里记录的是**请求**。真正确认停止需要进程组确认，那由后面的路径完成。
    /// 仓库的约定是：`cancel_requested` 与 `stop_confirmed` 是两个不同的事实，
    /// 中间不能合并——否则「我请求了取消」会被读成「它已经停了」。
    ///
    /// 【调用者/输入】只由 `cancel_run()` 调用；输入包括授权上下文、Run ID、已脱敏原因、
    /// 待取消的 invocation RequestId 列表及当前请求事件序号。
    /// 【输出】`true` 表示取消意图已提交或同命令已提交过，`false` 表示当前 turn 已有终态；
    /// 冲突、EventStore 故障或连续 CAS 竞争超过上限则返回 `CoreError`。
    /// 【副作用】使用稳定 command ID/digest 和 stream-version CAS 记录 `run.cancelling`；
    /// 本函数只持久化意图，不发送取消信号，也不证明 Runner 或子进程已经停止。
    /// 通过幂等 command receipt 和 stream-version CAS 写入 `run.cancelling` 事实。
    ///
    /// 相同 command ID 但摘要不同（例如原因、actor 或目标 invocation 不同）会作为冲突拒绝，
    /// 不能把第二个不同取消意图静默并入第一次提交。
    /// 返回 true 表示本次取消已提交或是同一命令的重放，false 表示目标 turn 已 terminal；
    /// 并发修改最多重读并重试八次，目标、actor、原因不一致则返回冲突。
    async fn record_cancel_requested(
        &self,
        context: &RequestContext,
        run_id: RunId,
        reason: &str,
        target_invocation_ids: &[RequestId],
        sequence: &mut u64,
    ) -> Result<bool, CoreError> {
        // 命令 ID/digest 让重复取消幂等，同时把 actor、reason 和目标 invocation 纳入冲突检测。
        let command_id = kiana_domain::derived_request_id("run.cancel", &run_id.to_string());
        let reason = redact_event_text(reason);
        let mut canonical_targets = target_invocation_ids.to_vec();
        // 目标排序去重后才计算 digest，避免同一组目标因调用方顺序不同被误判为两条命令。
        canonical_targets.sort();
        canonical_targets.dedup();
        let command_digest = kiana_domain::json_digest(&json!({
            "command":"run.cancel",
            "run_id":run_id,
            "reason":reason,
            "actor_id":context.actor_id,
            "targets":canonical_targets,
        }));
        // 最多重试 8 次 CAS 冲突；每轮重新读取 run stream，不能使用上一轮过期版本。
        for _ in 0..8 {
            if let Some(receipt) = self.events.read_command(&command_id).await? {
                // 已有 receipt 表示命令提交过；digest 不同说明相同 command_id 被重用，必须报冲突。
                if receipt.command_digest != command_digest.trim_start_matches("sha256:") {
                    return Err(
                        PortError::Conflict("run_cancel_command_conflict".to_owned()).into(),
                    );
                }
                *sequence = sequence.saturating_add(1);
                return Ok(true);
            }
            let prior = self.events.read_stream("run", &run_id.to_string()).await?;
            // 从最近 prompt 开始判断当前 turn 是否已经有 terminal 事实；历史 turn 的终态不阻止新 turn。
            let turn = prior
                .iter()
                .rposition(|event| event.kind == "run.prompt")
                .unwrap_or(0);
            if prior[turn..].iter().any(|event| {
                matches!(
                    event.kind.as_str(),
                    "run.completed" | "run.failed" | "run.cancelled" | "run.result_unknown"
                )
            }) {
                return Ok(false);
            }
            if let Some(existing) = prior.iter().find(|event| event.kind == "run.cancelling") {
                // 已有 cancelling 事件时必须逐字段核对，确认这是同一取消意图而非并发冲突。
                let fact = kiana_domain::RunCancellationFact::from_json(
                    &existing.data["cancellation_fact"],
                )
                .map_err(PortError::Failed)?;
                if fact.run_id != run_id
                    || fact.reason_digest != kiana_domain::json_digest(&json!({"reason":reason}))
                    || fact.actor_id != context.actor_id
                    || fact.target_invocation_ids != canonical_targets
                {
                    return Err(
                        PortError::Conflict("run_cancel_command_conflict".to_owned()).into(),
                    );
                }
                *sequence = sequence.saturating_add(1);
                return Ok(true);
            }
            let version = prior
                .iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0);
            // 取消事实需要可信时间戳；系统时钟回退时拒绝写入，而不是伪造顺序。
            let at_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
                .map_err(|_| PortError::Failed("cancel_clock_untrusted".to_owned()))?;
            let fact = kiana_domain::RunCancellationFact::new(
                run_id,
                command_id,
                kiana_domain::RunCancellationState::Stopping,
                &reason,
                context.actor_id.clone(),
                canonical_targets.clone(),
                version,
                true,
                None,
                at_unix_ms,
            )
            .map_err(PortError::Failed)?;
            let data = redact_event_value(&json!({
                "run_id": run_id,
                "reason": reason,
                "cancellation_state": "stopping",
                "cancellation_reason": reason,
                "cancel_actor_id": context.actor_id,
                "cancellation_targets": canonical_targets,
                "cancellation_at_unix_ms": at_unix_ms,
                "cancellation_fact": fact,
            }));
            let event = RuntimeEvent::new(context.request_id, *sequence, "run.cancelling", data)
                .map_err(|error| PortError::Failed(error.to_string()))?
                .with_stream_metadata("run", run_id.to_string(), version + 1)
                .with_idempotency_key(format!("run:{run_id}:cancel"));
            let batch = kiana_domain::TransitionBatch {
                command_id,
                command_digest: command_digest.clone(),
                expected_versions: vec![kiana_domain::AggregateVersion::new(
                    "run",
                    run_id.to_string(),
                    version,
                )],
                events: vec![event],
            };
            // commit_confirmed 以 expected stream version 做 CAS；并发写入时回到下一轮重读。
            match super::dispatch::commit_confirmed(self.events.as_ref(), batch).await {
                Ok(_) => {
                    *sequence = sequence.saturating_add(1);
                    self.invalidate_invocation_projection(run_id);
                    return Ok(true);
                }
                Err(PortError::Conflict(_)) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(PortError::Conflict("run_cancel_contention".to_owned()).into())
    }

    /// 请求取消 run，并区分“已请求”“已确认停止”和“副作用结果未知”。
    ///
    /// 【调用者】`harness_run.rs` 的入口和 `DaemonHost` 的 Cancel 请求都会映射到这里。
    ///
    /// 【输入】请求身份上下文、可选 Run ID、用户给出的取消原因；空原因会规范化为 `user`，
    /// 并在跨越 Runner 和响应边界前脱敏。
    ///
    /// 【输出】返回已知 terminal 状态，或新产生的 Cancelled/ResultUnknown/Failed 响应。
    /// Run 无法解析时写 `request.accepted` 与 `run.rejected` 并返回 Blocked；EventStore 等端口故障
    /// 返回 `CoreError`。
    ///
    /// 【副作用/核心流程】使待审批 invocation 失效并记录 `not_executed` 结果 → 以 CAS 持久化
    /// `run.cancelling` → 同时通知 capability supervisor 与 Runner → 等待 stop proof → 核对 Runner
    /// 事件的 Run ID、Cancelled 错误和是否存在竞争的完成/失败事件 → 记录 terminal 事实。
    ///
    /// 【为什么不能收到 cancel 就返回 Cancelled】取消信号只是请求，不证明 shell/MCP 等外部副作用
    /// 已停止。只有停止证明和匹配的 Runner 响应都齐全才写 `run.cancelled`；否则写
    /// `run.result_unknown`，调用方必须先 reconcile，不能把未知结果当作成功或安全重试。
    pub async fn cancel_run(
        &self,
        context: RequestContext,
        run_id: Option<RunId>,
        reason: String,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        // 空 reason 使用稳定的 user 标签；其后立即脱敏，因为 reason 会进入 runner 和直接响应。
        let reason = if reason.trim().is_empty() {
            "user".to_owned()
        } else {
            reason
        };
        // reason 还会传给 Runner 并出现在直接响应里；在入口脱敏，不能只依赖 EventLog 的二次脱敏。
        let reason = redact_event_text(&reason);
        let run_id = match self.resolve_run_id(&context, run_id).await? {
            Ok(run_id) => run_id,
            Err(code) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "request.accepted",
                    json!({ "command": "run.cancel" }),
                )
                .await?;
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": code }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, code));
            }
        };

        // cancel 的审批/授权范围沿用当前 context，但不改变原始 RequestContext 的身份字段。
        let context = self.approval_scope(&context).await?;
        if let Ok(state) = self.run_state(run_id).await {
            if let Some(outcome) = state.outcome {
                // terminal run 不再发取消命令，直接返回已知终态，保证取消是幂等查询。
                let status = match outcome {
                    RunOutcome::Completed => ExecutionStatus::Completed,
                    RunOutcome::Failed => ExecutionStatus::Failed,
                    RunOutcome::Cancelled => ExecutionStatus::Cancelled,
                    RunOutcome::ResultUnknown => ExecutionStatus::ResultUnknown,
                };
                return Ok(CoreResponse {
                    request_id,
                    status,
                    output: json!({"run_id":run_id,"session_id":context.session_id,"already_terminal":true}),
                    error: state.error,
                });
            }
        }

        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.cancel",
                "run_id": run_id,
                "reason": &reason,
            }),
        )
        .await?;

        // 先收集锁内的 pending invocation，再在锁外逐个 invalidate，避免异步 await 持有 Mutex。
        let pending_approvals: Vec<(ApprovalId, PendingInvocation)> = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(|(approval_id, pending)| {
                (pending.run_id == run_id).then_some((*approval_id, pending.clone()))
            })
            .collect();
        let cancellation_targets = pending_approvals
            .iter()
            .map(|(_, pending)| pending.request_id)
            .collect::<Vec<_>>();
        for (approval_id, pending) in pending_approvals {
            // 待审批工具取消后写 not_executed tool_result，Runner 不会把它误当成真实工具输出。
            if let Err(error) = self
                .approvals
                .invalidate(&context, approval_id, &reason)
                .await
            {
                let error = redact_event_text(&error.to_string());
                self.record_terminal_event(
                    request_id,
                    &mut sequence,
                    run_id,
                    "run.failed",
                    json!({
                        "run_id": run_id,
                        "error": &error,
                        "reason": "approval_invalidation_failed",
                        "approval_id": approval_id,
                    }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, "read-only"),
                    error: Some(format!("approval_invalidation_failed:{error}")),
                });
            }
            self.record_event(request_id,&mut sequence,"run.tool_result",json!({"run_id":run_id,
                "capability_request_id":pending.request_id,"call_id":pending.request.arguments["call_id"],
                "result":{"error":"cancelled:approval_pending","not_executed":true,"replay_safe":true},"not_executed":true})).await?;
            self.pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&approval_id);
        }

        let cancellation_committed = self
            .record_cancel_requested(
                &context,
                run_id,
                &reason,
                &cancellation_targets,
                &mut sequence,
            )
            .await?;
        if !cancellation_committed {
            // 另一个取消请求已提交或 run 在 CAS 前终止，重新读取 terminal 状态作为幂等响应。
            let state = self.run_state(run_id).await?;
            let Some(outcome) = state.outcome else {
                return Err(
                    PortError::Conflict("run_cancel_terminal_state_missing".to_owned()).into(),
                );
            };
            let status = match outcome {
                RunOutcome::Completed => ExecutionStatus::Completed,
                RunOutcome::Failed => ExecutionStatus::Failed,
                RunOutcome::Cancelled => ExecutionStatus::Cancelled,
                RunOutcome::ResultUnknown => ExecutionStatus::ResultUnknown,
            };
            return Ok(CoreResponse {
                request_id,
                status,
                output: json!({"run_id":run_id,"session_id":context.session_id,"already_terminal":true}),
                error: state.error,
            });
        }
        // 先写 cancelling 事实，再同时通知本地 capability supervisor 和 runner。
        self.signal_cancel(run_id);
        let events = match self
            .runner
            .send(RunnerCommand::Cancel {
                run_id,
                reason: reason.clone(),
            })
            .await
        {
            Ok(events) => events,
            Err(error) => {
                // runner 无法确认停止时结果未知；不能把“取消请求已发出”升级成 cancelled。
                let error = redact_event_text(&error.to_string());
                let error = format!("result_unknown:{error}");
                self.record_terminal_event(
                    request_id,
                    &mut sequence,
                    run_id,
                    "run.result_unknown",
                    json!({
                        "run_id": run_id,
                        "error": &error,
                        "cancellation_state": "result_unknown",
                        "cancellation_reason": &error,
                        "cancel_actor_id": context.actor_id,
                        "cancellation_targets": &cancellation_targets,
                        "stop_confirmed": false,
                    }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::ResultUnknown,
                    output: run_identity(&context, run_id, "read-only"),
                    error: Some(error),
                });
            }
        };

        // capability stop 是独立证据：即使 runner 返回了取消事件，也要确认正在执行的进程组已停。
        let stop_confirmed = self.await_capability_stop(run_id).await;
        for event in &events {
            if let RunnerEvent::ToolCancelled {
                run_id: event_run_id,
                request_id: tool_request_id,
                call_id,
                result,
            } = event
            {
                if *event_run_id == run_id {
                    self.record_event(request_id,&mut sequence,"run.tool_result",json!({
                        "run_id":run_id,"capability_request_id":tool_request_id,"call_id":call_id,
                        "result":result,"cancelled":true,"not_executed":true,
                    })).await?;
                }
            }
        }
        if !stop_confirmed {
            // 缺少 stop confirmation 时保守返回 result_unknown，等待后续 reconciliation。
            let error = "result_unknown:cancel_stop_unconfirmed";
            self.record_terminal_event(
                request_id,
                &mut sequence,
                run_id,
                "run.result_unknown",
                json!({
                    "run_id":run_id,
                    "error":error,
                    "cancellation_state":"result_unknown",
                    "cancellation_reason":error,
                    "cancel_actor_id":context.actor_id,
                    "cancellation_targets":&cancellation_targets,
                    "stop_confirmed":false,
                }),
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: run_identity(&context, run_id, "read-only"),
                error: Some(error.to_owned()),
            });
        }
        let response_run_ids_match = events.iter().all(|event| event.run_id() == run_id);
        let matching_failure_errors = events
            .iter()
            .filter_map(|event| match event {
                RunnerEvent::Failed {
                    run_id: event_run_id,
                    error,
                } if *event_run_id == run_id => Some(error.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let matching_non_cancel_error = matching_failure_errors
            .iter()
            .copied()
            .find(|error| lifecycle_error_code(error) != CapabilityErrorCode::Cancelled);
        let has_matching_completion = events.iter().any(|event| {
            matches!(
                event,
                RunnerEvent::Completed {
                    run_id: event_run_id,
                    ..
                } if *event_run_id == run_id
            )
        });
        let cancelled = response_run_ids_match
            && matching_failure_errors
                .iter()
                .any(|error| lifecycle_error_code(error) == CapabilityErrorCode::Cancelled)
            && matching_non_cancel_error.is_none()
            && !has_matching_completion;
        if cancelled {
            // 只有 run id、取消错误、无非取消失败、无 completion 四项同时满足才确认 cancelled。
            let cancelled = matching_failure_errors
                .iter()
                .copied()
                .find(|error| lifecycle_error_code(error) == CapabilityErrorCode::Cancelled)
                .map(redact_event_text)
                .expect("cancelled response was checked above");
            self.forget_session(context.session_id.as_str(), run_id);
            self.record_terminal_event(
                request_id,
                &mut sequence,
                run_id,
                "run.cancelled",
                json!({
                    "run_id": run_id,
                    "error": &cancelled,
                    "cancellation_state": "cancelled",
                    "cancellation_reason": &cancelled,
                    "cancel_actor_id": context.actor_id,
                    "cancellation_targets": &cancellation_targets,
                    "stop_confirmed": true,
                }),
            )
            .await?;
            self.settle_resumed_cell(
                &context,
                run_id,
                "read-only",
                ExecutionStatus::Cancelled,
                &mut sequence,
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Cancelled,
                output: run_identity(&context, run_id, "read-only"),
                error: Some(cancelled),
            });
        }

        // 任何响应不一致都不能安全宣称已取消，统一落 result_unknown 并保留诊断原因。
        let runner_error = if !response_run_ids_match {
            "cancel_response_run_id_mismatch".to_owned()
        } else if has_matching_completion {
            "cancel_confirmation_inconsistent".to_owned()
        } else if let Some(error) = matching_non_cancel_error {
            redact_event_text(error)
        } else {
            "cancel_confirmation_missing".to_owned()
        };
        let error = format!("result_unknown:{runner_error}");
        self.record_terminal_event(
            request_id,
            &mut sequence,
            run_id,
            "run.result_unknown",
            json!({
                "run_id": run_id,
                "error": &error,
                "cancellation_state": "result_unknown",
                "cancellation_reason": &error,
                "cancel_actor_id": context.actor_id,
                "cancellation_targets": &cancellation_targets,
                "stop_confirmed": false,
            }),
        )
        .await?;
        Ok(CoreResponse {
            request_id,
            status: ExecutionStatus::ResultUnknown,
            output: run_identity(&context, run_id, "read-only"),
            error: Some(error),
        })
    }

    /// 把一轮内连续到达的模型增量合并成一条持久化的 `run.delta`。
    ///
    /// 事件账本是事实源：按块落账会让每次 append 都付出全量 `read_stream` 算版本加 CAS 的
    /// 代价（见 `docs/streaming-unfreeze-plan.md` §5）。细粒度增量只走易失的展示通道。
    /// 【调用者】`drive_run()` 在遇到非增量事件以及处理完事件批次前调用。
    /// 【输入/输出】本次请求/Run 身份、命令内 event sequence 和累积文本 buffer；有内容时写一条
    /// EventLog 事件并清空 buffer，空 buffer 是无操作。
    /// 【失败/副作用】EventStore 写入失败向驱动循环返回 `CoreError`；`mem::take` 将 String 所有权
    /// 转移给事件载荷并留下一个可继续复用的空 buffer。
    async fn flush_run_delta(
        &self,
        request_id: RequestId,
        sequence: &mut u64,
        run_id: RunId,
        buffer: &mut String,
    ) -> Result<(), CoreError> {
        if buffer.is_empty() {
            // 避免为没有文本的边界事件写入空 delta。
            return Ok(());
        }
        // take 清空原 buffer 并把所有权交给本次事件，随后 runner 可继续复用同一 String。
        let text = std::mem::take(buffer);
        self.record_event(
            request_id,
            sequence,
            "run.delta",
            json!({ "run_id": run_id, "text": text }),
        )
        .await
    }

    /// 读取已经提交的取消未知终态，并验证它仍绑定到当前 Run/turn 的取消事实。
    ///
    /// 【调用者】`drive_run()` 在开始消费 Runner 事件前检查；`record_drive_terminal()` 在写 terminal
    /// 前及遇到 terminal 冲突后检查。
    /// 【输入】请求上下文、Run ID 和响应身份中的 sandbox 名称。
    /// 【输出】匹配到完整且绑定正确的 `run.result_unknown` 时返回对应 `CoreResponse`；没有该状态时为
    /// `None`。事件流损坏或取消事实绑定不一致时返回 `CoreError`，不会构造一个看似有效的未知结果。
    /// 【副作用】仅读取 EventLog 和投影状态，不写事件；它让迟到的 Runner 结果服从已提交的取消事实。
    async fn committed_cancel_unknown_response(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
    ) -> Result<Option<CoreResponse>, CoreError> {
        let prior = self.events.read_stream("run", &run_id.to_string()).await?;
        let turn = prior
            .iter()
            .rposition(|event| event.kind == "run.prompt")
            .unwrap_or(0);
        let current = &prior[turn..];
        let state = project_run_state(run_id, current)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        if state.outcome != Some(RunOutcome::ResultUnknown) {
            return Ok(None);
        }
        let Some(event) = current.iter().find(|event| {
            event.kind == "run.result_unknown"
                && event.data["cancellation_state"] == "result_unknown"
        }) else {
            return Ok(None);
        };
        let fact = kiana_domain::RunCancellationFact::from_json(&event.data["cancellation_fact"])
            .map_err(PortError::Failed)?;
        let command_id =
            kiana_domain::derived_request_id("run.terminal", &format!("{run_id}:{turn}"));
        if fact.run_id != run_id
            || fact.command_id != command_id
            || fact.state != kiana_domain::RunCancellationState::ResultUnknown
            || event.aggregate_type.as_deref() != Some("run")
            || event.aggregate_id.as_deref() != Some(run_id.to_string().as_str())
            || event
                .stream_version
                .is_none_or(|version| fact.expected_version.checked_add(1) != Some(version))
        {
            return Err(
                PortError::Failed("run_cancellation_fact_binding_invalid".to_owned()).into(),
            );
        }
        Ok(Some(CoreResponse {
            request_id: context.request_id,
            status: ExecutionStatus::ResultUnknown,
            output: run_identity(context, run_id, sandbox),
            error: state.error.map(|error| redact_event_text(&error)),
        }))
    }

    /// 记录 Runner 驱动循环的 terminal 结果，同时保护已提交的取消未知状态。
    ///
    /// 【调用者】新 run 启动失败、v1 continuation 失败及 `drive_run()` 的终态分支。
    /// 【输入】当前请求与 Run、sandbox、命令内 sequence、terminal 事件种类和载荷。
    /// 【输出】`None` 表示本次终态已成功记录；`Some(response)` 表示并发的取消未知事实先提交，
    /// 应直接返回该既有响应。其他 EventStore/校验冲突作为 `CoreError` 返回。
    /// 【设计原因】先读取消事实再写终态，并在 terminal CAS 冲突后重读；否则晚到的模型完成/失败
    /// 可能覆盖更早已经持久化的 `ResultUnknown`，让响应与事件事实矛盾。
    async fn record_drive_terminal(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        sequence: &mut u64,
        kind: &str,
        data: Value,
    ) -> Result<Option<CoreResponse>, CoreError> {
        if let Some(response) = self
            .committed_cancel_unknown_response(context, run_id, sandbox)
            .await?
        {
            return Ok(Some(response));
        }
        match self
            .record_terminal_event(context.request_id, sequence, run_id, kind, data)
            .await
        {
            Ok(_) => Ok(None),
            Err(error) => {
                if matches!(&error, CoreError::Port(PortError::Conflict(code)) if code == "run_terminal_conflict")
                {
                    if let Some(response) = self
                        .committed_cancel_unknown_response(context, run_id, sandbox)
                        .await?
                    {
                        return Ok(Some(response));
                    }
                }
                Err(error)
            }
        }
    }

    /// 在 ControlPlane 中消费 RunnerEvent、代办授权后的能力请求并形成 run 响应。
    ///
    /// 【调用者】`start_run_with_id()` 与 `continue_run()` 在成功启动/继续 Runner 后调用。
    /// 【输入】请求上下文、Run ID、响应 sandbox、Runner 首批事件，以及 request-local sequence。
    /// 能力完成后产生的新 RunnerEvent 会追加到同一待处理队列。
    ///
    /// 【输出】Completed、Failed、AwaitingApproval、澄清等待或 ResultUnknown 等 `CoreResponse`；
    /// EventStore、Broker、审批/投影端口错误会以 `CoreError` 传回入口。
    ///
    /// 【副作用/数据流】Runner 产生模型和工具意图 → 此循环将事件写入 EventLog → 工具意图回到
    /// ControlPlane 的授权/审批与 CapabilityBroker → 执行结果再转成 RunnerEvent → 最终写 terminal、
    /// Receipt 和 checkpoint。模型推理本身由 Runner/Harness 执行，本函数不直接调用 provider。
    ///
    /// 【为什么只有一个消费循环】v1/v2 和不同入口共享同一套事件持久化、能力授权、取消与终态规则；
    /// 若在入口另建循环，容易绕过 ControlPlane 的权限检查或产生第二种事实顺序。
    pub(crate) async fn drive_run(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        mut pending_events: Vec<RunnerEvent>,
        sequence: &mut u64,
    ) -> Result<CoreResponse, CoreError> {
        // 该循环是 Core 消费 RunnerEvent 的唯一入口；cancel_rx 供 capability broker 等待协作式取消。
        let request_id = context.request_id;
        let cancel_rx = self.watch_cancel(run_id);
        if let Some(response) = self
            .committed_cancel_unknown_response(context, run_id, sandbox)
            .await?
        {
            return Ok(response);
        }
        let mut output = Value::Null;
        let mut failed = None;
        let mut completed = false;
        // 同一轮模型调用会连出多条增量；先攒起来，遇到非增量事件时再合并落账。
        let mut delta_text = String::new();
        while !pending_events.is_empty() {
            // 取消信号只提示需要重新读取当前 turn 的事实；它本身不证明结果未知。
            // 正常流式增量无需逐块扫描整个 run stream，终态写入前仍无条件检查账本。
            let cancellation_signalled = *cancel_rx.borrow();
            if cancellation_signalled {
                if let Some(response) = self
                    .committed_cancel_unknown_response(context, run_id, sandbox)
                    .await?
                {
                    return Ok(response);
                }
            }
            // Vec 前端删除保持 Runner 返回事件的先后顺序；CapabilityRequested 还会向队尾追加新事件。
            let event = pending_events.remove(0);
            if event.run_id() != run_id {
                // 跨 run 事件会污染错误 run 的账本，无法归属时按结果未知停止消费。
                failed = Some("result_unknown:runner_event_run_id_mismatch".to_owned());
                break;
            }
            if !matches!(event, RunnerEvent::Delta { .. }) {
                // 文本批次在非 Delta 事件前落一条 durable run.delta，便于事件回放保留块顺序。
                self.flush_run_delta(request_id, sequence, run_id, &mut delta_text)
                    .await?;
            }
            match event {
                RunnerEvent::ToolCancelled {
                    run_id,
                    request_id: tool_request_id,
                    call_id,
                    result,
                } => {
                    // 取消结果是“未执行”的能力结果事实，单独记账以区分真实工具输出。
                    self.record_event(request_id,sequence,"run.tool_result",json!({
                        "run_id":run_id,"capability_request_id":tool_request_id,"call_id":call_id,
                        "result":result,"cancelled":true,"not_executed":true,
                    })).await?;
                }
                RunnerEvent::ModelTurn {
                    run_id,
                    step,
                    mut metadata,
                } => {
                    // 补全事件身份，确保 metadata 脱离 Runner 内存后仍能关联到 run/session/step。
                    if let Some(fields) = metadata.as_object_mut() {
                        fields.insert("run_id".to_owned(), json!(run_id));
                        fields.insert("step".to_owned(), json!(step));
                        fields.insert("session_id".to_owned(), json!(context.session_id));
                    }
                    if metadata["attempted"] == true {
                        if let Some(cell_id) = context.cell_id {
                            // 优先按 provider 实际 token 计费；usage 缺失时使用预算总额作为保守 reserve。
                            let measured = metadata["usage"]["input_tokens"]
                                .as_u64()
                                .zip(metadata["usage"]["output_tokens"].as_u64())
                                .and_then(|(input, output)| input.checked_add(output));
                            let charge = measured
                                .or_else(|| metadata["budget"]["total"].as_u64())
                                .unwrap_or(u64::MAX);
                            let turn_key = metadata["model_request_id"]
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| format!("{request_id}:{step}"));
                            // 同一 model_request_id 重放时 ledger 可去重，避免重复扣减 cell token budget。
                            match self
                                .cell_registry
                                .account_model_usage(cell_id, &turn_key, charge)
                                .await
                            {
                                Ok(budget) => {
                                    metadata["cell_budget"] = json!(budget);
                                    metadata["usage_accounting"] = json!(if measured.is_some() {
                                        "provider_reported"
                                    } else {
                                        "conservative_request_reserve"
                                    });
                                }
                                Err(error) => {
                                    // 超预算立即请求 runner 取消；错误保留进 metadata 便于 run.model_turn 审计。
                                    failed = Some(format!("run_budget_exceeded:{}", error));
                                    let _ = self
                                        .runner
                                        .send(RunnerCommand::Cancel {
                                            run_id,
                                            reason: "run_budget_exceeded:tokens".to_owned(),
                                        })
                                        .await;
                                    metadata["budget_error"] = json!(error.to_string());
                                }
                            }
                        }
                    }
                    self.record_event(
                        request_id,
                        sequence,
                        "run.model_turn",
                        redact_event_value(&metadata),
                    )
                    .await?;
                }
                RunnerEvent::Started { run_id } => {
                    // Started 进入 EventLog 后成为 run 开始事实，不依赖 UI 本地状态。
                    self.record_event(
                        request_id,
                        sequence,
                        "run.started",
                        json!({ "run_id": run_id }),
                    )
                    .await?;
                }
                RunnerEvent::Delta { text, .. } => {
                    // 展示 delta 暂存到批次，不逐字符写事件账本。
                    delta_text.push_str(&text);
                }
                RunnerEvent::CapabilityRequested { run_id, request } => {
                    // 模型工具意图回到 ControlPlane；Broker 前先经 capability authorization/approval。
                    match self
                        .broker_harness_capability(
                            context, request_id, run_id, sandbox, sequence, request, &cancel_rx,
                        )
                        .await?
                    {
                        // 同步完成的能力结果会生成后续 RunnerEvent，放回队尾继续同一循环。
                        Ok(Some(events)) => pending_events.extend(events),
                        Ok(None) => {
                            // None 表示权限审批挂起：先持久化 run checkpoint，再返回审批挑战给入口。
                            self.checkpoint_run(context, run_id, sandbox, sequence)
                                .await?;
                            let pending = self
                                .pending_invocations
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .values()
                                .find(|pending| pending.run_id == run_id)
                                .cloned();
                            let Some(pending) = pending else {
                                // AwaitingApproval 却找不到对应 invocation 是状态不一致，不能虚构 challenge。
                                failed = Some("pending_invocation_missing".to_owned());
                                break;
                            };
                            // 清除即时 cancel watch；恢复审批时会建立新的执行/取消生命周期。
                            self.clear_cancel(run_id);
                            return Ok(CoreResponse {
                                request_id,
                                status: ExecutionStatus::AwaitingApproval,
                                output: json!({
                                    "approval": pending.challenge,
                                    "capability": pending.request,
                                    "resume_binding": pending.resume_binding,
                                    "run_id": run_id
                                }),
                                error: Some("approval_required".to_owned()),
                            });
                        }
                        Err(reason) => {
                            // Broker 返回拒绝或执行失败时终止本批事件驱动循环。
                            failed = Some(reason);
                            break;
                        }
                    }
                }
                RunnerEvent::ClarificationRequested { run_id, request } => {
                    // waiting_view 校验并生成可投影视图；错误的澄清协议不能进入事件账本。
                    let wait = request.waiting_view().map_err(|error| {
                        PortError::Failed(format!("clarification_invalid:{error}"))
                    })?;
                    self.record_event(
                        request_id,
                        sequence,
                        "run.clarification.requested",
                        json!({
                            "run_id": run_id,
                            "interaction_id": request.interaction_id,
                            "turn_id": request.turn_id,
                            "request": &request,
                            "wait": &wait,
                            "status": kiana_domain::CLARIFICATION_WAITING_STATUS,
                        }),
                    )
                    .await?;
                    // 暂停前保存当前 cursor/状态，后续回答可以在同一 run 恢复。
                    self.checkpoint_run(context, run_id, sandbox, sequence)
                        .await?;
                    return Ok(CoreResponse {
                        request_id,
                        status: ExecutionStatus::Accepted,
                        output: json!({
                            "run_id": run_id,
                            "status": kiana_domain::CLARIFICATION_WAITING_STATUS,
                            "interaction_id": request.interaction_id,
                            "turn_id": request.turn_id,
                            "step_id": request.step_id,
                            "question": request.question,
                            "required": request.required,
                            "expires_at_unix_ms": request.expires_at_unix_ms,
                        }),
                        error: Some(kiana_domain::CLARIFICATION_WAITING_STATUS.to_owned()),
                    });
                }
                RunnerEvent::Completed {
                    run_id,
                    output: mut harness_output,
                } => {
                    // runner 输出补 run_id 后先经过 TurnOutcome 规则，再脱敏并作为 terminal 事实写入。
                    if let Some(object) = harness_output.as_object_mut() {
                        object.insert("run_id".to_owned(), json!(run_id));
                    }
                    let outcome = crate::propose_turn_outcome(
                        kiana_domain::TurnOutcomeInput::completed(run_id, harness_output.clone()),
                    )
                    .map_err(PortError::Failed)?;
                    harness_output = crate::annotate_output(harness_output, &outcome)
                        .map_err(PortError::Failed)?;
                    output = redact_event_value(&harness_output);
                    completed = true;
                    // Lesson proposal 是旁路派生记录；失败不改变已完成 run 的 terminal 结果。
                    if let Some(response) = self
                        .record_drive_terminal(
                            context,
                            run_id,
                            sandbox,
                            sequence,
                            "run.completed",
                            output.clone(),
                        )
                        .await?
                    {
                        return Ok(response);
                    }
                    self.derive_memory_proposal(
                        context,
                        Some(run_id),
                        "run.completed",
                        "lesson",
                        sequence,
                    )
                    .await;
                }
                RunnerEvent::Failed { run_id, error } => {
                    // 错误脱敏后按结构化 CapabilityErrorCode 选择 failed/cancelled/result_unknown 终态。
                    let error = redact_event_text(&error);
                    failed = Some(error.clone());
                    let terminal_kind = match lifecycle_error_code(&error) {
                        CapabilityErrorCode::Cancelled => "run.cancelled",
                        CapabilityErrorCode::ResultUnknown
                        | CapabilityErrorCode::CompensationRequired => "run.result_unknown",
                        _ => "run.failed",
                    };
                    if let Some(response) = self
                        .record_drive_terminal(
                            context,
                            run_id,
                            sandbox,
                            sequence,
                            terminal_kind,
                            json!({ "run_id": run_id, "error": error }),
                        )
                        .await?
                    {
                        return Ok(response);
                    }
                    self.derive_memory_proposal(
                        context,
                        Some(run_id),
                        terminal_kind,
                        "lesson",
                        sequence,
                    )
                    .await;
                }
                RunnerEvent::Compacted {
                    run_id,
                    tokens_before,
                    tokens_after,
                    summary_present,
                } => {
                    // Compacted 只记录压缩统计，不把 summary 当作 EventLog 的事实输入覆盖历史。
                    self.record_event(
                        request_id,
                        sequence,
                        "run.compacted",
                        json!({
                            "schema": COMPACT_SCHEMA,
                            "run_id": run_id,
                            "tokens_before": tokens_before,
                            "tokens_after": tokens_after,
                            "summary_present": summary_present,
                        }),
                    )
                    .await?;
                }
            }
            if completed || failed.is_some() {
                // terminal event 已决定本轮结果，忽略同批次后续事件以免写出矛盾终态。
                break;
            }
        }
        // 末尾可能还有没被非增量事件触发的增量（例如事件列表直接结束）。
        // 先把已观察到的文本落账；只有确认确定终态后才清除取消 watch。
        self.flush_run_delta(request_id, sequence, run_id, &mut delta_text)
            .await?;

        if let Some(error) = failed {
            let error_code = lifecycle_error_code(&error);
            let result_unknown = matches!(
                error_code,
                CapabilityErrorCode::ResultUnknown | CapabilityErrorCode::CompensationRequired
            );
            // Error code 决定 terminal kind 与直接响应状态；未知副作用不能降格成普通 failed。
            if let Some(response) = self
                .record_drive_terminal(
                    context,
                    run_id,
                    sandbox,
                    sequence,
                    if result_unknown {
                        "run.result_unknown"
                    } else if error_code == CapabilityErrorCode::Cancelled {
                        "run.cancelled"
                    } else {
                        "run.failed"
                    },
                    json!({"run_id":run_id,"error":error}),
                )
                .await?
            {
                return Ok(response);
            }
            if !result_unknown {
                self.clear_cancel(run_id);
            }
            if error == "run_not_found" {
                self.forget_session(context.session_id.as_str(), run_id);
            }
            if result_unknown {
                // 再写一次是兼容既有恢复约定；失败只忽略追加错误，原 ResultUnknown 仍作为响应返回。
                let _ = self
                    .record_terminal_event(
                        request_id,
                        sequence,
                        run_id,
                        "run.result_unknown",
                        json!({ "run_id": run_id, "error": redact_event_text(&error) }),
                    )
                    .await;
            }
            return Ok(CoreResponse {
                request_id,
                status: if error == "run_not_found" {
                    ExecutionStatus::Blocked
                } else if result_unknown {
                    ExecutionStatus::ResultUnknown
                } else if error_code == CapabilityErrorCode::Cancelled {
                    ExecutionStatus::Cancelled
                } else {
                    ExecutionStatus::Failed
                },
                output: run_identity(context, run_id, sandbox),
                error: Some(error),
            });
        }
        if !completed {
            // 事件批次耗尽却没有 Completed/Failed 表示没有结果证据，不能合成 Completed。
            let reason = "run_result_missing";
            if let Some(response) = self
                .record_drive_terminal(
                    context,
                    run_id,
                    sandbox,
                    sequence,
                    "run.result_unknown",
                    json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                )
                .await?
            {
                return Ok(response);
            }
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: run_identity(context, run_id, sandbox),
                error: Some(reason.to_owned()),
            });
        }

        // 成功终态后保留 session->run 索引，构造 receipt 投影并写 checkpoint 供下一次 continue/recovery 使用。
        self.clear_cancel(run_id);
        self.remember_session(&context, run_id);
        let receipt = self
            .run_receipt_from_store(context, run_id, sandbox, output)
            .await?;
        self.record_event(request_id, sequence, "run.receipt", receipt.clone())
            .await?;
        self.checkpoint_run(context, run_id, sandbox, sequence)
            .await?;
        Ok(CoreResponse::completed(request_id, receipt))
    }
}

pub(crate) fn authorized_harness_sandbox(
    context: &RequestContext,
    requested: Option<&str>,
) -> Result<&'static str, &'static str> {
    // 这是创建/继续 Harness run 前的权限边界；只返回静态白名单值，入口提供的任意字符串不能直接成为 sandbox。
    // 未指定或空白配置采用最小权限 sandbox；其他值必须由下方白名单明确支持。
    let requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("read-only");
    match requested {
        "read-only" => Ok("read-only"),
        "workspace-write" => {
            // 写权限同时要求项目已被信任、permission profile 非 Safe、角色目录允许写入。
            if context.project_trusted
                && !matches!(context.permission_profile, PermissionProfile::Safe)
            {
                let role = RoleSpec::lookup(&context.role_id).ok_or("role_unknown")?;
                if role.workspace_write_allowed() {
                    Ok("workspace-write")
                } else {
                    Err("role_sandbox_read_only")
                }
            } else {
                Err("workspace_write_requires_trusted_non_safe_profile")
            }
        }
        _ => Err("sandbox_unsupported"),
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use kiana_capability_broker::CapabilityBroker;
    use kiana_domain::{ApprovalChallenge, PendingApproval};
    use kiana_eventlog::MemoryEventLog;
    use kiana_gates::DefaultGateEngine;
    use kiana_policy::DefaultPolicyEngine;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct NoApprovals;

    #[async_trait::async_trait]
    impl ApprovalStorePort for NoApprovals {
        async fn stage(
            &self,
            _context: &RequestContext,
            _request: CapabilityRequest,
            _reason: &str,
        ) -> Result<ApprovalChallenge, PortError> {
            Err(PortError::Unavailable("fixture_no_approvals".to_owned()))
        }

        async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
            Err(PortError::Unavailable("fixture_no_approvals".to_owned()))
        }

        async fn consume(
            &self,
            _context: &RequestContext,
            _approval_id: ApprovalId,
        ) -> Result<PendingApproval, PortError> {
            Err(PortError::Unavailable("fixture_no_approvals".to_owned()))
        }
    }

    #[derive(Default)]
    struct UnsupportedAssignmentRunner {
        sends: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl RunnerPort for UnsupportedAssignmentRunner {
        async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            panic!("default binding denial must stop before dispatch");
        }
    }

    #[derive(Default)]
    struct RecoveringAssignmentRunner {
        accepts_bindings: AtomicBool,
        sends: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl RunnerPort for RecoveringAssignmentRunner {
        fn bind_model_assignment(
            &self,
            run_id: RunId,
            assignment: kiana_domain::ModelAssignment,
        ) -> Result<(), PortError> {
            assignment
                .validate()
                .map_err(|error| PortError::Failed(error.to_string()))?;
            if assignment.run_id != run_id {
                return Err(PortError::Failed(
                    "model_assignment_run_mismatch".to_owned(),
                ));
            }
            if !self.accepts_bindings.load(Ordering::SeqCst) {
                return Err(PortError::Unavailable(
                    "runner_model_assignment_unsupported".to_owned(),
                ));
            }
            Ok(())
        }

        async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            let RunnerCommand::Start { run_id, .. } = command else {
                panic!("fixture expects a fresh Start");
            };
            Ok(vec![
                RunnerEvent::Started { run_id },
                RunnerEvent::Completed {
                    run_id,
                    output: json!({"text":"recovered"}),
                },
            ])
        }
    }

    fn plane(runner: Arc<dyn RunnerPort>) -> (ControlPlane, Arc<MemoryEventLog>) {
        let events = Arc::new(MemoryEventLog::new());
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events.clone(),
            Arc::new(CapabilityBroker::new()),
            Arc::new(NoApprovals),
            runner,
        );
        (core, events)
    }

    fn context() -> RequestContext {
        let mut context = RequestContext::local("startup-binding-fixture", "/repo");
        context.project_trusted = true;
        context
    }

    async fn assert_closed_binding_failure(
        core: &ControlPlane,
        events: &MemoryEventLog,
        context: RequestContext,
    ) -> RunId {
        let request_id = context.request_id;
        let response = core
            .start_run(context.clone(), "inspect".to_owned(), None)
            .await
            .expect("binding failure must have a terminal response");
        assert_eq!(response.status, ExecutionStatus::Failed, "{response:?}");
        assert_eq!(
            response.error.as_deref(),
            Some("port_unavailable:runner_model_assignment_unsupported")
        );
        let run_id = RunId::parse_str(response.output["run_id"].as_str().unwrap()).unwrap();
        let request_events = events.read_request(&request_id).await.unwrap();
        assert_eq!(
            request_events
                .iter()
                .filter(|event| event.kind == "run.failed")
                .count(),
            1,
            "{request_events:?}"
        );
        assert!(
            !request_events.iter().any(|event| matches!(
                event.kind.as_str(),
                "run.started" | "run.completed" | "capability.requested"
            )),
            "{request_events:?}"
        );
        assert_eq!(
            core.run_state(run_id).await.unwrap().outcome,
            Some(RunOutcome::Failed)
        );
        assert!(!core.session_known(context.session_id.as_str()));
        assert!(!core.cancellations.lock().unwrap().contains_key(&run_id));
        assert!(!core
            .active_terminal_scopes
            .lock()
            .unwrap()
            .contains_key(&run_id));
        run_id
    }

    #[tokio::test]
    async fn default_model_assignment_binding_failure_is_terminal_before_dispatch() {
        let runner = Arc::new(UnsupportedAssignmentRunner::default());
        let (core, events) = plane(runner.clone());
        let first = assert_closed_binding_failure(&core, &events, context()).await;
        let second = assert_closed_binding_failure(&core, &events, context()).await;
        assert_ne!(first, second);
        assert_eq!(runner.sends.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn model_assignment_binding_failure_cleans_indices_and_allows_new_request() {
        let runner = Arc::new(RecoveringAssignmentRunner::default());
        let (core, events) = plane(runner.clone());
        let failed = assert_closed_binding_failure(&core, &events, context()).await;
        assert_eq!(runner.sends.load(Ordering::SeqCst), 0);

        runner.accepts_bindings.store(true, Ordering::SeqCst);
        let response = core
            .start_run(context(), "retry".to_owned(), None)
            .await
            .unwrap();
        assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
        let retried = RunId::parse_str(response.output["run_id"].as_str().unwrap()).unwrap();
        assert_ne!(failed, retried);
        assert_eq!(runner.sends.load(Ordering::SeqCst), 1);
        assert_eq!(
            core.run_state(failed).await.unwrap().outcome,
            Some(RunOutcome::Failed)
        );
        assert_eq!(
            core.run_state(retried).await.unwrap().outcome,
            Some(RunOutcome::Completed)
        );
    }
}
