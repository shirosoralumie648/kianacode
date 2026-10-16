//! 本文件是 Core 中持久化 workflow 命令的适配层：把入口收到的命令交给纯 planner，
//! 将规划结果写入 EventStore，再由同一个 `ControlPlane` 调用现有 Company/能力执行路径。
//!
//! # 它在系统中的位置
//!
//! ```text
//! CLI / Workbench / Web 等入口
//!     -> kiana-entrypoints 生成命令意图
//!     -> Core commands.rs 按 AUTOMATION_COMMAND 分流
//!     -> 本文件：校验调用上下文、读取/重建状态、规划、提交事实
//!     -> kiana-workflow::plan_command：纯状态转换，返回下一状态和待办 effect
//!     -> 本文件先提交 workflow.command_applied
//!     -> AgentTask 复用 Company StartRun；Capability 复用 authorize_and_execute
//!     -> 从运行事件/Company 状态重建结果，再提交内部 RecordObservation
//! ```
//!
//! `kiana-domain` 定义 workflow 命令、状态和事件 DTO；`kiana-workflow` 负责定义校验与
//! 确定性状态规划；本文件负责把这些领域结果接到 Core 的授权、EventStore、Company 和
//! capability handler。普通命令入口由 `commands.rs` 路由；`company.rs` 的能力守卫也会
//! 调用本文件，确认带有 workflow execution ID 的能力请求仍属于可继续的节点。
//!
//! # 确定性与副作用的边界
//!
//! “确定性”指相同的状态、命令、调用方提供的 authority 时间快照和 proof 输入会得到相同
//! 的规划状态/effect；它不表示外部模型、Capability 或 Company run 一定产生相同结果。
//! 规划器本身不做 I/O，所有 effect 必须由这里在 EventStore 记录准入事实后交给现有执行
//! 路径。重放只折叠已提交事件，不再次执行 effect；无法确认的运行结果保留为
//! `ResultUnknown`，等待 `Reconcile` 根据运行事实补记观测。
//!
//! ⚠ 不要把 `plan_command()` 返回的 effect 当作授权或执行许可。持久化 workflow 转换与
//! 后续 Company/Capability 副作用是两个阶段；前者提交后即使后者失败，也不能假装事件已回滚。
use super::*;
use kiana_domain::*;
use kiana_workflow::{definition_key, plan_command, plan_command_intent};

/// 所有 workflow 状态事件共用的 EventStore 聚合类型；具体聚合 ID 再按 actor 和项目根隔离。
const AGGREGATE: &str = "workflow";

/// 将 planner 的稳定拒绝原因包装为 Core 统一的结构化冲突错误。
///
/// 调用者通常会把该错误转换为 `Blocked` 响应或向上传播；本函数不写日志、不追加事件，
/// 也不判断当前调用处是否应该重试。
fn automation_error(reason: &str) -> CoreError {
    CoreError::Port(PortError::Conflict(reason.into()))
}

/// 取出 workflow 数据的规范项目根，用于在 EventStore key 中固定项目隔离边界。
///
/// 上游数据来自已建立的 `RequestContext`。路径先经过 Core 的统一规范化函数，再转成
/// 可放入聚合 ID 的字符串；这一步只生成读写 key，不证明目录当前存在或可信。
fn root(context: &RequestContext) -> String {
    ControlPlane::canonical_project_root(&context.project_root)
        .to_string_lossy()
        .into_owned()
}

/// 生成 workflow 聚合 ID，使不同 actor 或不同规范项目根的 workflow 事实分流到不同事件流。
///
/// 空 actor 会变成空字符串，但公开命令处理与纯 planner 都会单独拒绝缺少 actor 的请求；
/// 因此不能把这个 key helper 当成身份校验。
fn aggregate_id(context: &RequestContext) -> String {
    format!(
        "{}\n{}",
        context.actor_id.as_deref().unwrap_or_default(),
        root(context)
    )
}

/// 取得毫秒级 Unix 时间，供 planner 写入开始/过期时间等快照字段。
///
/// 结果上限钳制在 `u64`，避免 `u128` 毫秒值转换时溢出。若系统时间早于 Unix epoch 或
/// 读取系统时钟失败，当前实现退回 `0`；本函数不会单独记录时钟故障。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// 为一次 workflow execution 收集可引用的 EventStore 事件 ID。
///
/// 【调用者】`workflow_runtime_proof()` 生成未知结果的回执，以及 `workflow_proof()` 对
/// Company run 无法确认时补证据引用。
///
/// 【数据流】先按事件自身的 `request_id` 精确筛选；若结果为空且调用方允许 fallback，
/// 再从事件 JSON 的 `/authority/execution_id` 字段查找。返回值是 `event:<EventId>` 字符串，
/// 不复制事件正文，也不据此认定执行成功。
///
/// `allow_reservation_fallback` 之所以单独控制，是因为一些准入/预留事实可能使用外层请求
/// ID 作为事件关联 ID，但把实际 execution ID 放在 authority payload 里。关闭 fallback
/// 时，调用方只接受直接关联的事件，避免把较宽松的关联规则用于普通完成回执。
fn execution_event_refs(
    events: &[RuntimeEvent],
    execution_id: RequestId,
    allow_reservation_fallback: bool,
) -> Vec<String> {
    let execution_text = execution_id.to_string();
    let mut refs = events
        .iter()
        .filter(|event| event.request_id == execution_id)
        .map(|event| format!("event:{}", event.event_id))
        .collect::<Vec<_>>();
    if refs.is_empty() && allow_reservation_fallback {
        refs = events
            .iter()
            .filter(|event| {
                event
                    .data
                    .pointer("/authority/execution_id")
                    .and_then(Value::as_str)
                    == Some(execution_text.as_str())
            })
            .map(|event| format!("event:{}", event.event_id))
            .collect();
    }
    refs
}

