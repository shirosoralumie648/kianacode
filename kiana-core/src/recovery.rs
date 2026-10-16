//! 恢复：把一次中断的 run 重新接上，并把它留下的 Cell 归位。
//!
//! # 「恢复」不等于「重试」
//!
//! 恢复的前提是**当初那次执行到底发生了什么并不完全清楚**。
//! 这件事一旦搞错，代价是重复执行或状态错乱，所以这个文件里的每个判断都偏向
//! 「不确定就保持不确定」：
//! - 状态机不允许从 `ResultUnknown` 走回成功（见 `states.rs`）；
//! - 恢复只还原已有 Run 和必要的待审批续体；之后的能力仍交给同一个 ControlPlane
//!   的授权/dispatch/Broker 路径，不另开模型循环；
//! - Cell 的归位由**已知的执行状态**决定，而不是由「我们希望它成功」决定。
use super::redaction::*;
use super::*;
use kiana_domain::{json_digest, ApprovalView, RunSnapshot};

fn resume_unknown_response(context: &RequestContext, run_id: RunId, reason: &str) -> CoreResponse {
    let error = format!("result_unknown:{reason}");
    CoreResponse {
        request_id: context.request_id,
        status: ExecutionStatus::ResultUnknown,
        output: json!({
            "run_id": run_id,
            "resume": "denied",
            "unknown": true,
            "reason": reason,
        }),
        error: Some(error),
    }
}