/// 把 Core 执行响应和已有事件引用整理成 planner 能消费的运行 proof。
///
/// `ResultUnknown` 表示“副作用是否发生无法确认”，不是失败或取消；若调用方没找到直接
/// 事件引用，这里尝试收集 reservation 事实，帮助之后人工/自动 reconciliation 定位证据。
/// 已终结的响应会生成 `RuntimeReceiptRef`；未知终态还会生成可重复计算 ID 的
/// `WorkflowIncident`。非终态（例如等待审批）不会伪造终态回执。
///
/// 回执或 incident 结构不满足领域不变量时返回冲突错误，交由当前命令路径处理；本函数
/// 不落库，真正持久化发生在随后的内部 `RecordObservation` workflow 事件中。
fn workflow_runtime_proof(
    instance_id: &str,
    node_id: &str,
    response: CoreResponse,
    evidence_refs: Vec<String>,
    events: &[RuntimeEvent],
) -> Result<AutomationProof, CoreError> {
    let mut evidence_refs = evidence_refs;
    if evidence_refs.is_empty() && response.status == ExecutionStatus::ResultUnknown {
        evidence_refs = execution_event_refs(events, response.request_id, true);
    }
    let mut proof = AutomationProof {
        response: Some(response.clone()),
        evidence_refs: evidence_refs.clone(),
        event_kind: None,
        runtime_receipt: None,
        incident: None,
    };
    if response.status.is_terminal() {
        let receipt =
            RuntimeReceiptRef::new(response.request_id, response.status, evidence_refs.clone())
                .map_err(|_| automation_error("workflow_runtime_receipt_invalid"))?;
        if response.status == ExecutionStatus::ResultUnknown {
            let identity_digest = json_digest(&json!({
                "instance_id": instance_id,
                "node_id": node_id,
                "request_id": response.request_id,
            }));
            let incident = WorkflowIncident::new(
                format!("workflow-unknown:{identity_digest}"),
                instance_id,
                node_id,
                response.request_id,
                "workflow_runtime_result_unknown",
                evidence_refs,
            )
            .map_err(|_| automation_error("workflow_incident_invalid"))?;
            proof.incident = Some(incident);
        }
        proof.runtime_receipt = Some(receipt);
    }
    Ok(proof)
}

/// 为一次 workflow planner 调用冻结授权上下文和运行标识。
///
/// 【输入/输出】保留调用者完整 `RequestContext`，另外生成当前时间、一个 execution ID 和
/// 一个带 `workflow-` 前缀的新 Session ID。planner 将它们写入新节点或 authority 事实，
/// 后续 dispatch 用返回的 execution/session 标识关联运行。
///
/// 这里的 ID 是关联标识，不是凭证；真正的能力授权仍由 ControlPlane 的既有守卫和 Broker
/// 流程完成。
fn authority(context: &RequestContext) -> AutomationAuthority {
    AutomationAuthority {
        context: context.clone(),
        now_ms: now_ms(),
        execution_id: RequestId::new(),
        session_id: SessionId::new(format!("workflow-{}", RequestId::new())),
    }
}

impl ControlPlane {
    /// 当 Company/能力入口收到带 workflow execution ID 的请求时，检查对应节点是否仍可继续。
    ///
    /// 【调用者】`company.rs::guard_company_capability()` 在执行 Company 能力守卫时调用它。
    ///
    /// 【输入/输出】用 `request.request_id` 查当前 actor+项目根的 workflow 状态；若它匹配某
    /// 个节点，则只有以下限制都未触发时才返回 `Ok(())`：祖先实例未暂停/取消/终结、当前实例
    /// 未终结且未暂停/取消、节点未终结、当前会话与节点记录一致、当前角色与实例角色一致。
    /// `Ok(())` 只表示 workflow 归属守卫没有拒绝，后续的 Company、Policy、Gate、审批和 Broker
    /// 检查仍然照常运行。
    ///
    /// 【失败与边界】匹配到节点但任一限制不成立时返回 `workflow_execution_not_authorized`；
    /// 读取 workflow 事件流失败则向调用者传播存储错误。若请求 ID 没匹配任何节点，本方法
    /// 返回成功，让普通非-workflow 能力继续由其他守卫检查；所以这里不是通用权限校验器。
    ///
    /// ⚠ `workflow_ancestors_active()` 检查的是父 workflow 链上的状态，不是“每个上游节点
    /// 都成功”。节点是否已终结由下面针对当前节点的状态判断负责。
    pub(crate) async fn guard_workflow_capability(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<(), CoreError> {
        let (state, _) = self.load_workflows(context).await?;
        for instance in state.instances.values() {
            if let Some(node) = instance
                .nodes
                .values()
                .find(|node| node.execution_id == request.request_id)
            {
                if !kiana_workflow::workflow_ancestors_active(&state, &instance.instance_id)
                    || instance.status.terminal()
                    || matches!(
                        instance.status,
                        WorkflowInstanceStatus::Paused | WorkflowInstanceStatus::CancelRequested
                    )
                    || node.status.terminal()
                    || node.session_id != context.session_id
                    || instance.role_id != context.role_id
                {
                    return Err(automation_error("workflow_execution_not_authorized"));
                }
            }
        }
        Ok(())
    }

    /// 返回当前 actor 和项目根下可由事件重建的 workflow 状态快照。
    ///
    /// 【调用者】`commands.rs` 将 `AUTOMATION_SNAPSHOT` 路由到这里。
    /// 【输入】调用上下文中的 actor 与规范项目根决定读取哪个聚合；没有 actor ID 时返回
    /// `Blocked(workflow_actor_required)`，不访问状态流。
    /// 【输出】成功时返回 `kiana.workflow-state.v1` 和折叠出的 `AutomationState`。
    /// 【副作用/错误】这里只读 EventStore，不推进 workflow、不运行节点；存储损坏或事件
    /// 重放冲突会作为 Core 错误返回，而不是以部分状态响应。
    pub(crate) async fn workflow_snapshot(
        &self,
        context: RequestContext,
    ) -> Result<CoreResponse, CoreError> {
        if context.actor_id.as_deref().is_none_or(|id| id.is_empty()) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "workflow_actor_required",
            ));
        }
        let (state, _) = self.load_workflows(&context).await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":"kiana.workflow-state.v1","state":state}),
        ))
    }

    /// 处理外部提交的 workflow 命令，并在提交状态事实之后执行 planner 产生的 effect。
    ///
    /// 【调用者】Core 的 `commands.rs` 识别 `AUTOMATION_COMMAND` 后调用；更上游的入口通过
    /// 统一 `DaemonHost/ControlPlane` 命令面到达这里。
    ///
    /// 【主要数据流】JSON `arguments` → schema/信任/角色/大小检查 → 当前事件流重建状态 →
    /// `workflow_proof()` 验证证据引用 → `kiana_workflow::plan_command_intent()` 计算新状态与
    /// effect → `commit_workflow()` 先记录命令事实 → 需要时复用 Company 或 capability 执行
    /// 路径 → `record_workflow_observation()` 记录可观察结果并返回新状态。
    ///
    /// 【失败情况】可识别的拒绝会由 `reject_workflow()` 记录 `workflow.rejected` 并返回
    /// `Blocked`；EventStore、序列化、执行后观测等其他错误按下面分支传播或转换为
    /// `ResultUnknown`。未知结果不会被伪装成成功/失败；调用方需通过快照和 `Reconcile` 继续
    /// 核对。
    ///
    /// ⚠ planner 生成的 effect 只在本次命令首次通过 revision/idempotency 检查并提交后执行。
    /// 幂等重放直接返回当前状态，不再 dispatch；这是避免把一次已提交的执行意图重复发送给
    /// 上游的边界。提交成功与外部副作用不是同一个事务，副作用结果必须另行观测。
    pub(crate) async fn handle_workflow_command(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        // workflow 能启动 AgentTask 或 Capability，因此项目必须已通过信任检查，并且当前
        // Profile 不能是 Safe/Ask。该门只筛掉不允许这种工作流写操作的模式；后续 command、
        // Company、权限和 capability 各自仍有更具体的校验。
        if !context.project_trusted
            || matches!(
                context.permission_profile,
                PermissionProfile::Safe | PermissionProfile::Ask
            )
        {
            return self
                .reject_workflow(&context, "workflow_project_trust_and_authority_required")
                .await;
        }
        // 角色 ID 必须能在内建 RoleSpec 中解析，而且角色声明的 department 必须与当前上下文
        // 一致。只比较调用者传入的字符串，会允许上下文携带互相矛盾的 role/department 组合。
        let valid_role = RoleSpec::lookup(&context.role_id)
            .is_some_and(|r| r.department_id == context.department_id);
        if !valid_role {
            return self
                .reject_workflow(&context, "workflow_role_unknown")
                .await;
        }
        // 限制的是 arguments 序列化后的 UTF-8 字节数（512 KiB），不是节点数或执行时间。
        // 先限大小可避免把超大 JSON 交给 DTO 解析、事件摘要和 EventStore 序列化链路。
        if serde_json::to_vec(&arguments).map_or(true, |v| v.len() > 512 * 1024) {
            return self
                .reject_workflow(&context, "workflow_command_too_large")
                .await;
        }
        let request: AutomationCommandRequest = match serde_json::from_value(arguments) {
            Ok(r) => r,
            Err(_) => {
                return self
                    .reject_workflow(&context, "workflow_command_invalid")
                    .await
            }
        };
        // RecordObservation 是运行后的内部事实，只能由本 ControlPlane 在 dispatch/reconcile
        // 后构造。允许外部直接提交它会让调用方自报节点执行结果，绕过 proof 生成边界。
        // `str::len()` 计算 UTF-8 字节数；幂等键上限是 256 字节，用来限制 EventStore 索引和
        // 后续生成的 key 长度，不是 256 个字符。
        if request.schema != AUTOMATION_SCHEMA
            || request.idempotency_key.trim().is_empty()
            || request.idempotency_key.len() > 256
            || matches!(request.command, AutomationCommand::RecordObservation { .. })
        {
            return self
                .reject_workflow(&context, "workflow_schema_or_internal_command_denied")
                .await;
        }
        let (state, history) = self.load_workflows(&context).await?;
        // 幂等键重用时必须是同一命令并来自同一 actor/role/session。匹配则返回“现在的状态”
        // 快照而不是重复运行外部 effect；不匹配则当成键冲突记录拒绝事实。
        if let Some(previous) = history
            .iter()
            .find(|e| e.request.idempotency_key == request.idempotency_key)
        {
            if previous.request != request
                || previous.authority.context.actor_id != context.actor_id
                || previous.authority.context.role_id != context.role_id
                || previous.authority.context.session_id != context.session_id
            {
                return self
                    .reject_workflow(&context, "workflow_idempotency_conflict")
                    .await;
            }
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"schema":"kiana.workflow-state.v1","replayed":true,"state":state}),
            ));
        }
        if state.revision != request.expected_revision {
            return self
                .reject_workflow(&context, "workflow_revision_conflict")
                .await;
        }
        let proof = match self
            .workflow_proof(&context, &state, &request.command)
            .await
        {
            Ok(p) => p,
            Err(CoreError::Port(PortError::Conflict(reason))) => {
                return self.reject_workflow(&context, &reason).await
            }
            Err(e) => return Err(e),
        };
        let a = authority(&context);
        // intent 在本路径中仅作为 planner 计算产物；这里持久化的是 command + authority +
        // proof，由 plan_command 重放得到状态。真正的可执行 effect 是单独返回的第三项。
        let (next, _planner_intent, effect) =
            match plan_command_intent(&state, &request.command, &a, &proof) {
                Ok(n) => n,
                Err(reason) => return self.reject_workflow(&context, reason).await,
            };
        let event = AutomationEvent {
            schema: AUTOMATION_EVENT_SCHEMA.into(),
            request: request.clone(),
            authority: a,
            proof,
            envelope: None,
        };
        let committed = match self.commit_workflow(&context, event).await {
            Ok(e) => e,
            Err(CoreError::Port(PortError::Conflict(reason))) => {
                return self.reject_workflow(&context, &reason).await
            }
            Err(e) => return Err(e),
        };
        if let Some(WorkflowEffect::Dispatch {
            instance_id,
            node_id,
            kind,
            execution_id,
            session_id,
        }) = effect
        {
            // 已提交事件先于 effect，构成 durable reservation。若此后进程退出，重放会保留
            // “已预留”状态但不会重新 dispatch；结果应由后续 Reconcile 从 Company/能力事件核对，
            // 而不是为了补跑而可能重复触发外部副作用。
            let mut execution_context = context.clone();
            execution_context.request_id = execution_id;
            execution_context.session_id = session_id;
            // 子执行不继承父请求临时携带的 Cell、WorkPacket 或 path allow 列表。AgentTask
            // 通过 Company StartRun 重新建立运行边界；Capability 则仍走授权与 Broker 路径。
            execution_context.cell_id = None;
            execution_context.work_packet_id = None;
            execution_context.path_allow.clear();
            let runtime = match kind {
                WorkflowNodeKind::AgentTask {
                    project_id,
                    packet_id,
                    sandbox,
                } => {
                    let (company, _) = self.load_company(&execution_context).await?;
                    self.handle_company_command(
                        execution_context.clone(),
                        serde_json::to_value(CompanyCommandRequest {
                            schema: COMPANY_COMMAND_SCHEMA.into(),
                            expected_revision: company.revision,
                            idempotency_key: format!("workflow:{execution_id}"),
                            command: CompanyCommand::StartRun {
                                project_id,
                                packet_id,
                                sandbox,
                            },
                        })
                        .map_err(|_| automation_error("workflow_dispatch_encode_failed"))?,
                    )
                    .await
                }
                WorkflowNodeKind::Capability { mut request } => {
                    request.request_id = execution_id;
                    self.authorize_and_execute(&execution_context, request)
                        .await
                }
                _ => return Err(automation_error("workflow_effect_invalid")),
            };
            // 外层执行 API 返回 Err 时，不能据此断言副作用未发生：错误可能出现在副作用之后。
            // 因此保守映射成 ResultUnknown，让后续 observation/reconcile 保持事实不确定。
            let response = runtime.unwrap_or_else(|error| CoreResponse {
                request_id: execution_id,
                status: ExecutionStatus::ResultUnknown,
                output: Value::Null,
                error: Some(error.to_string()),
            });
            let events = self.events.read_all().await?;
            let proof = workflow_runtime_proof(
                &instance_id,
                &node_id,
                response,
                execution_event_refs(&events, execution_id, false),
                &events,
            )?;
            match self
                .record_workflow_observation(&context, &instance_id, &node_id, proof)
                .await
            {
                Ok(state) => {
                    return Ok(CoreResponse::completed(
                        context.request_id,
                        json!({"schema":"kiana.workflow-state.v1","event_id":committed.event_id,"state":state}),
                    ))
                }
                Err(error) => {
                    // effect 已经尝试过，但其 observation 未能持久化；响应显式要求 reconciliation，
                    // 不在这里重新 dispatch。调用方应以返回的 instance/node 定位未决节点。
                    return Ok(CoreResponse {
                        request_id: context.request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: json!({"instance_id":instance_id,"node_id":node_id}),
                        error: Some(format!("workflow_reconciliation_required:{error}")),
                    });
                }
            }
        }
        if let Some(WorkflowEffect::Cancel { instance_id }) = effect {
            // 取消状态已随上面的 workflow.command_applied 提交；此处才向 Company/审批边界发出
            // 取消动作。若动作失败，错误会返回给调用方，但已提交的取消请求仍需后续核对。
            self.cancel_workflow_effects(&context, &next, &instance_id)
                .await?;
        }
        if let AutomationCommand::Tick { trigger_id } | AutomationCommand::Fire { trigger_id, .. } =
            &request.command
        {
            // Tick/Fire 可能因触发器策略把关联实例标成 CancelRequested；规划器给出状态后，
            // Core 逐个对这些实例发送运行取消请求。真正终态仍需运行事实和 Reconcile 确认。
            for instance in next.instances.values().filter(|i| {
                i.trigger_id.as_deref() == Some(trigger_id)
                    && i.status == WorkflowInstanceStatus::CancelRequested
            }) {
                self.cancel_workflow_effects(&context, &next, &instance.instance_id)
                    .await?;
            }
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":"kiana.workflow-state.v1","event_id":committed.event_id,"replayed":false,"state":next}),
        ))
    }
    /// 记录一次 workflow 命令拒绝，并把同一原因返回为 `Blocked` 响应。
    ///
    /// 【调用者】外部命令中的信任、角色、schema、幂等键、revision、证据或 planner 校验失败
    /// 时由 `handle_workflow_command()` 调用。
    /// 【输入】`reason` 是稳定错误码；事件同时记录 actor 和规范项目根，便于审计拒绝发生在
    /// 哪个调用范围。
    /// 【副作用】追加 `workflow.rejected` 事件，但不修改 workflow 聚合状态，也不尝试 effect。
    /// 若追加事件本身失败，存储错误向上传播，调用方不会收到下方构造的 Blocked 响应。
    ///
    /// 内部 `RecordObservation` 校验失败也可以传播回外层；外层只会将观测流程标成
    /// reconciliation required，不把它伪装成用户命令成功。
    async fn reject_workflow(
        &self,
        context: &RequestContext,
        reason: &str,
    ) -> Result<CoreResponse, CoreError> {
        self.append_event(
            context.request_id,
            1,
            "workflow.rejected",
            json!({"reason":reason,"actor_id":context.actor_id,"project_root":root(context)}),
        )
        .await?;
        Ok(CoreResponse::blocked(context.request_id, reason))
    }
    /// 从一个 actor+项目根隔离的 workflow 事件流重建当前状态与幂等历史。
    ///
    /// 【调用者】snapshot、命令处理、能力守卫、观测重试和 reconciliation 均会读取这里。
    /// 【输入/输出】通过 `aggregate_id(context)` 读取 `workflow` 聚合，将事件按 stream version
    /// 排序后逐条校验并折叠，返回 `(AutomationState, Vec<AutomationEvent>)`。第二项供命令处理
    /// 查重；它不是另一个状态真源。
    ///
    /// 【重放校验】每条事实必须匹配当前 schema、上一 revision、连续 stream version、事件种类、
    /// actor、规范项目根，并且 idempotency key 不得重复。存在 envelope 时还校验聚合、版本、
    /// request/idempotency 身份和 command/payload 摘要；旧事件没有 envelope 时，本实现不会对
    /// envelope 执行这一步校验。
    ///
    /// 【为什么重放时丢弃 effect】planner 每步返回“下一状态 + 可选 effect”；历史重建只取
    /// `.0` 状态，不执行 dispatch/cancel。否则每次读取 snapshot、查权限或处理新命令都可能
    /// 重复运行历史副作用。
    ///
    /// 任一反序列化、连续性、摘要或 planner 不变量失败都会停止整个重建并返回冲突错误；本方法
    /// 不尝试跳过坏事件继续返回部分状态。
    pub(crate) async fn load_workflows(
        &self,
        context: &RequestContext,
    ) -> Result<(AutomationState, Vec<AutomationEvent>), CoreError> {
        let mut events = self
            .events
            .read_stream(AGGREGATE, &aggregate_id(context))
            .await?;
        events.sort_by_key(|e| e.stream_version);
        let mut state = AutomationState::default();
        let mut history = Vec::new();
        let mut keys = HashSet::new();
        for event in events {
            let e: AutomationEvent = serde_json::from_value(event.data)
                .map_err(|_| automation_error("workflow_event_invalid"))?;
            if e.schema != AUTOMATION_EVENT_SCHEMA
                || e.request.schema != AUTOMATION_SCHEMA
                || e.request.expected_revision != state.revision
                || event.stream_version != Some(state.revision + 1)
                || event.kind != "workflow.command_applied"
                || e.authority.context.actor_id != context.actor_id
                || root(&e.authority.context) != root(context)
                || !keys.insert(e.request.idempotency_key.clone())
            {
                return Err(automation_error("workflow_replay_conflict"));
            }
            // Envelope 是当前事实的显式身份/摘要绑定。`Option` 允许历史载荷没有 envelope；
            // 但只要存在，就必须和事件所在流及载荷完全一致，不能把一个事件的 envelope 搬到
            // 另一条流或另一个 revision 上复用。
            if let Some(envelope) = &e.envelope {
                let command_digest = json_digest(
                    &serde_json::to_value(&e.request)
                        .map_err(|_| automation_error("workflow_event_invalid"))?,
                );
                let payload_digest = json_digest(&json!({
                    "authority": &e.authority,
                    "proof": &e.proof,
                }));
                envelope
                    .validate_against(
                        &aggregate_id(context),
                        event.stream_version.unwrap_or_default(),
                        e.authority.context.request_id,
                        &e.request.idempotency_key,
                        &command_digest,
                        &payload_digest,
                    )
                    .map_err(|_| automation_error("workflow_event_envelope_mismatch"))?;
            }
            // 这里只重算状态。effect 是当初提交后由命令路径消费的一次性工作，不属于状态
            // 折叠结果；重放绝不能再次触发 Company Run、能力调用或取消动作。
            state = plan_command(&state, &e.request.command, &e.authority, &e.proof)
                .map_err(automation_error)?
                .0;
            history.push(e);
        }
        Ok((state, history))
    }
    /// 将一条 planner 已接受的 workflow 命令封装成带摘要的 RuntimeEvent 并受保护地提交。
    ///
    /// 【调用者】`handle_workflow_command()` 提交外部命令；`record_workflow_observation()` 提交
    /// ControlPlane 内部生成的观测命令。
    /// 【输入】`event` 带 command、authority 快照和 proof；expected revision 来自命令请求。
    /// 【转换】为 command 与 authority/proof 分别计算摘要，构造带目标 stream version、request
    /// ID、幂等键和摘要的 `AutomationEventEnvelope`，再把完整 DTO 序列化到 RuntimeEvent。
    /// 【副作用】先拒绝任何会被通用 EventLog 脱敏器改写的 payload，再交给
    /// `commit_protected_event()`。后者除 workflow revision 外还 CAS 约束 authority stream，
    /// 防止授权事实在检查后并发变化。
    ///
    /// 序列化、revision 溢出、envelope 校验、敏感字段拒绝或 EventStore/CAS 失败都会返回错误；
    /// 本方法不执行 command 中的外部 effect。
    async fn commit_workflow(
        &self,
        context: &RequestContext,
        mut event: AutomationEvent,
    ) -> Result<RuntimeEvent, CoreError> {
        let expected = event.request.expected_revision;
        let command_digest = json_digest(
            &serde_json::to_value(&event.request)
                .map_err(|_| automation_error("workflow_event_encode_failed"))?,
        );
        let payload_digest = json_digest(&json!({
            "authority": &event.authority,
            "proof": &event.proof,
        }));
        event.envelope = Some(
            AutomationEventEnvelope::new(
                aggregate_id(context),
                expected
                    .checked_add(1)
                    .ok_or_else(|| automation_error("workflow_revision_exhausted"))?,
                event.authority.context.request_id,
                event.request.idempotency_key.clone(),
                command_digest,
                payload_digest,
            )
            .map_err(|_| automation_error("workflow_event_envelope_invalid"))?,
        );
        let data = serde_json::to_value(&event)
            .map_err(|_| automation_error("workflow_event_encode_failed"))?;
        // 工作流事件必须以原样、可重放的内容保存。这里的比较不是自动清洗：若公共脱敏器会
        // 改变 payload，就拒绝提交，避免“日志里保存的命令”和 planner 实际验证过的命令不同。
        if super::redaction::redact_event_value(&data) != data {
            return Err(automation_error("workflow_sensitive_payload_denied"));
        }
        let record = RuntimeEvent::new(context.request_id, 1, "workflow.command_applied", data)?
            .with_stream_metadata(
                AGGREGATE,
                aggregate_id(context),
                expected
                    .checked_add(1)
                    .ok_or_else(|| automation_error("workflow_revision_exhausted"))?,
            )
            .with_idempotency_key(format!(
                "workflow:{}:{}",
                aggregate_id(context),
                event.request.idempotency_key
            ));
        self.commit_protected_event(context, record, expected).await
    }
    /// 把刚执行或 reconciliation 得到的运行结果作为内部命令持久化，并在 revision 冲突时重试。
    ///
    /// 【调用者】dispatch effect 完成后由 `handle_workflow_command()` 调用；`Reconcile` 则先由
    /// `workflow_proof()` 得到证据，再经外部命令的常规提交路径记录该 observation。
    /// 【输入/输出】绑定 workflow instance/node 和 ControlPlane 生成的 proof；成功返回折叠后的
    /// 新状态，不直接把 proof 写成可由外部任意构造的接口。
    /// 【并发处理】最多进行 3 次“重新读状态 → 以当前 revision 规划 → CAS 提交”的尝试；只有
    /// EventStore Conflict 会重新读取，其他错误立刻向上传播。每次重试都会重算下一状态和事件，
    /// 但复用同一份运行 proof，因为重试的是 observation 事实提交，不是重新执行外部动作。
    /// 三次均冲突时返回 `workflow_observation_contention`，上游将运行标为待 reconciliation。
    ///
    /// ⚠ 不要把重试包到 dispatch 外层。那会把“写观测冲突”误变成“重新执行能力”，可能造成
    /// 重复副作用。
    async fn record_workflow_observation(
        &self,
        context: &RequestContext,
        instance_id: &str,
        node_id: &str,
        proof: AutomationProof,
    ) -> Result<AutomationState, CoreError> {
        for _ in 0..3 {
            let (state, _) = self.load_workflows(context).await?;
            let command = AutomationCommand::RecordObservation {
                instance_id: instance_id.into(),
                node_id: node_id.into(),
            };
            let a = authority(context);
            let (next, _) = plan_command(&state, &command, &a, &proof).map_err(automation_error)?;
            let event = AutomationEvent {
                schema: AUTOMATION_EVENT_SCHEMA.into(),
                request: AutomationCommandRequest {
                    schema: AUTOMATION_SCHEMA.into(),
                    expected_revision: state.revision,
                    idempotency_key: format!("observe:{instance_id}:{node_id}:{}", state.revision),
                    command,
                },
                authority: a,
                proof: proof.clone(),
                envelope: None,
            };
            let mut observation = context.clone();
            observation.request_id = RequestId::new();
            match self.commit_workflow(&observation, event).await {
                Ok(_) => return Ok(next),
                Err(CoreError::Port(PortError::Conflict(_))) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(automation_error("workflow_observation_contention"))
    }
    /// 从事件与 Company 当前投影中构造 planner 所需的证据快照；尤其负责 Reconcile 的结果恢复。
    ///
    /// 【调用者】命令处理在规划前调用；外部请求可指定 event reference，Reconcile 则用
    /// instance/node 定位已经启动的运行。
    /// 【输入/输出】读取 EventStore 全部事件，返回 `AutomationProof`。普通 Decide/Signal/Fire/
    /// RegisterTrigger 引用必须是 `event:<EventId>` 且归当前 workflow actor/root 或其 Company
    /// 事实所有；trigger 注册还要求引用到项目批准或已批准的 workflow 决策事件。
    /// Reconcile 分别从 Company Run、Capability 事件或 child workflow 状态重建 response 和证据。
    ///
    /// 【失败情况】引用格式错误、事件不存在/不归属、Run 身份不匹配、子实例缺失、序列化失败、
    /// 无运行时的节点类型以及 EventStore/Company 读取错误都会返回错误；调用者对结构化 Conflict
    /// 记拒绝事实，其他错误向上传播。查不到可确认结果时默认保持 `ResultUnknown`，而不是猜测。
    ///
    /// 这里生成的是基于本地事件/投影的 proof 输入，不是密码学签名，也不是调用上游来询问结果。
    /// `workflow_runtime_proof()` 再把运行响应与引用转换成终态 receipt/unknown incident。
    async fn workflow_proof(
        &self,
        context: &RequestContext,
        state: &AutomationState,
        command: &AutomationCommand,
    ) -> Result<AutomationProof, CoreError> {
        let all = self.events.read_all().await?;
        let mut proof = AutomationProof::default();
        // 只有这些命令携带外部证据引用。内部状态命令不要求提供 event reference，避免把
        // 所有 workflow 转换都误绑到一条任意事件上。
        let reference = match command {
            AutomationCommand::RegisterTrigger { trigger } => Some(&trigger.approval_ref),
            AutomationCommand::Decide { evidence_ref, .. }
            | AutomationCommand::Signal { evidence_ref, .. } => Some(evidence_ref),
            AutomationCommand::Fire { event_ref, .. } => event_ref.as_ref(),
            _ => None,
        };
        if let Some(reference) = reference {
            let id = reference
                .strip_prefix("event:")
                .ok_or_else(|| automation_error("workflow_event_reference_required"))?;
            let event = all
                .iter()
                .find(|e| e.event_id.to_string() == id)
                .ok_or_else(|| automation_error("workflow_evidence_not_found"))?;
            let owned = (event.aggregate_type.as_deref() == Some(AGGREGATE)
                && event.aggregate_id.as_deref() == Some(aggregate_id(context).as_str()))
                || super::company::company_event_owned(event, context, &all);
            if !owned {
                return Err(automation_error("workflow_evidence_owner_mismatch"));
            }
            if matches!(command, AutomationCommand::RegisterTrigger { .. }) {
                // Trigger 的 approval_ref 必须指向批准事实。这里只接受两个确切形状：Company
                // 项目批准事件，或 workflow.command_applied 中 approve=true 的 Decide 命令；其他
                // event 即使属于同一 actor/project，也不能用作注册 trigger 的批准凭据。
                let approved = event.kind == "company.ProjectApproved"
                    || (event.kind == "workflow.command_applied"
                        && event
                            .data
                            .pointer("/request/command/type")
                            .and_then(Value::as_str)
                            == Some("decide")
                        && event
                            .data
                            .pointer("/request/command/approve")
                            .and_then(Value::as_bool)
                            == Some(true));
                if !approved {
                    return Err(automation_error("trigger_approval_event_required"));
                }
            }
            proof.evidence_refs.push(reference.clone());
            proof.event_kind = Some(event.kind.clone());
        }
        if let AutomationCommand::Reconcile {
            instance_id,
            node_id,
        } = command
        {
            // 默认值刻意是 Unknown：只要下面没有找到足够的终态事实，就不把“没找到”说成成功、
            // 失败或取消。只有具体节点类型的证据分支才能覆盖此状态。
            let instance = state
                .instances
                .get(instance_id)
                .ok_or_else(|| automation_error("workflow_instance_not_found"))?;
            let node = instance
                .nodes
                .get(node_id)
                .ok_or_else(|| automation_error("workflow_node_not_started"))?;
            let definition = &state.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            let mut response = CoreResponse {
                request_id: node.execution_id,
                status: ExecutionStatus::ResultUnknown,
                output: Value::Null,
                error: Some("workflow_execution_unobserved".into()),
            };
            match &definition.nodes[node_id].kind {
                WorkflowNodeKind::AgentTask {
                    project_id,
                    packet_id,
                    ..
                } => {
                    let (company, _) = self.load_company(context).await?;
                    if let Some(run) = company.runs.get(packet_id) {
                        if run.author_session_id != node.session_id || run.project_id != *project_id
                        {
                            return Err(automation_error("workflow_run_identity_mismatch"));
                        }
                        match super::company::observe_company_run(run, context, &all) {
                            Ok(observed) => {
                                response.status = observed.status;
                                response.output = serde_json::to_value(&observed)
                                    .map_err(|_| automation_error("workflow_run_encode_failed"))?;
                                response.error = None;
                                proof.evidence_refs = observed.evidence_refs;
                            }
                            Err(PortError::Conflict(reason))
                                if reason == "company_run_reconciliation_unavailable" =>
                            {
                                // Company 状态存在，但当前证据不足以给出终态。保留 Unknown 并
                                // 附上执行 ID 可关联到的事实，供下一轮 reconciliation 继续使用。
                                response.status = ExecutionStatus::ResultUnknown;
                                response.error = Some(reason);
                                proof.evidence_refs =
                                    execution_event_refs(&all, node.execution_id, true);
                            }
                            Err(error) => return Err(CoreError::from(error)),
                        }
                    }
                    // 没有匹配 packet_id 的 Company Run 时保留默认 Unknown；不创建新 Run 来“补救”。
                }
                WorkflowNodeKind::Capability { .. } => {
                    let events: Vec<_> = all
                        .iter()
                        .filter(|e| e.request_id == node.execution_id)
                        .collect();
                    proof.evidence_refs = events
                        .iter()
                        .map(|e| format!("event:{}", e.event_id))
                        .collect();
                    if let Some(event) = events.iter().rev().find(|e| {
                        matches!(
                            e.kind.as_str(),
                            "capability.completed"
                                | "capability.failed"
                                | "capability.result_unknown"
                                | "capability.blocked"
                        )
                    }) {
                        // 只将明确的 capability 结果事件映射为运行结果。ResultUnknown 保持未知；
                        // 不根据普通成功/失败日志或请求本身推断 capability 已完成。
                        response.status = match event.kind.as_str() {
                            "capability.completed" => ExecutionStatus::Completed,
                            "capability.failed" => ExecutionStatus::Failed,
                            "capability.blocked" => ExecutionStatus::Blocked,
                            _ => ExecutionStatus::ResultUnknown,
                        };
                        response.output = event.data.clone();
                        response.error = event
                            .data
                            .get("error")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    } else if events.iter().any(|e| e.kind == "approval.denied") {
                        response.status = ExecutionStatus::Cancelled;
                        response.error = Some("workflow_approval_denied".into());
                    } else if events.iter().any(|e| e.kind == "approval.requested")
                        && now_ms() < node.lease_expires_at
                    {
                        // 审批仍待处理且节点 lease 未过期时，结果是 AwaitingApproval，不是 Unknown。
                        response.status = ExecutionStatus::AwaitingApproval;
                    }
                }
                WorkflowNodeKind::SubWorkflow { .. } => {
                    let child = node
                        .child_instance_id
                        .as_ref()
                        .and_then(|id| state.instances.get(id))
                        .ok_or_else(|| automation_error("workflow_child_instance_missing"))?;
                    response.status = match child.status {
                        WorkflowInstanceStatus::Succeeded => ExecutionStatus::Completed,
                        WorkflowInstanceStatus::Failed => ExecutionStatus::Failed,
                        WorkflowInstanceStatus::Cancelled => ExecutionStatus::Cancelled,
                        WorkflowInstanceStatus::ResultUnknown => ExecutionStatus::ResultUnknown,
                        _ => ExecutionStatus::Running,
                    };
                    response.output =
                        json!({"child_instance_id":child.instance_id,"outputs":child.outputs});
                    response.error = None;
                    proof.evidence_refs = all
                        .iter()
                        .filter(|e| {
                            e.aggregate_type.as_deref() == Some(AGGREGATE)
                                && e.aggregate_id.as_deref() == Some(aggregate_id(context).as_str())
                        })
                        .map(|e| format!("event:{}", e.event_id))
                        .collect();
                }
                _ => return Err(automation_error("workflow_node_has_no_runtime")),
            }
            // 运行 lease 是 Core 写入 workflow 节点时记录的观察期限，不会取消底层操作。过期后，
            // 仍处于 Running/Accepted 的结果降为 Unknown，避免仅凭超时声称外部动作没有发生。
            if matches!(
                response.status,
                ExecutionStatus::Running | ExecutionStatus::Accepted
            ) && now_ms() >= node.lease_expires_at
            {
                response.status = ExecutionStatus::ResultUnknown;
                response.error = Some("workflow_execution_lease_expired".into());
            }
            proof =
                workflow_runtime_proof(instance_id, node_id, response, proof.evidence_refs, &all)?;
        }
        Ok(proof)
    }

    /// 向目标 workflow 及其子 workflow 的未终结节点发送取消请求。
    ///
    /// 【调用者】显式 Cancel effect，以及 Tick/Fire 导致实例进入 CancelRequested 后由命令处理
    /// 路径调用。
    /// 【输入】使用刚规划出的 `AutomationState` 确定目标和 descendants；对应定义提供节点种类，
    /// Company state 与 EventStore 提供审批/运行身份。
    /// 【处理】已终结节点跳过；Capability 节点查找其 execution 对应的待审批请求并提交 Deny；
    /// AgentTask 节点若已有 Company Run，则核对 author session 后调用 `cancel_run()`。
    /// 【副作用/失败】取消通过现有审批与 Company 生命周期入口完成，不直接操作 Broker 或外部
    /// 进程。任一读取、身份检查或取消调用失败会停止循环并向上传播；先前已发出的取消请求不会
    /// 自动回滚。
    ///
    /// ⚠ 请求取消不等于确认取消，也不等于补偿（compensation）。能力/AgentTask 的外部效果可能
    /// 已发生；节点会保持未决，直到 `Reconcile` 读到终态事实。本函数不会声称已撤销这些效果。
    async fn cancel_workflow_effects(
        &self,
        context: &RequestContext,
        state: &AutomationState,
        instance_id: &str,
    ) -> Result<(), CoreError> {
        if !state.instances.contains_key(instance_id) {
            return Err(automation_error("workflow_instance_not_found"));
        }
        for instance in state.instances.values().filter(|i| {
            i.instance_id == instance_id
                || kiana_workflow::workflow_descends_from(state, &i.instance_id, instance_id)
        }) {
            let definition = &state.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            let (company, _) = self.load_company(context).await?;
            for (node_id, node) in &instance.nodes {
                if node.status.terminal() {
                    continue;
                }
                if let WorkflowNodeKind::Capability { .. } = &definition.nodes[node_id].kind {
                    let events = self.events.read_all().await?;
                    if let Some(approval_id) = events
                        .iter()
                        .filter(|e| {
                            e.request_id == node.execution_id && e.kind == "approval.requested"
                        })
                        .find_map(|e| e.data.get("approval_id").cloned())
                        .and_then(|id| serde_json::from_value::<ApprovalId>(id).ok())
                    {
                        let mut cancel = context.clone();
                        cancel.request_id = RequestId::new();
                        cancel.session_id = node.session_id.clone();
                        let _ = self
                            .decide_approval(&cancel, approval_id, ApprovalDecision::Deny)
                            .await?;
                    }
                }
                if let WorkflowNodeKind::AgentTask { packet_id, .. } =
                    &definition.nodes[node_id].kind
                {
                    if let Some(run) = company.runs.get(packet_id) {
                        if run.author_session_id != node.session_id {
                            return Err(automation_error("workflow_run_identity_mismatch"));
                        }
                        let mut cancel = context.clone();
                        cancel.request_id = RequestId::new();
                        cancel.session_id = node.session_id.clone();
                        let _ = self
                            .cancel_run(cancel, run.run_id, "workflow_cancelled".into())
                            .await?;
                    }
                }
                // Even after a cancellation request the node remains unresolved until Reconcile reads
                // terminal facts. There is no claim that an external capability was rolled back.
            }
        }
        Ok(())
    }
}