impl ControlPlane {
    /// 从运行快照、事件投影和 ApprovalStore 重建一个尚待人工决定的 Run invocation。
    ///
    /// 【调用者与输入】
    /// 仅由 [`Self::resume_run`] 调用。`context` 和 `sandbox` 是恢复时重新建立并检查过的
    /// 服务端范围；`events` 是该 Run 的事件，`projections` 是这些事件折叠出的能力执行视图；
    /// `snapshot_pending` 是旧版/已有快照中可能保存的待续体。当前 checkpoint 会把该字段置空，
    /// 因此通常要从审批事件和 ApprovalStore 重建执行材料。
    ///
    /// 【核心流程】
    /// 找出唯一一个 AwaitingApproval 的能力投影；通过 ApprovalStore 读取认证过的 challenge
    /// 和原始待执行请求；再核对 ApprovalId、RequestId、RunId、动作摘要、request hash、主体、
    /// session、过期时间、sandbox 与恢复绑定。多个候选、材料缺失或任一字段不一致都会 fail-closed。
    ///
    /// 【输出与失败】
    /// 没有待批能力时返回 `Ok(None)`；成功时返回可交给后续 prepare/authorize 的
    /// `PendingInvocation`。ApprovalStore/EventLog 错误或证据矛盾返回 `CoreError`，由
    /// `resume_run` 转成拒绝响应。此函数只读事实并构造内存值，不执行工具、不写 Store。
    ///
    /// 【为什么不能从 UI 列表直接恢复】
    /// 列表是脱敏展示投影，可能缺少原始参数和执行范围；恢复必须重新从受控 Store 取回材料，
    /// 并与已提交的 approval.requested 事件及 Run 投影交叉核对。
    async fn rebuild_pending_invocation(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        events: &[RuntimeEvent],
        projections: &[InvocationProjection],
        snapshot_pending: Option<PendingInvocation>,
    ) -> Result<Option<PendingInvocation>, CoreError> {
        let mut pending = match snapshot_pending {
            Some(pending) => pending,
            None => {
                let pending_projections = projections
                    .iter()
                    .filter(|projection| {
                        projection.state == kiana_domain::CapabilityExecutionState::AwaitingApproval
                            && projection.approval_id.is_some()
                            && projection.request.is_some()
                    })
                    .collect::<Vec<_>>();
                let projection = match pending_projections.as_slice() {
                    [] => return Ok(None),
                    [projection] => *projection,
                    _ => {
                        return Err(
                            PortError::Failed("approval_continuation_ambiguous".to_owned()).into(),
                        )
                    }
                };
                let approval_id = projection.approval_id.expect("checked above");
                let approval_context = RequestContext {
                    request_id: projection.request_id,
                    ..context.clone()
                };
                // Journal 可返回与当前主体绑定的 challenge，包括已经批准但尚未恢复的 subject。
                // 列表/UI 请求只是展示投影，不能当作可执行请求材料。
                let decision = self
                    .approvals
                    .read_decision(&approval_context, approval_id)
                    .await?;
                if decision.approval_id != approval_id
                    || decision.challenge.approval_id != approval_id
                    || decision.challenge.request_id != projection.request_id
                {
                    return Err(PortError::Failed("approval_binding_changed".to_owned()).into());
                }
                let material = self
                    .approvals
                    .pending_with_proof(
                        &approval_context,
                        approval_id,
                        Some(&decision.challenge.request_hash),
                        Some(&decision.challenge.nonce),
                    )
                    .await?;
                let approval_event = events.iter().rev().find(|event| {
                    event.kind == "approval.requested"
                        && event.data.get("approval_id") == Some(&json!(approval_id))
                        && event.data.get("capability_request_id")
                            == Some(&json!(material.request.request_id))
                });
                let approval_event = approval_event.ok_or_else(|| {
                    CoreError::Port(PortError::Failed(
                        "approval_continuation_unavailable".to_owned(),
                    ))
                })?;
                let resume_binding = match approval_event.data.get("resume_binding") {
                    Some(value) => Some(serde_json::from_value(value.clone()).map_err(|_| {
                        CoreError::Port(PortError::Failed(
                            "approval_resume_binding_invalid".to_owned(),
                        ))
                    })?),
                    None => None,
                };
                PendingInvocation {
                    approval_id,
                    challenge: material.challenge,
                    request_id: material.request.request_id,
                    event_request_id: approval_event.request_id,
                    event_sequence: events
                        .iter()
                        .filter(|candidate| candidate.request_id == approval_event.request_id)
                        .map(|candidate| candidate.sequence)
                        .max()
                        .unwrap_or(approval_event.sequence),
                    run_id,
                    request: material.request,
                    context: context.clone(),
                    sandbox: sandbox.to_owned(),
                    resume_binding,
                }
            }
        };
        let Some(projection) = projections
            .iter()
            .find(|projection| projection.request_id == pending.request.request_id)
        else {
            return Err(
                PortError::Failed("run_snapshot_pending_invocation_missing".to_owned()).into(),
            );
        };
        if pending.run_id != run_id {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_run_mismatch".to_owned(),
            )
            .into());
        }
        if pending.approval_id != pending.challenge.approval_id {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_approval_mismatch".to_owned(),
            )
            .into());
        }
        if pending.challenge.request_id != pending.request.request_id {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_request_mismatch".to_owned(),
            )
            .into());
        }
        if projection.state != kiana_domain::CapabilityExecutionState::AwaitingApproval {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_state_mismatch".to_owned(),
            )
            .into());
        }
        if projection.approval_id != Some(pending.approval_id) {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_approval_mismatch".to_owned(),
            )
            .into());
        }
        let approval_context = RequestContext {
            request_id: pending.request.request_id,
            ..context.clone()
        };
        let material = self
            .approvals
            .pending_with_proof(
                &approval_context,
                pending.approval_id,
                Some(&pending.challenge.request_hash),
                Some(&pending.challenge.nonce),
            )
            .await?;
        if material.request.request_id != pending.request.request_id
            || material.challenge.approval_id != pending.approval_id
            || material.challenge.request_id != material.request.request_id
        {
            return Err(PortError::Failed("approval_binding_changed".to_owned()).into());
        }
        let scope = material
            .request
            .execution_scope
            .as_ref()
            .ok_or_else(|| PortError::Failed("approval_execution_scope_missing".to_owned()))?;
        scope
            .validate_for_request(&material.request)
            .map_err(PortError::Failed)?;
        let material_digest = kiana_domain::capability_action_digest(&material.request);
        let display = projection.request.as_ref().ok_or_else(|| {
            PortError::Failed("run_snapshot_pending_invocation_missing".to_owned())
        })?;
        // 事件参数和 execution_scope 已经过通用脱敏，因此这里只比较可公开的身份字段和原始
        // action digest；受保护请求的 scope/binding 仍需验证，随后还要重新 prepare/authorize。
        if display.request_id != material.request.request_id
            || display.capability != material.request.capability
            || display.operation != material.request.operation
            || display.risk != material.request.risk
            || display.cell_id != material.request.cell_id
            || display.capability_grant_id != material.request.capability_grant_id
            || display.budget_lease_id != material.request.budget_lease_id
            || projection.args_fingerprint.as_deref() != Some(material_digest.as_str())
        {
            return Err(PortError::Failed("approval_action_changed".to_owned()).into());
        }
        let approval_event = events
            .iter()
            .rev()
            .find(|event| {
                event.kind == "approval.requested"
                    && event.data.get("approval_id") == Some(&json!(pending.approval_id))
                    && event.data.get("capability_request_id")
                        == Some(&json!(material.request.request_id))
            })
            .ok_or_else(|| PortError::Failed("approval_continuation_unavailable".to_owned()))?;
        if approval_event.request_id != pending.event_request_id
            || approval_event.data.get("run_id") != Some(&json!(run_id))
            || approval_event.data.get("action_digest") != Some(&json!(material_digest))
            || approval_event.data.get("request_hash")
                != Some(&json!(material.challenge.request_hash))
            || approval_event.data.get("session_id") != Some(&json!(context.session_id))
            || approval_event.data.get("actor_id") != Some(&json!(context.actor_id))
            || approval_event.data.get("expires_at_unix_ms")
                != Some(&json!(material.challenge.expires_at_unix_ms))
        {
            return Err(PortError::Failed("approval_binding_changed".to_owned()).into());
        }
        let resume_binding = match pending.resume_binding.as_ref() {
            Some(binding) => {
                binding
                    .validate_against(
                        run_id,
                        pending.event_request_id,
                        &material.request,
                        context,
                        sandbox,
                    )
                    .map_err(PortError::Failed)?;
                binding.clone()
            }
            None => {
                let pending_batch_digest = material
                    .request
                    .arguments
                    .get("pending_batch_digest")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        json_digest(&json!({
                            "request_id": material.request.request_id,
                            "call_id": material.request.arguments.get("call_id"),
                        }))
                    });
                kiana_domain::InvocationResumeBinding::from_request(
                    run_id,
                    pending.event_request_id,
                    &material.request,
                    context,
                    sandbox,
                    pending_batch_digest,
                )
                .map_err(PortError::Failed)?
            }
        };
        pending.challenge = material.challenge;
        pending.request = material.request;
        pending.context = context.clone();
        pending.resume_binding = Some(resume_binding);
        // 游标属于本次续体所用的 request stream。全局事件序号可能属于无关 aggregate，
        // 把它用在这里会让陈旧审批覆盖其他事实的局部顺序。
        let latest_sequence = events
            .iter()
            .filter(|event| event.request_id == pending.event_request_id)
            .map(|event| event.sequence)
            .max()
            .unwrap_or(pending.event_sequence);
        pending.event_sequence = latest_sequence;
        Ok(Some(pending))
    }

    /// 列出当前调用主体可见的待审批项目，并返回脱敏后的展示视图。
    ///
    /// 【调用者与输入】
    /// daemon 的 PendingApprovals 请求会调用此方法；`context` 必须是已认证的服务端上下文。
    /// 若提供 `requested_run`，它只用于通过 `resolve_run_id` 检查当前 session 绑定的 Run 身份；
    /// 之后的 `ApprovalStore::list_pending(context)` 按 context 的范围返回项目，本方法没有再按
    /// run_id 过滤列表。
    ///
    /// 【数据流与输出】
    /// 方法从 ApprovalStore 读取 pending 项，再读取可用的审批版本/执行材料标志和保存的范围，
    /// 将请求参数脱敏后组装为 `ApprovalView`，最后放入 Completed 响应。未知决策读取能力或
    /// 执行材料不可恢复时仍可返回不含可执行材料的展示项；其他存储错误传播为 `CoreError`。
    ///
    /// 【边界】
    /// 这是只读列表，不消费审批、不创建 proof、不向 Broker 派发。界面返回的数据不可直接
    /// 用于恢复执行；真正批准仍需提交 proof，由决策/恢复路径从 Store 验证原始材料。
    pub async fn list_pending_approvals(
        &self,
        context: &RequestContext,
        requested_run: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        if let Some(run_id) = requested_run {
            if let Err(reason) = self.resolve_run_id(context, Some(run_id)).await? {
                return Ok(CoreResponse::blocked(context.request_id, reason));
            }
        }
        let mut approvals = Vec::new();
        for pending in self.approvals.list_pending(context).await? {
            let (expected_version, payload_available) = match self
                .approvals
                .read_decision(context, pending.challenge.approval_id)
                .await
            {
                Ok(record) => (Some(record.version), record.payload_available),
                Err(PortError::Unavailable(reason))
                    if reason == "approval_decision_read_unsupported" =>
                {
                    (None, false)
                }
                Err(PortError::Failed(reason)) if reason == "approval_payload_unrecoverable" => {
                    (None, false)
                }
                Err(error) => return Err(error.into()),
            };
            let scope = self
                .approvals
                .context_for_pending(context, pending.challenge.approval_id)
                .await?;
            let plan_preview = kiana_domain::ApprovalPlanPreview::from_pending(
                &pending,
                &scope,
                payload_available,
            )
            .map_err(PortError::Failed)?;
            approvals.push(ApprovalView {
                permission_profile: Some(scope.permission_profile),
                challenge: pending.challenge,
                operation: pending.request.operation,
                arguments: redact_event_value(&pending.request.arguments),
                available_decisions: vec![ApprovalDecision::Approve, ApprovalDecision::Deny],
                expected_version,
                plan_preview: Some(plan_preview),
            });
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"approvals": approvals}),
        ))
    }

    /// 把当前 Run 的可恢复执行状态写成 EventLog 快照。
    ///
    /// 【调用者与输入】
    /// Runner 生命周期/终态处理在需要暂停或检查点时调用；参数是服务端上下文、RunId、已授权
    /// sandbox 和该请求流的事件序号游标。
    ///
    /// 【核心流程与数据流】
    /// 先从 RunnerPort 取得 runner state，再收集 Cell 状态、data epoch、authority revision、
    /// role prompt hash 和 state digest，序列化为 `RunSnapshot` 后经过通用脱敏，最后追加
    /// `run.snapshot`。任何字段被脱敏时将 `resumable` 改为 false，防止把占位符误当原始执行材料。
    /// 快照不保存 pending invocation 的原始执行请求；待批材料留在 ApprovalStore，恢复时按
    /// 事件中的身份/摘要重新获取并验证。写入成功后还会更新内存待续体的事件序号。
    ///
    /// 【输出与失败】
    /// 写入/状态读取成功返回 `Ok(())`；RunnerPort 明确返回 Unavailable 时视为 adapter 不支持
    /// checkpoint，直接返回 `Ok(())` 且不写快照；其他 Runner、Cell、版本读取或 EventLog 错误
    /// 传播为 `CoreError`。
    pub(crate) async fn checkpoint_run(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        sequence: &mut u64,
    ) -> Result<(), CoreError> {
        let runner_state = match self.runner.checkpoint(run_id).await {
            Ok(state) => state,
            Err(PortError::Unavailable(_)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let snapshot = RunSnapshot {
            cell_state: if context.cell_id.is_some() {
                Some(self.cell_registry.checkpoint_run(run_id).await?)
            } else {
                None
            },
            data_epoch: self.data_epoch(&context.project_root).await?,
            authority_revision: self.authority_revision(&context.project_root).await?,
            schema: "kiana.run-snapshot.v1".to_owned(),
            run_id,
            context: context.clone(),
            sandbox: sandbox.to_owned(),
            role_prompt_hash: RoleSpec::lookup(&context.role_id)
                .map(|role| role.prompt_hash)
                .unwrap_or_default(),
            runner_state_digest: json_digest(&runner_state),
            runner_state,
            // 已提交的 approval.requested 身份/摘要才是续体引用；原始执行材料留在 ApprovalStore，
            // 恢复时必须重新通过 proof 和范围校验，不能信任快照中复制的一份请求。
            pending_invocation: None,
            resumable: true,
        };
        let raw =
            serde_json::to_value(snapshot).map_err(|error| PortError::Failed(error.to_string()))?;
        let mut safe = redact_event_value(&raw);
        // 脱敏后的参数/历史只是展示占位内容，不能冒充原始请求继续执行。
        if safe != raw {
            safe["resumable"] = json!(false);
        }
        self.record_event(
            context.request_id,
            sequence,
            "run.snapshot",
            json!({"run_id":run_id,"snapshot":safe}),
        )
        .await?;
        for pending in self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values_mut()
        {
            if pending.run_id == run_id {
                pending.event_sequence = *sequence;
            }
        }
        Ok(())
    }

    /// 根据最近一次安全快照恢复一个已有 Run；不创建新 Run，也不直接执行快照里的工具参数。
    ///
    /// 【调用者与输入】
    /// daemon 的 `RequestBody::Resume` 路径调用此方法。输入是当前服务端身份上下文和可选 RunId；
    /// `None` 时由 `resolve_run_id` 按该主体可访问的会话解析目标。
    ///
    /// 【核心流程】
    /// 1. 拒绝内部 memory-distillation 会话，并解析/校验 Run 所有者；
    /// 2. 读取 Run stream 版本、Run 事件、能力执行投影和最近快照；
    /// 3. 检查快照可恢复标记、schema、Runner state 摘要、authority/data 代次、主体范围、角色和 sandbox；
    /// 4. 拒绝快照之后存在不允许的事件或已有 resume claim 的重复恢复；
    /// 5. 如有审批绑定的调用，从 ApprovalStore 重建材料并重新 prepare/authorize；
    /// 6. 以观察到的 stream version 追加一次 `run.resume_prepared`，再恢复 Cell/Runner；
    /// 7. 活跃审批恢复到 AwaitingApproval；已批准材料走已有的批准续体路径，消费一次后进入同一
    ///    `drive_run`。缺材料、状态变化或恢复证据不完整时 fail-closed。
    ///
    /// 【副作用、输出与失败】
    /// 会读取 EventStore、ApprovalStore 和 Runner/Cell 状态，成功 claim 后会写事件、恢复 Runner，
    /// 并可能重新登记待批 invocation；它不会在本方法内运行模型或派发能力。无待批调用时的
    /// `Completed` 只表示 Resume 控制请求完成，不代表该 Run 的业务任务已进入 Completed 终态。
    /// 证据不符时返回
    /// Blocked CoreResponse；端口、存储或还原错误可能返回 CoreError。拒绝分支覆盖快照损坏、
    /// scope/sandbox/authority/data 变化、快照陈旧、审批变化和重复 claim。
    ///
    /// ⚠ `run.resume_prepared` 的 CAS 在真正恢复 Runner/Cell 之前持久化。若 claim 成功后还原
    /// 失败，当前实现不会撤销该 claim；随后同一快照的重试会被识别为重复 claim。需要先对账或
    /// 有明确恢复证据，不能简单删掉 claim 再重跑，以免同时恢复出两个 Runner。
    pub async fn resume_run(
        &self,
        mut context: RequestContext,
        requested_run: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        if context
            .session_id
            .as_str()
            .starts_with(kiana_domain::MEMORY_DISTILL_SESSION_PREFIX)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "memory_distillation_internal_resume_denied",
            ));
        }
        let run_id = match self.resolve_run_id(&context, requested_run).await? {
            Ok(id) => id,
            Err(reason) => return Ok(CoreResponse::blocked(context.request_id, reason)),
        };
        // 先固定本次读取到的 Run stream 版本，再检查其他事实。若期间有并发追加，后面的 CAS
        // 只能失败；不能拿一个尚未检查过的新版本冒充已验证状态。
        let observed_run_stream = self.events.read_stream("run", &run_id.to_string()).await?;
        let last_version = observed_run_stream
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let events = self
            .read_all_events()
            .await?
            .ok_or_else(|| PortError::Unavailable("run_resume_unsupported".to_owned()))?;
        let events = crate::receipts::try_filter_run_events(&events, run_id)
            .map_err(|reason| CoreError::Port(PortError::Failed(reason)))?;
        let run_projection = match crate::project_run_state(run_id, &events) {
            Ok(state) => state,
            Err(error) => {
                return Ok(resume_unknown_response(
                    &context,
                    run_id,
                    &format!("run_projection_invalid:{error}"),
                ));
            }
        };
        if run_projection.phase == crate::RunPhase::Cancelling {
            return Ok(resume_unknown_response(
                &context,
                run_id,
                "run_resume_cancel_fence",
            ));
        }
        if let Some(outcome) = run_projection.outcome {
            let reason = match outcome {
                crate::RunOutcome::Completed => "run_resume_terminal_completed",
                crate::RunOutcome::Failed => "run_resume_terminal_failed",
                crate::RunOutcome::Cancelled => "run_resume_terminal_cancelled",
                crate::RunOutcome::ResultUnknown => "run_resume_result_unknown",
            };
            return Ok(resume_unknown_response(&context, run_id, reason));
        }
        let projections = self.cache_invocation_projection(run_id, &events)?;
        let Some((index, event)) = events
            .iter()
            .enumerate()
            .rev()
            .find(|(_, event)| event.kind == "run.snapshot")
        else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_continuation_unavailable",
            ));
        };
        let snapshot_value = event.data.get("snapshot").cloned().unwrap_or(Value::Null);
        // 旧快照的 pending_invocation 可能含被脱敏成占位值的 fencing_token。
        // 必须先检查 resumable 拒绝标志再解码，绝不能把脱敏后的材料复活成可执行对象。
        if snapshot_value.get("resumable").and_then(Value::as_bool) != Some(true) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_snapshot_invalid",
            ));
        }
        let mut snapshot: RunSnapshot = match serde_json::from_value(snapshot_value) {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "run_snapshot_invalid",
                ))
            }
        };
        let role = RoleSpec::lookup(&context.role_id)
            .ok_or_else(|| PortError::Failed("role_unknown".to_owned()))?;
        if snapshot.authority_revision != self.authority_revision(&context.project_root).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_authority_changed",
            ));
        }
        if snapshot.data_epoch != self.data_epoch(&context.project_root).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_data_revoked",
            ));
        }
        if snapshot.schema != "kiana.run-snapshot.v1"
            || !snapshot.resumable
            || snapshot.run_id != run_id
            || json_digest(&snapshot.runner_state) != snapshot.runner_state_digest
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_snapshot_invalid",
            ));
        }
        context = self.approval_scope(&context).await?;
        if snapshot.context.work_packet_id.is_some() {
            self.bind_company_run_scope(&mut context, &snapshot.sandbox, false)
                .await?;
        }
        let original = &snapshot.context;
        if original.session_id != context.session_id
            || original.actor_id != context.actor_id
            || original.project_root != context.project_root
            || original.project_trusted != context.project_trusted
            || original.permission_profile != context.permission_profile
            || original.role_id != context.role_id
            || original.department_id != context.department_id
            || original.path_allow != context.path_allow
            || snapshot.role_prompt_hash != role.prompt_hash
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_scope_changed",
            ));
        }
        if crate::lifecycle::authorized_harness_sandbox(&context, Some(&snapshot.sandbox)).is_err()
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_sandbox_denied",
            ));
        }
        if events[index + 1..].iter().any(|event| {
            !matches!(
                event.kind.as_str(),
                "approval.continuation_unavailable"
                    | "run.resume_prepared"
                    | "request.accepted"
                    | "cell.waiting_input"
                    | "packet.awaiting_approval"
            )
        }) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_snapshot_stale",
            ));
        }
        // 每份快照最多 claim 一次。第二个 Resume 不能从同一暂停材料再次恢复 Runner，
        // 也不能重新打开同一个待批能力。先返回稳定冲突，避免重复安装 authority/effect 边界；
        // 并发请求还会在后面的 CAS 失败分支再次核对是否已有相同 claim。
        if events[index + 1..].iter().any(|claim_event| {
            claim_event.kind == "run.resume_prepared"
                && claim_event.data.get("snapshot_event_id") == Some(&json!(event.event_id))
        }) {
            return Ok(resume_unknown_response(
                &context,
                run_id,
                "run_resume_claim_conflict",
            ));
        }
        let mut pending_invocation = match self
            .rebuild_pending_invocation(
                &context,
                run_id,
                &snapshot.sandbox,
                &events,
                &projections,
                snapshot.pending_invocation.clone(),
            )
            .await
        {
            Ok(pending) => pending,
            Err(error) => return Ok(CoreResponse::blocked(context.request_id, error.to_string())),
        };
        let pending_approval_state = if let Some(pending) = &pending_invocation {
            let approval_context = RequestContext {
                request_id: pending.request.request_id,
                ..context.clone()
            };
            let record = self
                .approvals
                .read_decision(&approval_context, pending.approval_id)
                .await
                .map_err(CoreError::from)?;
            let prepared = match self
                .prepare_capability_action(
                    &context,
                    pending.request.clone(),
                    Some(&snapshot.sandbox),
                    true,
                )
                .await
            {
                Ok(prepared) if prepared == pending.request => prepared,
                Ok(_) => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "approval_action_changed",
                    ))
                }
                Err(error) => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        redact_event_text(&error.to_string()),
                    ))
                }
            };
            if record.state == kiana_domain::ApprovalState::Active {
                let (_, gate) = self
                    .authorize_capability_action(&context, &prepared, None)
                    .await?;
                match gate {
                    GateDecision::AwaitingApproval { .. } => {
                        Some(kiana_domain::ApprovalState::Active)
                    }
                    GateDecision::Denied { reason } => {
                        return Ok(CoreResponse::blocked(context.request_id, reason))
                    }
                    GateDecision::Allowed { .. } => {
                        return Ok(CoreResponse::blocked(
                            context.request_id,
                            "approval_requirements_changed",
                        ))
                    }
                }
            } else {
                let (_, gate) = self
                    .authorize_capability_action(
                        &context,
                        &prepared,
                        Some((pending.approval_id, &pending.challenge.reason)),
                    )
                    .await?;
                match (record.state, gate) {
                    (kiana_domain::ApprovalState::Approved, GateDecision::Allowed { .. }) => {
                        Some(kiana_domain::ApprovalState::Approved)
                    }
                    (_, GateDecision::Denied { reason }) => {
                        return Ok(CoreResponse::blocked(context.request_id, reason))
                    }
                    _ => {
                        return Ok(CoreResponse::blocked(
                            context.request_id,
                            "approval_requirements_changed",
                        ))
                    }
                }
            }
        } else {
            None
        };
        let resume_turn = kiana_domain::TurnIdentity::new(
            context.session_id.as_str(),
            run_id,
            kiana_domain::TurnId::from_uuid(context.request_id.as_uuid()),
            None,
            kiana_domain::TurnSemantics::Resume,
            events
                .iter()
                .filter(|event| event.kind == "run.prompt")
                .count()
                .saturating_add(1) as u64,
        )
        .map_err(|error| PortError::Failed(format!("turn_identity_invalid:{error}")))?;
        // 以刚才观察并检查过的精确 stream version 执行 CAS，再安装 Runner。
        let claim = RuntimeEvent::new(
            context.request_id,
            1,
            "run.resume_prepared",
            json!({
                "run_id":run_id,"session_id":context.session_id,"actor_id":context.actor_id,
                "snapshot_event_id":event.event_id,
                "turn_id":resume_turn.turn_id,"turn":resume_turn,
            }),
        )?
        .with_stream_metadata("run", run_id.to_string(), last_version + 1);
        match self.events.append_expected(claim, Some(last_version)).await {
            Ok(()) => {}
            Err(PortError::Conflict(reason)) => {
                // 读取之后、CAS 之前可能有另一个 Resume 抢先 claim 同一快照。先重读 stream，
                // 如果确实已有该快照的 claim，就返回与串行重复请求相同的稳定冲突，而不是泛化错误。
                let claimed = self
                    .events
                    .read_stream("run", &run_id.to_string())
                    .await?
                    .iter()
                    .any(|claim_event| {
                        claim_event.kind == "run.resume_prepared"
                            && claim_event.data.get("snapshot_event_id")
                                == Some(&json!(event.event_id))
                    });
                if claimed {
                    return Ok(resume_unknown_response(
                        &context,
                        run_id,
                        "run_resume_claim_conflict",
                    ));
                }
                return Err(PortError::Conflict(reason).into());
            }
            Err(error) => return Err(error.into()),
        }
        if snapshot.authority_revision != self.authority_revision(&context.project_root).await? {
            return Ok(resume_unknown_response(
                &context,
                run_id,
                "run_resume_authority_changed",
            ));
        }
        if snapshot.data_epoch != self.data_epoch(&context.project_root).await? {
            return Ok(resume_unknown_response(
                &context,
                run_id,
                "run_resume_data_revoked",
            ));
        }
        if snapshot.context.cell_id.is_some() {
            let Some(state) = snapshot.cell_state.take() else {
                return Ok(resume_unknown_response(
                    &context,
                    run_id,
                    "run_resume_cell_snapshot_missing",
                ));
            };
            if let Err(error) = self.cell_registry.restore_run(run_id, state).await {
                return Ok(resume_unknown_response(
                    &context,
                    run_id,
                    &format!("run_resume_cell_restore_failed:{error}"),
                ));
            }
        }
        if let Err(error) = self.runner.restore(run_id, snapshot.runner_state).await {
            return Ok(resume_unknown_response(
                &context,
                run_id,
                &format!("run_resume_runner_restore_failed:{error}"),
            ));
        }
        self.remember_session(&context, run_id);
        if let (Some(mut pending), Some(state)) =
            (pending_invocation.take(), pending_approval_state)
        {
            pending.context.request_id = context.request_id;
            pending.event_request_id = context.request_id;
            if let Some(binding) = pending.resume_binding.as_mut() {
                binding
                    .rebind_event_request(context.request_id)
                    .map_err(PortError::Failed)?;
            }
            pending.event_sequence = 2;
            let challenge = pending.challenge.clone();
            let resume_binding = pending.resume_binding.clone();
            if state == kiana_domain::ApprovalState::Active {
                self.pending_invocations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(pending.approval_id, pending);
                return Ok(CoreResponse {
                    request_id: context.request_id,
                    status: ExecutionStatus::AwaitingApproval,
                    output: json!({
                        "run_id":run_id,
                        "session_id":context.session_id,
                        "restored":true,
                        "approval":challenge,
                        "resume_binding":resume_binding
                    }),
                    error: Some("approval_required".to_owned()),
                });
            }
            return self
                .resume_approved_invocation(
                    &context,
                    context.request_id,
                    pending.approval_id,
                    kiana_domain::ApprovalDecision::Approve,
                    pending,
                )
                .await;
        }
        let pending_events = match self.runner.send(RunnerCommand::resume(run_id)).await {
            Ok(events) => events,
            Err(error) => {
                return Ok(resume_unknown_response(
                    &context,
                    run_id,
                    &format!(
                        "runner_resume_failed:{}",
                        redact_event_text(&error.to_string())
                    ),
                ));
            }
        };
        let mut sequence = 1;
        self.drive_run(
            &context,
            run_id,
            &snapshot.sandbox,
            pending_events,
            &mut sequence,
        )
        .await
    }

    /// 一次执行结束后，把它所属的 Cell 放到对应的生命周期状态上。
    ///
    /// 【三个「什么都不做」的出口，先看它们】
    /// ```text
    /// 没有 cell_id            → 这次执行不属于任何 Cell，不用归位
    /// 找不到 reservation      → 没有登记过，不归位（不凭空造一个）
    /// current 已经是终态      → 已经归位过了，重复调用是安全的空操作
    /// ```
    /// 这三个出口都是「幂等」而不是「跳过检查」：恢复流程可能被重放，
    /// 归位也必须能重放。
    ///
    /// 【状态映射里最要紧的一条】
    /// ```text
    /// Completed         → ReadyToMerge
    /// AwaitingApproval  → WaitingInput
    /// ResultUnknown     → Quarantined     ← 不是 Failed
    /// Cancelled         → CancelRequested
    /// Failed/Blocked/Denied → Failed
    /// 其它              → 不动
    /// ```
    ///
    /// ⚠ `ResultUnknown` 映射到 **Quarantined 而不是 Failed**，是本仓库最一致的一个选择。
    ///   「不知道」不等于「失败」：失败的 Cell 可以被重试或丢弃，
    ///   而 Quarantined 的含义是「先别动它，等对账给出答案」。
    ///   如果这里写成 Failed，一个可能已经产生了副作用的 Cell 就会被当成干净的失败品处理掉。
    ///
    /// 【为什么每次都记事件】
    /// 归位不是内存操作，它会写一条 `cell.<状态>` 事件。
    /// 理由和 approvals.rs 里 `request.accepted` 一样：Cell 处于什么状态是一个事实，
    /// 而这个事实必须在事后可查。
    ///
    /// 【输入、输出与失败】
    /// 调用方提供当前上下文、RunId、sandbox、执行结果状态和本地事件序号；成功返回 `Ok(())`。
    /// Cell 查询、状态 CAS、快照写入或事件追加失败会传播为 `CoreError`。除状态改变外，本方法
    /// 在 AwaitingApproval 时会调用 [`Self::checkpoint_run`]；普通终态会退休 Cell。
    pub(crate) async fn settle_resumed_cell(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        status: ExecutionStatus,
        sequence: &mut u64,
    ) -> Result<(), CoreError> {
        let Some(cell_id) = context.cell_id else {
            return Ok(());
        };
        let Some(reservation) = self.cell_registry.reservation_for_cell(cell_id).await? else {
            return Ok(());
        };
        let current = reservation.cell.lifecycle;
        if current.is_terminal() {
            return Ok(());
        }
        let next = match status {
            ExecutionStatus::Completed => CellLifecycle::ReadyToMerge,
            ExecutionStatus::AwaitingApproval => CellLifecycle::WaitingInput,
            ExecutionStatus::ResultUnknown => CellLifecycle::Quarantined,
            ExecutionStatus::Cancelled => CellLifecycle::CancelRequested,
            ExecutionStatus::Failed | ExecutionStatus::Blocked | ExecutionStatus::Denied => {
                CellLifecycle::Failed
            }
            _ => return Ok(()),
        };
        if current != next {
            let cell = self
                .cell_registry
                .transition_cell(cell_id, current, next)
                .await?;
            self.record_event(
                context.request_id,
                sequence,
                &format!("cell.{}", next.as_str()),
                json!({"run_id":run_id,"cell_id":cell_id,"cell":cell}),
            )
            .await?;
        }
        if status == ExecutionStatus::AwaitingApproval {
            self.checkpoint_run(context, run_id, sandbox, sequence)
                .await?;
        } else if status != ExecutionStatus::ResultUnknown {
            // 执行效果未知时保留资源预约，直到 reconciliation（对账/恢复核对）给出结论。
            if next == CellLifecycle::CancelRequested {
                self.cell_registry
                    .transition_cell(cell_id, next, CellLifecycle::Cancelled)
                    .await?;
            }
            let retirement = self
                .cell_registry
                .retire_cell(cell_id, "run_continuation_terminal")
                .await?;
            self.record_event(
                context.request_id,
                sequence,
                "cell.retired",
                json!({"run_id":run_id,"retirement":retirement}),
            )
            .await?;
        }
        Ok(())
    }

    /// 从受控的运行上下文或快照恢复审批所需范围，不采信 UI 直接传入的 scope 字段。
    ///
    /// lifecycle 的 continue/启动相关路径和 [`Self::resume_run`] 会调用本方法。它优先查找同一
    /// session 的内存待续体；Host 重启后则读取最近的 `run.snapshot`，还原冻结的 RequestContext。
    /// actor、role、department、project root/trust 和 permission profile 必须匹配；如果调用方
    /// 显式给出非空 path_allow，也必须与快照相同。通过后从快照补回 path_allow、work_packet_id
    /// 和 cell_id，返回派生后的 context 副本。
    ///
    /// EventStore 读取错误或上下文不匹配返回 `CoreError`；没有可用的原始快照时返回输入 context
    /// 的副本。本方法只读并构造值，不写授权状态；恢复出的范围仍要经过正常的策略/Gate 检查。
    pub(crate) async fn approval_scope(
        &self,
        context: &RequestContext,
    ) -> Result<RequestContext, CoreError> {
        let live = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .find(|pending| pending.context.session_id == context.session_id)
            .map(|pending| pending.context.clone());
        let original = match live {
            Some(original) => Some(original),
            None => self
                .events
                .read_all()
                .await?
                .iter()
                .rev()
                .filter(|event| event.kind == "run.snapshot")
                .filter(|event| event.data["snapshot"]["schema"] == "kiana.run-snapshot.v1")
                // 冻结的授权上下文与可执行的 pending body 分开保存。
                // 旧快照即使把执行材料脱敏，也不能因此隐藏原始路径/Cell/packet 边界。
                .filter_map(|event| {
                    serde_json::from_value::<RequestContext>(
                        event.data["snapshot"]["context"].clone(),
                    )
                    .ok()
                })
                .find(|original| original.session_id == context.session_id),
        };
        let mut derived = context.clone();
        if let Some(original) = original {
            if original.actor_id != context.actor_id
                || original.role_id != context.role_id
                || original.department_id != context.department_id
                || original.project_root != context.project_root
                || original.project_trusted != context.project_trusted
                || original.permission_profile != context.permission_profile
                || (!context.path_allow.is_empty() && context.path_allow != original.path_allow)
            {
                return Err(PortError::Failed("approval_context_mismatch".to_owned()).into());
            }
            derived.path_allow = original.path_allow;
            derived.work_packet_id = original.work_packet_id;
            derived.cell_id = original.cell_id;
        }
        Ok(derived)
    }
}
