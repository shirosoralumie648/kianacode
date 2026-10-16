//! ControlPlane 中新能力请求的授权、审批决策与 Run 续接编排。
//!
//! 这部分位于唯一执行脊柱的 Core 层：命令、连接器、自动化等调用方把
//! [`CapabilityRequest`] 和服务端 [`RequestContext`] 交给这里；Core 再准备动作、计算策略与
//! 关卡（Gate）决策、记录事件，并在许可后进入派发（dispatch）/能力代理（Broker）。CLI、Workbench 等入口不应复制这套
//! 权限判断或另建模型循环。
//!
//! # 在执行链路中的位置
//!
//! 多个 Core 子系统会调用 `authorize_and_execute`，例如 `commands.rs`、`connectors.rs`、
//! `context_query.rs`、`workspace_checkpoints.rs`、`company_business.rs` 和 `automation.rs`。
//! 这些调用方各自负责把领域操作整理为请求；本文件负责共享的策略/审批入口。
//! 它不是每个请求唯一的 Rust 函数入口：已暂停的 Run 在审批后由
//! `resume_approved_invocation` 使用原先保存的调用上下文继续，并最终回到 Core dispatch。
//! 这样既复用同一控制面，也不需要重新伪装成一次全新的请求。
//!
//! # 一次请求在这里经历什么
//!
//! ```text
//! 请求进来
//!   ↓ request_id 必须与上下文一致      ← 否则 request_context_mismatch
//!   ↓ 追加 request.accepted            ← 身份一致性检查通过后、动作准备之前
//!   ↓ prepare_capability_action       ← 准备（可能失败 → capability.blocked）
//!   ↓ 策略 + 关卡                      ← kiana-policy → kiana-gates
//!   ↓ 追加 capability.decision
//!   ├─ Denied            → 直接拒绝，没产生任何 effect
//!   ├─ AwaitingApproval  → **登记一个待批**，返回 challenge，结束
//!   └─ Allowed           → execute_authorized_request  ← 唯一会真正执行的一支
//! ```
//!
//! # 三条值得单独记住的性质
//!
//! 1. **先记事件，后做判断**。被拒绝的请求同样会留下 `request.accepted`。
//!    审计链上能看到的不是「哪些操作成功了」，而是「谁请求过什么」。
//! 2. **拒绝路径的文案先脱敏再写进事件**（`redact_event_text`）——
//!    否则一条错误消息就可能把 secret 带进 EventLog。
//! 3. **决策事件显式声明「尚未产生 effect」**：
//!    `effect_started: false, effect_known: true, zero_effect: true`。
//!    这三个字段让「被拒绝」与「结果未知」在事件层面就是两件不同的事，
//!    而不是靠读的人自己推断。
//!
//! # 审批（Approval）的完整生命周期
//!
//! ## 阶段 1：准备到决策 — `authorize_and_execute`
//! 当一个请求需要人工审批时，`GateDecision::AwaitingApproval` 将它转送给
//! `stage_capability_action`，该函数：
//! - 生成一个 challenge（包含 request_hash、nonce、过期时间）
//! - 创建一个 `ApprovalId` 与之相关联
//! - 返回 challenge 给调用方，让人类审批者验证
//!
//! challenge 中的 hash 与 nonce 绑定了待审批材料；JournalApprovalStore 要求提交决策时同时
//! 提供二者，并检查 challenge 尚未过期。审批状态只能被一次地从 Active 推进到已决状态；
//! 网络重试时相同决策会返回已记录的预览/结果，不会重新执行能力。
//!
//! ## 阶段 2：人类决策 — `decide_approval_with_proof_and_version`
//! 审批者用 `ApprovalDecision::Approve` 或 `::Deny` + challenge 的 hash/nonce 提交决策。
//! 在这一步：
//! - 验证 proof（审批证明材料：request_hash + nonce 与存储的 challenge 一致）
//! - 检查乐观并发控制（optimistic concurrency）版本，避免基于过期审批状态提交新决策
//! - 如果是 Approve，再验证一次「执行材料自决策前后没变」
//! - 如果仍然需要审批（需求改变了），立即拒绝
//! - 如果 Run 已被取消，立即拒绝
//!
//! 同一个 ApprovalId 的重复相同决策可重放已记录结果；不同决策会发生冲突。审批 Store 的
//! aggregate version 是并发比较依据，不应把它误解为事件全局游标。
//!
//! ## 阶段 3a：与运行实例绑定（Run-bound）的执行 — `resume_approved_invocation`
//! 如果这个审批关联了一个 Runner 运行实例（Run），决策完成后：
//! - 清理一遍「中间是否断裂了」的各种标志
//! - 重新 validate turn_id/step_id/run_id 绑定
//! - 再过一遍策略关卡（要求不能再次变化为需要新审批）
//! - 一旦通过，就通知 Runner 继续执行、递进到下一步 Tool 调用
//!
//! 如果中间任何地方失败了，整个 Run 都会被标记为 Failed 或 Cancelled，
//! 并记录为终止事件。
//!
//! ## 阶段 3b：直接（非 Run-bound）执行 — `execute_authorized_request`
//! 如果没有关联 Run，批准后直接执行，结果立即返回给调用方。
//!
//! # 与 dispatch 的关系
//!
//! - `approval` 保存人工决策，并在 Run-bound 情况下恢复此前暂停的执行上下文；
//! - `dispatch` 在策略/关卡再次确认后消费与本次调用匹配的执行许可，并调用 Broker；
//! - 批准本身不等于工具已经运行，更不等于外部效果已被确认。
//!
//! 批准记录完成后仍须经再次授权和 dispatch，随后 Broker 才会调用能力 handler。
//!
//! # 与 EventLog 的交互
//!
//! 审批的每个关键节点都产生事件：
//! - `approval.requested` — 登记一个待批
//! - `approval.approved` / `approval.denied` — 人类决策结果
//! - `approval.continuation_unavailable` — 持久化的审批无法找到运行时续体
//!
//! 这些事件形成了**可审计的证链**。
//!
//! # 并发与竞态问题
//!
//! ## 同一个 approval_id 被并发提交或重试
//! Store 的状态版本和 decision command id 用于区分首次提交、相同请求重放与冲突决策。
//! 已有记录被读取后，Core 用 `replay_approval_decision` 从事件/Run 状态重建响应；若无法
//! 证实最终结果，会返回 `ResultUnknown`，而不是再次启动能力执行。
//!
//! ## 审批进行中 Run 被取消
//! `watch_cancel` 信号被检查多次：
//! - 审批期间就有取消信号 → 在 resume 时注意到，立即拒绝，Run 被停止
//! - 决策完成后才取消 → Run 的执行被中止（通知 Runner），事件记录为 "run.cancelled"
//!
//! ## 审批进行中执行材料变化
//! - 若 policy 需求变化，要求新的审批 → 拒绝原审批，原 Run 停止
//! - 若 request 自己被篡改 → proof mismatch，拒绝
//! - 若 capability_action 被重新 prepare，结果不同 → "approval_action_changed"，拒绝
//!
//! # Error 类型与含义
//!
//! - `request_context_mismatch` — request_id 与上下文不一致
//! - `approval_proof_mismatch` — Core 收到的 hash/nonce 与 challenge 不同；Store 还可能返回
//!   `approval_proof_required`、`approval_request_hash_mismatch` 或 `approval_nonce_mismatch`
//! - `approval_expected_version_conflict` — 乐观并发冲突
//! - `approval_permission_profile_changed` — 权限配置在审批途中变化
//! - `approval_run_already_terminal` — Run 已完成，无法批准续体
//! - `approval_continuation_unavailable` — 持久化记录找不到运行时续体
//! - `approval_action_changed` — 执行动作被篡改或重新准备后不一致
//! - `approval_requirements_changed` — 策略需求在批准时再次变化
//! - `cancelled:before_approval` — 批准前 Run 被取消
//! - `approval_denied` — 人类拒绝了这个审批
use super::events::*;
use super::redaction::*;
use super::*;

impl ControlPlane {
    /// 对一条新提交的能力请求执行准备、策略/Gate 判定和授权后的派发。
    ///
    /// 【调用者】
    /// Core 的命令、连接器、上下文查询、工作区检查点、Company 和自动化处理会在执行具体
    /// 能力前调用它。Run 中等待审批的旧调用不会从这里重新开始，而由
    /// [`Self::resume_approved_invocation`] 验证原上下文后续接。
    ///
    /// 【输入与输出】
    /// - `context`：由服务端建立的请求身份和权限范围；
    /// - `request`：调用方希望执行的领域能力请求，其 `request_id` 必须等于 context 中的 ID。
    /// 成功返回 `CoreResponse`，状态可能是完成、拒绝或等待审批；发生 EventLog、策略端口或
    /// Broker 等基础设施错误时返回 `CoreError`。
    ///
    /// 【副作用与失败】
    /// 身份不匹配时返回 `request_context_mismatch`，且在写入 `request.accepted` 前结束；
    /// 身份匹配后会先记录请求，再准备能力并记录决策。准备失败会脱敏后写入
    /// `capability.blocked`；Denied 不执行；AwaitingApproval 会登记 challenge 后返回；只有
    /// Allowed 才会进入实际 dispatch。后续失败可能是可判定失败，也可能因副作用是否发生无法
    /// 证实时返回 ResultUnknown，最终状态取决于 dispatch/Broker 的回执。
    ///
    /// 【为什么必须经过这一层】
    /// 各领域调用方不能自行把原始请求交给 handler；集中准备、权限判定、事件记录和派发，才能
    /// 让拒绝、待批和执行共用同一套授权边界。
    ///
    /// 【第一个检查为什么是 request_id 的一致性】
    /// 请求自己的 id 必须和上下文里的 id 一样。
    /// 少这一条，一个属于会话 A 的请求可以被挂在会话 B 的上下文里执行——
    /// 而所有后续的权限判定读的都是**上下文**。
    ///
    /// 【为什么先写 `request.accepted` 再判断】
    /// 因为「有人请求过这件事」本身就是一个事实，无论它后来被不被允许。
    /// 只记录成功操作，等于让审计链上看不见所有未遂的尝试——
    /// 而未遂的尝试恰恰是安全上最值得看的东西。
    ///
    /// 【三个分支里只有一个会执行】
    /// `Denied` 直接返回；`AwaitingApproval` 走到 `stage_capability_action`
    /// 登记一个待批就结束——**它不执行，也不代替人做决定**；
    /// 只有 `Allowed` 带着 `authorization_id` 进 `execute_authorized_request`。
    ///
    /// ⚠ 所以「审批通过」这件事本身不等于「已执行」。中间还隔着一次真正的调用，
    ///   而那次调用仍然可能被拒绝、被 fence、或者结果未知。
    pub async fn authorize_and_execute(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = request.request_id;
        if request_id != context.request_id {
            return Ok(CoreResponse::blocked(
                request_id,
                "request_context_mismatch",
            ));
        }
        self.append_event(request_id,1,"request.accepted",json!({"capability":request.capability,"operation":request.operation,"risk":request.risk})).await?;
        let request = match self
            .prepare_capability_action(context, request, None, false)
            .await
        {
            Ok(request) => request,
            Err(error) => {
                let reason = redact_event_text(&error.to_string());
                self.append_event(request_id, 2, "capability.blocked", json!({"error":reason}))
                    .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        let (policy, gate) = self
            .authorize_capability_action(context, &request, None)
            .await?;
        self.append_event(
            request_id,
            2,
            "capability.decision",
            json!({"policy":policy,"gate":gate,
            "action_digest":kiana_domain::capability_action_digest(&request),
            "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
            "stop_state":"not_requested","fenced":false}),
        )
        .await?;
        let authorization_id = match gate {
            GateDecision::Allowed { authorization_id } => authorization_id,
            GateDecision::Denied { reason } => {
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Denied,
                    output: Value::Null,
                    error: Some(reason),
                })
            }
            GateDecision::AwaitingApproval { reason } => {
                let mut sequence = 3;
                let challenge = self
                    .stage_capability_action(
                        context,
                        &request,
                        &reason,
                        None,
                        request_id,
                        None,
                        &mut sequence,
                    )
                    .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::AwaitingApproval,
                    output: json!({"approval":challenge}),
                    error: Some(reason),
                });
            }
        };
        self.execute_authorized_request(context, request, authorization_id, 3)
            .await
    }

    /// 省略 proof 与 expected version 的兼容便捷入口。
    ///
    /// 它把请求转发到完整决策流程，并传入 `None` 作为 request hash、nonce 和 expected
    /// version。调用者无需关心转发细节，但这不意味着审批 Store 一定接受无 proof 决策：当前
    /// daemon 的 `JournalApprovalStore` 要求 hash 与 nonce，缺失时返回
    /// `approval_proof_required`。因此不能用本便捷入口替代带 proof 的用户审批协议。
    ///
    /// 当前 `automation::cancel_workflow_effects` 会用本方法尝试拒绝待批审批。Journal Store
    /// 对活动审批因缺少 proof 会返回 `approval_proof_required`；Core 将这个端口错误包装成
    /// `Ok(CoreResponse { status: Blocked, ... })`，而该调用点目前丢弃了响应内容，所以 `?`
    /// 不会让拒绝失败向上传播。调用者应检查 CoreResponse 的状态和 error，不能只检查外层 Result。
    /// 本方法本身没有独立副作用，副作用和失败都来自 [`Self::decide_approval_with_proof`]。
    pub async fn decide_approval(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<CoreResponse, CoreError> {
        self.decide_approval_with_proof(context, approval_id, decision, None, None)
            .await
    }

    /// 把带审批 proof、但不带版本断言的决定转发给完整审批决策流程。
    ///
    /// 【调用者与输入】
    /// 可供持有 challenge 信息的入口使用，输入为当前 RequestContext、ApprovalId、批准/拒绝
    /// 决策，以及 challenge 返回的 request hash 和 nonce。当前 daemon wire 路径使用更完整的
    /// [`Self::decide_approval_with_proof_and_version`]，以便一并传入客户端观察到的版本号。
    ///
    /// 【输出、失败与副作用】
    /// 返回完整方法产生的 CoreResponse/CoreError。本包装函数不自行校验或写事件；完整流程会
    /// 让 ApprovalStore 验证 proof 和有效期，并可能持久化决策、恢复 Run 或派发能力。省略的
    /// proof 字段会传为 `None`，生产 Store 可因此拒绝请求。
    pub async fn decide_approval_with_proof(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<CoreResponse, CoreError> {
        self.decide_approval_with_proof_and_version(
            context,
            approval_id,
            decision,
            request_hash,
            nonce,
            None,
        )
        .await
    }

    /// 接收带审批 proof 和可选版本断言的批准/拒绝，并编排持久化、Run 续接或直接执行。
    ///
    /// 【调用者】
    /// wire 路径由 `DaemonHost::handle` 的 `RequestBody::ApprovalDecision` 分支调用；Core 内部
    /// 也可由持有完整 challenge 的流程调用。响应是 [`CoreResponse`]，基础设施层失败会传播为
    /// [`CoreError`]。
    ///
    /// 【输入】
    /// `context` 是当前服务端请求身份；`approval_id` 指向待决审批；`decision` 是批准/拒绝；
    /// `request_hash` 与 `nonce` 应来自待审批 challenge；`expected_version` 是调用方读取审批
    /// 聚合时观察到的版本，可用来拒绝基于过期状态提交的首次决定。
    ///
    /// 【核心流程】
    /// 1. 读取并按调用主体限定审批记录；若提供的 proof 与 challenge 不同则阻止；
    /// 2. 已有决定时先重建重放结果，再检查新决定的版本断言；
    /// 3. 未决批准会重取执行材料、重新 prepare 并重跑策略/Gate，且检查 Run 状态和取消信号；
    /// 4. 由 ApprovalStore 以版本比较提交决定；若另一请求先提交，则重放已提交决定；
    /// 5. 记录审批事件；Run-bound 决定恢复原 invocation，非 Run-bound 拒绝则返回，批准则派发。
    ///
    /// 【输出、副作用与失败】
    /// 失败可表现为结构化 Blocked/Denied/Failed/ResultUnknown 响应，也可能以 `CoreError` 返回。
    /// 此流程会读写 ApprovalStore、EventLog、运行时 invocation map；当前函数中的
    /// 恢复 Runner、dispatch 能力并结算 Cell。批准不是执行成功；副作用结果不确定时必须保留
    /// ResultUnknown，而不能把它改写为普通失败后再次执行。
    ///
    /// 【proof 与 replay 的边界】
    /// 此层会拒绝“已提供但不匹配”的 hash/nonce。对于未决状态，当前 JournalApprovalStore 会
    /// 要求二者都存在并校验有效期；但已决记录会在 Store 再次执行 proof 校验之前进入重放分支，
    /// 因而 `None` proof 可能只在满足 read_decision 的主体绑定后得到重放响应。这条路径不应
    /// 再次派发能力。expected version 同样只约束首次提交；相同已决请求优先重放。
    pub async fn decide_approval_with_proof_and_version(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<&str>,
        nonce: Option<&str>,
        expected_version: Option<u64>,
    ) -> Result<CoreResponse, CoreError> {
        // 先按当前上下文读取该审批记录。这里若调用方提供了错误 proof 会立刻阻止；
        // 未决状态下 Store 还会强制要求完整 proof，已决状态则会走下方的只读重放路径。
        let record = self.approvals.read_decision(context, approval_id).await?;
        if request_hash.is_some_and(|hash| hash != record.challenge.request_hash)
            || nonce.is_some_and(|nonce| nonce != record.challenge.nonce)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_proof_mismatch",
            ));
        }

        // 相同审批若已完成状态转换，只重建原决定对应的响应，绝不重新进入执行分支。
        if record.decision.is_some() {
            return self
                .replay_approval_decision(context, &record, decision)
                .await;
        }

        // expected_version 是审批聚合的版本断言，不是 EventLog 全局游标；0 被保留为无效版本。
        if expected_version.is_some_and(|version| version == 0) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_expected_version_invalid",
            ));
        }
        if record.decision.is_none()
            && expected_version.is_some_and(|version| version != record.version)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_expected_version_conflict",
            ));
        }

        // 【第 4 步】如果是 Approve，验证权限配置没变
        let scoped_context = self
            .approvals
            .context_for_pending(context, approval_id)
            .await?;
        if decision == ApprovalDecision::Approve
            && context.permission_profile != scoped_context.permission_profile
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_permission_profile_changed",
            ));
        }
        let context = &scoped_context;

        // 【第 5 步】检查是否有关联的 Run 以及它的状态
        let persisted_approval = self.approval_cursor(approval_id).await?;
        let live_run = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&approval_id)
            .map(|pending| pending.run_id);
        if decision == ApprovalDecision::Approve {
            if let Some(run_id) = live_run {
                if self.run_state(run_id).await?.outcome.is_some() {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "approval_run_already_terminal",
                    ));
                }
            }
        }

        // 【第 6 步】如果是持久化的审批但没有运行时续体，拒绝
        let has_live_invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&approval_id);
        if decision == ApprovalDecision::Approve
            && persisted_approval.is_some()
            && !has_live_invocation
        {
            // 重启后的 Host 可能仍能认证并读取已持久化的审批，但内存中的 Runner 续体已丢失。
            // 在续体尚未恢复前不能消费这条审批，否则以后即使找回 Run 也无法继续执行。
            if let Err(error) = self
                .approvals
                .validate_with_proof(context, approval_id, request_hash, nonce)
                .await
            {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
            let persisted_approval = persisted_approval.expect("checked above");
            if !persisted_approval.continuation_recorded {
                self.append_event(
                    persisted_approval.event_request_id,
                    persisted_approval.event_sequence.saturating_add(1),
                    "approval.continuation_unavailable",
                    json!({
                        "approval_id": approval_id,
                        "run_id": persisted_approval.run_id,
                        "error": "approval_continuation_unavailable",
                    }),
                )
                .await?;
            }
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_continuation_unavailable",
            ));
        }

        // 【第 7 步】如果是 Approve，再验证一遍执行材料没变且不再需要新审批
        if decision == ApprovalDecision::Approve {
            let validated = match self
                .approvals
                .pending_with_proof(context, approval_id, request_hash, nonce)
                .await
            {
                Ok(validated) => validated,
                Err(error) => {
                    if let Some(response) = self
                        .close_live_pending_approval(context, approval_id, &error.to_string())
                        .await?
                    {
                        return Ok(response);
                    }
                    return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
                }
            };
            let prepared = match self
                .prepare_capability_action(
                    context,
                    validated.request.clone(),
                    None,
                    has_live_invocation || persisted_approval.is_some(),
                )
                .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    if let Some(response) = self
                        .close_live_pending_approval(context, approval_id, &error.to_string())
                        .await?
                    {
                        return Ok(response);
                    }
                    return Err(error);
                }
            };
            if prepared != validated.request {
                if let Some(response) = self
                    .close_live_pending_approval(context, approval_id, "approval_action_changed")
                    .await?
                {
                    return Ok(response);
                }
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "approval_action_changed",
                ));
            }
            let (_, gate) = self
                .authorize_capability_action(
                    context,
                    &prepared,
                    Some((approval_id, &validated.challenge.reason)),
                )
                .await?;
            match gate {
                GateDecision::Allowed { .. } => {}
                GateDecision::Denied { reason } => {
                    if let Some(response) = self
                        .close_live_pending_approval(context, approval_id, &reason)
                        .await?
                    {
                        return Ok(response);
                    }
                    return Ok(CoreResponse::blocked(context.request_id, reason));
                }
                GateDecision::AwaitingApproval { .. } => {
                    if let Some(response) = self
                        .close_live_pending_approval(
                            context,
                            approval_id,
                            "approval_requirements_changed",
                        )
                        .await?
                    {
                        return Ok(response);
                    }
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "approval_requirements_changed",
                    ));
                }
            }
            if let Some(run_id) = live_run {
                if *self.watch_cancel(run_id).borrow() {
                    if let Some(response) = self
                        .close_live_pending_approval(
                            context,
                            approval_id,
                            "cancelled:before_approval",
                        )
                        .await?
                    {
                        return Ok(response);
                    }
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "cancelled:before_approval",
                    ));
                }
            }
        }

        // 【第 8 步】提交决策，得到持久化记录
        let pending = match self
            .approvals
            .decide_with_proof_and_version(
                context,
                approval_id,
                decision,
                request_hash,
                nonce,
                expected_version,
            )
            .await
        {
            Ok(pending) => pending,
            Err(error) => {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
        };
        let committed = self.approvals.read_decision(context, approval_id).await?;

        // 【第 9 步】防止并发重放：如果这个决策是别人提交的，返回重放
        if committed.decision_command_id != Some(context.request_id) {
            return self
                .replay_approval_decision(context, &committed, decision)
                .await;
        }

        // 【第 10 步】尝试取出运行时绑定的待续体
        let invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&approval_id);
        if decision == ApprovalDecision::Approve
            && invocation.is_none()
            && (has_live_invocation || persisted_approval.is_some())
        {
            // 并发重放不能把原本绑定 Run 的能力悄悄改成脱离 Run 的直接命令。
            return self
                .replay_approval_decision(context, &committed, decision)
                .await;
        }

        // 【第 11 步】记录决策事件，绑定到 Run 或直接请求
        let request_id = pending.request.request_id;
        let event_request_id = invocation
            .as_ref()
            .map(|pending| pending.event_request_id)
            .unwrap_or(request_id);
        let event_sequence = invocation
            .as_ref()
            .map(|pending| pending.event_sequence)
            .unwrap_or(4);
        let event_kind = match decision {
            ApprovalDecision::Approve => "approval.approved",
            ApprovalDecision::Deny => "approval.denied",
        };
        let mut approval_event = json!({
            "approval_id": approval_id,
            "request_hash": pending.challenge.request_hash,
            "session_id": context.session_id,
            "actor_id": context.actor_id,
            "decision":decision,
            "subject_request_id": pending.request.request_id,
            "operation": pending.request.operation,
            "expires_at_unix_ms":pending.challenge.expires_at_unix_ms,
            "scope": {"project_root":context.project_root,"role_id":context.role_id,"department_id":context.department_id,"paths":context.path_allow},
        });
        if let Some(invocation) = &invocation {
            approval_event["run_id"] = json!(invocation.run_id);
        } else if let Some(cursor) = persisted_approval {
            approval_event["run_id"] = json!(cursor.run_id);
        }
        self.append_event(event_request_id, event_sequence, event_kind, approval_event)
            .await?;

        // 【第 12 步】根据绑定情况走相应的执行路径
        if let Some(invocation) = invocation {
            return self
                .resume_approved_invocation(context, request_id, approval_id, decision, invocation)
                .await;
        }

        // 【第 13 步】如果是拒绝，立即终止（Run 或直接请求）
        if decision == ApprovalDecision::Deny {
            if let Some(cursor) = persisted_approval {
                let mut sequence = event_sequence + 1;
                self.record_terminal_event(
                    event_request_id,
                    &mut sequence,
                    cursor.run_id,
                    "run.failed",
                    json!({"run_id":cursor.run_id,"error":"approval_denied"}),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: json!({"run_id":cursor.run_id,"approval_id":approval_id,"decision":"deny"}),
                    error: Some("approval_denied".to_owned()),
                });
            }
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Denied,
                output: json!({ "approval_id": approval_id }),
                error: Some("approval_denied".to_owned()),
            });
        }

        // 【第 14 步】审批通过且没有 Run 绑定，直接执行能力请求
        if let Err(error) = self
            .guard_company_capability(context, &pending.request)
            .await
        {
            self.append_event(
                event_request_id,
                event_sequence + 1,
                "capability.blocked",
                json!({"error":error.to_string(),"attempt":1,"effect_started":false,
                    "effect_known":true,"zero_effect":true,"stop_state":"not_requested","fenced":false}),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, error.to_string()));
        }
        self.execute_authorized_request(
            context,
            pending.request,
            format!("approval:{approval_id}"),
            5,
        )
        .await
    }

    /// 关闭一个活跃的 Run-bound 续体，当审批证明/材料/权限在决策前发生变化时。
    ///
    /// 【为什么需要 close_live_pending_approval】
    /// 当 Run 的审批有运行时续体（invocation 在内存中等待），但中间发现了问题
    /// （材料被篡改、需求升级、权限变化），我们必须：
    /// 1. 把 Run 停止（立即发 Cancel 给 Runner）
    /// 2. 记录拒绝事件
    /// 3. 标记 Run 为 Failed 或 Cancelled
    ///
    /// 找不到内存续体时返回 `None`，让调用者按普通拒绝响应处理；找到时返回清理后的
    /// CoreResponse。此方法先从内存 map 移除续体，再尝试使 Store 中的审批失效，并调用
    /// [`Self::finish_rejected_approval`] 终结 Run。
    ///
    /// ⚠ 当前实现忽略 `ApprovalStore::invalidate` 的错误；如果持久化失效失败，Run 续体仍已
    /// 被移除并会继续记录终止清理结果，但 Store 中审批状态是否已撤销无法由此函数确认。
    async fn close_live_pending_approval(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        reason: &str,
    ) -> Result<Option<CoreResponse>, CoreError> {
        let invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&approval_id);
        let Some(invocation) = invocation else {
            return Ok(None);
        };
        let _ = self
            .approvals
            .invalidate(context, approval_id, reason)
            .await;
        let mut continuation = context.clone();
        continuation.request_id = invocation.event_request_id;
        let mut sequence = invocation.event_sequence.saturating_add(1);
        let status = if reason.starts_with("cancelled:") {
            ExecutionStatus::Cancelled
        } else {
            ExecutionStatus::Failed
        };
        self.finish_rejected_approval(
            &continuation,
            context.request_id,
            approval_id,
            &invocation,
            reason,
            status,
            &mut sequence,
        )
        .await
        .map(Some)
    }

    /// 重放一个已经决策过的审批。
    ///
    /// 【为什么需要重放】
    /// 网络可能失败、客户端可能重试。如果同一个审批 ID 被提交两次，我们不能
    /// 再执行两次——而是根据已经提交的记录重建响应。这是避免重复派发的幂等边界。
    ///
    /// 重建出来的状态不一定表示 Run 仍在执行：Run 可能已完成、被取消，或仍等待审批；
    /// 若 EventLog 无法证明终态，则返回 ResultUnknown。此处查询记录而不重新派发能力。
    ///
    /// 调用者是 [`Self::decide_approval_with_proof_and_version`]；输入包括当前上下文、已读出的
    /// 决策记录和本次决定。相反决定会返回 `approval_decision_conflict`，所有者不匹配会拒绝，
    /// 日志/状态读取失败会传播为 `CoreError`。正常结果为 [`CoreResponse`]，本方法只读状态，
    /// 不消费审批、不写新事件，也不发起 Runner/Broker 调用。
    async fn replay_approval_decision(
        &self,
        context: &RequestContext,
        record: &kiana_domain::ApprovalDecisionRecord,
        decision: ApprovalDecision,
    ) -> Result<CoreResponse, CoreError> {
        let approval_id = record.approval_id;
        if record.decision != Some(decision) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_decision_conflict",
            ));
        }
        if decision == ApprovalDecision::Deny {
            return Ok(CoreResponse {
                request_id: context.request_id,
                status: ExecutionStatus::Denied,
                output: json!({"approval_id":approval_id,"replayed":true,"decision":"deny"}),
                error: Some("approval_denied".to_owned()),
            });
        }

        // 【查找 Run 绑定】
        let events = self.read_all_events().await?.unwrap_or_default();
        let run_id = events
            .iter()
            .find(|event| {
                event.kind == "approval.requested"
                    && event.data["approval_id"] == json!(approval_id)
            })
            .and_then(|event| event.data["run_id"].as_str())
            .and_then(RunId::parse_str);
        if let Some(run_id) = run_id {
            // 【如果有 Run，检查 Run 的最终状态】
            let owned = crate::receipts::filter_run_events(&events, run_id);
            if crate::receipts::receipt_owner_mismatch(&owned, context) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "run_owner_mismatch",
                ));
            }
            let state = self.run_state(run_id).await?;
            let status = match state.outcome {
                Some(RunOutcome::Completed) => ExecutionStatus::Completed,
                Some(RunOutcome::Failed) => ExecutionStatus::Failed,
                Some(RunOutcome::Cancelled) => ExecutionStatus::Cancelled,
                Some(RunOutcome::ResultUnknown) => ExecutionStatus::ResultUnknown,
                None if state.phase == RunPhase::AwaitingApproval => {
                    ExecutionStatus::AwaitingApproval
                }
                None => ExecutionStatus::ResultUnknown,
            };
            return Ok(CoreResponse {
                request_id: context.request_id,
                status,
                output: json!({"approval_id":approval_id,"run_id":run_id,"replayed":true,"state":record.state}),
                error: state.error.or_else(|| {
                    (status == ExecutionStatus::ResultUnknown)
                        .then(|| "result_unknown:approval_run_not_terminal".to_owned())
                }),
            });
        }

        // 【如果没有 Run 绑定，查找直接执行的结果】
        if let Some(result) = events
            .iter()
            .rev()
            .filter(|event| {
                event.kind == "execution.result_committed"
                    && event.data["capability_request_id"] == json!(record.challenge.request_id)
            })
            .find_map(|event| {
                serde_json::from_value::<CapabilityResult>(event.data["result"].clone()).ok()
            })
        {
            let result =
                kiana_domain::normalize_capability_result(record.challenge.request_id, result);
            let status = match result.failure_code() {
                None => ExecutionStatus::Completed,
                Some(kiana_domain::CapabilityErrorCode::Cancelled) => ExecutionStatus::Cancelled,
                Some(
                    kiana_domain::CapabilityErrorCode::ResultUnknown
                    | kiana_domain::CapabilityErrorCode::CompensationRequired,
                ) => ExecutionStatus::ResultUnknown,
                _ => ExecutionStatus::Failed,
            };
            return Ok(CoreResponse {
                request_id: context.request_id,
                status,
                error: (!result.success).then(|| {
                    result.output["error"]
                        .as_str()
                        .unwrap_or("capability_failed")
                        .to_owned()
                }),
                output: json!({"approval_id":approval_id,"replayed":true,"result":redact_capability_result(result)}),
            });
        }

        // 【都找不到，返回结果未知】
        return Ok(CoreResponse {
            request_id: context.request_id,
            status: ExecutionStatus::ResultUnknown,
            output: json!({"approval_id":approval_id,"replayed":true,"state":record.state,"dispatch_command_id":record.dispatch_command_id}),
            error: Some("result_unknown:approval_dispatch_reconciliation_required".to_owned()),
        });
    }

    /// 恢复一个已批准的 Run-bound 待续体。
    ///
    /// 【调用者与输入】
    /// 由审批决策流程在批准记录提交后调用。`invocation` 是此前从内存 map 取出的待续体，
    /// 保存原 Run、能力请求、challenge、sandbox 和可选 turn/step 绑定；`decision_context` 是
    /// 当前审批请求的服务端身份上下文，`request_id` 是本次响应使用的请求 ID。
    ///
    /// 【流程】
    /// 1. 如果是拒绝，停止 Run，返回 Failed
    /// 2. 验证 Approval 绑定没变（approval_id、request_id 都要对得上）
    /// 3. 重新注入 run_id/turn_id/step_id 到请求中
    /// 4. 重新 prepare，确保没变化（否则 approval_action_changed）
    /// 5. 重新授权，确保不需要新审批（否则 approval_requirements_changed）
    /// 6. 检查 Run 没被取消（被取消就停止）
    /// 7. dispatch 能力请求
    /// 8. 如果 dispatch 返回失败/未知/被拒绝，停止 Run
    /// 9. deliver 结果给 harness，继续执行下一步
    ///
    /// 【输出、副作用与失败】
    /// 成功时返回继续驱动后的 Run 响应；绑定不符、重新授权拒绝或取消时终结 Run；dispatch
    /// 或结果交付无法确认时可能返回 ResultUnknown。此流程会写事件、派发能力、向 Runner 交付
    /// 结果并结算 Cell。底层不可恢复错误传播为 `CoreError`。这里重验的是“恢复执行”所需的
    /// 绑定和当前策略；审批决定提交前的 proof/版本检查由调用它的决策流程负责。
    pub(crate) async fn resume_approved_invocation(
        &self,
        decision_context: &RequestContext,
        request_id: RequestId,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        invocation: PendingInvocation,
    ) -> Result<CoreResponse, CoreError> {
        let mut continuation_context = decision_context.clone();
        continuation_context.request_id = invocation.event_request_id;
        let mut sequence = invocation.event_sequence + 1;

        // 【第 1 步】如果是拒绝，立即停止 Run
        if decision == ApprovalDecision::Deny {
            return self
                .finish_rejected_approval(
                    &continuation_context,
                    request_id,
                    approval_id,
                    &invocation,
                    "approval_denied",
                    ExecutionStatus::Failed,
                    &mut sequence,
                )
                .await;
        }

        // 【第 2 步】验证 Approval 绑定
        if invocation.approval_id != approval_id
            || invocation.challenge.approval_id != approval_id
            || invocation.challenge.request_id != invocation.request_id
            || invocation.request_id != invocation.request.request_id
        {
            return self
                .finish_rejected_approval(
                    &continuation_context,
                    request_id,
                    approval_id,
                    &invocation,
                    "approval_binding_changed",
                    ExecutionStatus::Failed,
                    &mut sequence,
                )
                .await;
        }

        // 【第 3 步】准备请求，重新注入 run_id/turn_id/step_id
        let mut pending_request = invocation.request.clone();
        let (turn_id, step_id) = if let Some(binding) = invocation.resume_binding.as_ref() {
            if let Err(error) = binding.validate_against(
                invocation.run_id,
                invocation.event_request_id,
                &pending_request,
                decision_context,
                &invocation.sandbox,
            ) {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        &error,
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await;
            }
            (binding.turn_id, binding.step_id)
        } else {
            let turn_id = pending_request
                .arguments
                .get("turn_id")
                .and_then(|value| {
                    serde_json::from_value::<kiana_domain::TurnId>(value.clone()).ok()
                })
                .or_else(|| {
                    pending_request
                        .execution_scope
                        .as_ref()
                        .and_then(|scope| scope.turn_id)
                })
                .or_else(|| {
                    Some(kiana_domain::TurnId::from_uuid(
                        invocation.event_request_id.as_uuid(),
                    ))
                });
            let step_id = pending_request.arguments.get("step_id").and_then(|value| {
                serde_json::from_value::<kiana_domain::StepId>(value.clone()).ok()
            });
            (turn_id, step_id)
        };
        if let Some(arguments) = pending_request.arguments.as_object_mut() {
            arguments.insert("run_id".to_owned(), json!(invocation.run_id));
            if let Some(turn_id) = turn_id {
                arguments.insert("turn_id".to_owned(), json!(turn_id));
            }
            if let Some(step_id) = step_id {
                arguments.insert("step_id".to_owned(), json!(step_id));
            }
        }

        // 【第 4 步】重新 prepare，确保没变化
        let request = match self
            .prepare_capability_action(
                decision_context,
                pending_request.clone(),
                Some(&invocation.sandbox),
                true,
            )
            .await
        {
            Ok(request) if request == pending_request => request,
            Ok(_) => {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        "approval_action_changed",
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await
            }
            Err(error) => {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        &redact_event_text(&error.to_string()),
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await;
            }
        };

        // 【第 5 步】检查 Run 没被取消
        let cancellation = self.watch_cancel(invocation.run_id);
        if *cancellation.borrow() {
            return self
                .finish_rejected_approval(
                    &continuation_context,
                    request_id,
                    approval_id,
                    &invocation,
                    "cancelled:before_dispatch",
                    ExecutionStatus::Cancelled,
                    &mut sequence,
                )
                .await;
        }

        // 【第 6 步】重新授权，确保不需要新审批
        let (policy, gate) = self
            .authorize_capability_action(
                decision_context,
                &request,
                Some((approval_id, &invocation.challenge.reason)),
            )
            .await?;
        self.record_event(
            invocation.event_request_id,
            &mut sequence,
            "capability.decision",
            json!({"run_id":invocation.run_id,"capability_request_id":request.request_id,"policy":policy,"gate":gate,
                "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                "stop_state":"not_requested","fenced":false}),
        )
        .await?;
        let authorization_id = match gate {
            GateDecision::Allowed { authorization_id } => authorization_id,
            GateDecision::Denied { reason } => {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        &reason,
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await;
            }
            GateDecision::AwaitingApproval { .. } => {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        "approval_requirements_changed",
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await;
            }
        };

        // 【第 7 步】如果请求涉及 Cell，转移 Cell 状态
        if let Some(cell_id) = request.cell_id {
            let current = self
                .cell_registry
                .reservation_for_cell(cell_id)
                .await?
                .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
            if current.cell.lifecycle == CellLifecycle::WaitingInput {
                self.cell_registry
                    .transition_cell(cell_id, CellLifecycle::WaitingInput, CellLifecycle::Running)
                    .await?;
            }
        }

        // 【第 8 步】dispatch 能力请求
        let finalized = self
            .dispatch_capability_action(
                decision_context,
                Some(invocation.run_id),
                &request,
                authorization_id,
                cancellation,
                invocation.event_request_id,
                &mut sequence,
            )
            .await?;

        // 【第 9 步】如果 dispatch 返回失败/未知/被拒绝，停止 Run
        if matches!(
            finalized.status,
            ExecutionStatus::Cancelled | ExecutionStatus::ResultUnknown
        ) || finalized.result.output["dispatch_rejected"] == true
        {
            let reason = finalized
                .error
                .unwrap_or_else(|| "result_unknown:approved_invocation_unconfirmed".to_owned());
            let _ = self
                .runner
                .send(RunnerCommand::Cancel {
                    run_id: invocation.run_id,
                    reason: reason.clone(),
                })
                .await;
            self.record_terminal_event(
                invocation.event_request_id,
                &mut sequence,
                invocation.run_id,
                if finalized.status == ExecutionStatus::Cancelled {
                    "run.cancelled"
                } else if finalized.status == ExecutionStatus::ResultUnknown {
                    "run.result_unknown"
                } else {
                    "run.failed"
                },
                json!({"run_id":invocation.run_id,"error":reason}),
            )
            .await?;
            self.settle_resumed_cell(
                &continuation_context,
                invocation.run_id,
                &invocation.sandbox,
                finalized.status,
                &mut sequence,
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: finalized.status,
                output: run_identity(decision_context, invocation.run_id, &invocation.sandbox),
                error: Some(reason),
            });
        }

        // 【第 10 步】开启终止作用域（确保 Run 结束时正确清理）
        let _terminal_scope = self.begin_terminal_scope(invocation.run_id);

        // 【第 11 步】deliver 结果给 harness，继续执行下一步
        let events = match self
            .deliver_capability_result(invocation.run_id, finalized.result)
            .await
        {
            Ok(events) => events,
            Err(error) => {
                let error = redact_event_text(&error.to_string());
                let status = match kiana_domain::CapabilityErrorCode::from_reason(&error) {
                    kiana_domain::CapabilityErrorCode::Cancelled => ExecutionStatus::Cancelled,
                    _ => ExecutionStatus::ResultUnknown,
                };
                self.record_terminal_event(
                    invocation.event_request_id,
                    &mut sequence,
                    invocation.run_id,
                    if status == ExecutionStatus::Cancelled {
                        "run.cancelled"
                    } else {
                        "run.result_unknown"
                    },
                    json!({"run_id":invocation.run_id,"error":error}),
                )
                .await?;
                self.settle_resumed_cell(
                    &continuation_context,
                    invocation.run_id,
                    &invocation.sandbox,
                    status,
                    &mut sequence,
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status,
                    output: run_identity(decision_context, invocation.run_id, &invocation.sandbox),
                    error: Some(error),
                });
            }
        };

        // 【第 12 步】驱动 Run 执行下一步
        let response = self
            .drive_run(
                &continuation_context,
                invocation.run_id,
                &invocation.sandbox,
                events,
                &mut sequence,
            )
            .await?;
        self.settle_resumed_cell(
            &continuation_context,
            invocation.run_id,
            &invocation.sandbox,
            response.status,
            &mut sequence,
        )
        .await?;
        Ok(response)
    }

    /// 结束一个因审批相关检查失败而不能继续的 Run-bound invocation。
    ///
    /// 【何时调用】
    /// 当一个 Run-bound 的待批审批因为某个检查失败而被拒绝时，我们需要：
    /// 1. 取消 Run 内所有待执行的能力
    /// 2. 记录最终的失败/取消事件
    /// 3. 清理 Cell 预约
    ///
    /// 【输出与失败】
    /// 正常清理后返回调用者要求的 Failed/Cancelled 状态；若取消待处理工具、写终态事件或
    /// 结算 Cell 中任一步骤报错，则返回 ResultUnknown，因为 Run 的完整清理事实尚未确认。
    /// 错误文本在记录前经过脱敏，避免路径、环境变量等内容进入事件或响应。
    async fn finish_rejected_approval(
        &self,
        context: &RequestContext,
        request_id: RequestId,
        approval_id: ApprovalId,
        invocation: &PendingInvocation,
        reason: &str,
        status: ExecutionStatus,
        sequence: &mut u64,
    ) -> Result<CoreResponse, CoreError> {
        let reason = redact_event_text(reason);
        let tools_recorded = self
            .cancel_pending_tools(
                invocation.run_id,
                context.request_id,
                sequence,
                Some(&invocation.request),
                &reason,
            )
            .await;
        let recorded = self.record_terminal_event(context.request_id, sequence, invocation.run_id,
            if status == ExecutionStatus::Cancelled { "run.cancelled" } else { "run.failed" },
            json!({"run_id":invocation.run_id,"approval_id":approval_id,"error":reason,"not_executed":true})).await;
        let settled = self
            .settle_resumed_cell(
                context,
                invocation.run_id,
                &invocation.sandbox,
                status,
                sequence,
            )
            .await;
        if tools_recorded.is_err() || recorded.is_err() || settled.is_err() {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: json!({"approval_id":approval_id,"run_id":invocation.run_id,"not_executed":true}),
                error: Some("result_unknown:approval_rejection_cleanup_failed".to_owned()),
            });
        }
        Ok(CoreResponse {
            request_id,
            status,
            output: json!({"approval_id":approval_id,"run_id":invocation.run_id,"not_executed":true}),
            error: Some(reason),
        })
    }

    /// 执行一个已授权但没有 Run 绑定的能力请求。
    ///
    /// 【调用者与输入】
    /// 由普通授权通过路径或非 Run-bound 审批通过路径调用。输入是服务端 RequestContext、
    /// 已准备的请求、Core 签发的 authorization id，以及接续事件写入使用的序号。
    ///
    /// 【输出与副作用】
    /// 通过 `dispatch_capability_action` 创建/消费执行许可并调用 Broker，返回 Broker 结果对应
    /// 的 CoreResponse；EventLog、Store 或 Broker 的底层错误会传播为 CoreError。此处创建的
    /// cancellation channel 初始为未取消，且不代表一个 KianaHarness Run；因此这条直接路径不
    /// 承担 Run 生命周期、续体恢复或 Runner 取消管理。
    pub(crate) async fn execute_authorized_request(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        authorization_id: String,
        result_sequence: u64,
    ) -> Result<CoreResponse, CoreError> {
        let (_cancellation_sender, cancellation) = watch::channel(false);
        let mut sequence = result_sequence;
        let finalized = self
            .dispatch_capability_action(
                context,
                None,
                &request,
                authorization_id,
                cancellation,
                request.request_id,
                &mut sequence,
            )
            .await?;
        Ok(CoreResponse {
            request_id: request.request_id,
            status: finalized.status,
            output: finalized.result.output,
            error: finalized.error,
        })
    }

    /// 同时运行固定安全策略和部署配置策略，产出统一的策略结论。
    ///
    /// 【调用者、输入与输出】
    /// 由授权流程在 Gate 判定前调用，输入为服务端上下文和已准备的能力请求；返回
    /// `PolicyDecision`，自身不读写 EventLog、ApprovalStore 或 Broker。
    ///
    /// 【为什么要有固定策略】
    /// 策略引擎是可配置的（由部署者决定）。但有些检查是根本的、
    /// 不能由策略配置关掉的——比如「风险等级声明错了」、「参数类型不对」。
    ///
    /// 【流程】
    /// 1. 应用固定策略（kiana-policy::DefaultPolicyEngine）
    /// 2. 如果固定策略拒绝了，直接返回拒绝
    /// 3. 如果固定策略没拒绝，应用可配置的策略
    /// 4. 合并两个结果：任何一个说需要审批，就需要审批
    pub(crate) fn evaluate_policy(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> PolicyDecision {
        let fixed = kiana_policy::DefaultPolicyEngine.evaluate(context, request);
        if matches!(fixed, PolicyDecision::Deny { .. }) {
            return fixed;
        }
        let configured = self.policy.evaluate(context, request);
        if matches!(&configured, PolicyDecision::Allow { authorization_id } if authorization_id.trim().is_empty())
        {
            return PolicyDecision::Deny {
                reason: "authorization_id_required".to_owned(),
            };
        }
        match (fixed, configured) {
            (_, PolicyDecision::Deny { reason }) => PolicyDecision::Deny { reason },
            (PolicyDecision::Ask { reason: left }, PolicyDecision::Ask { reason: right }) => {
                PolicyDecision::Ask {
                    reason: super::capabilities::merge_requirements(&left, &right),
                }
            }
            (PolicyDecision::Ask { reason }, _) | (_, PolicyDecision::Ask { reason }) => {
                PolicyDecision::Ask { reason }
            }
            _ => PolicyDecision::Allow {
                authorization_id: format!("policy:{}", request.request_id),
            },
        }
    }

    /// 把策略结论与当前风险/关卡状态合并为最终 Gate 决策。
    ///
    /// 【调用者、输入与输出】
    /// 由能力授权流程在 `evaluate_policy` 之后调用，输入是请求和策略结论，输出是 Allowed、
    /// Denied 或 AwaitingApproval。它不派发请求；真正执行留给通过授权后的 dispatch 路径。
    ///
    /// 【关卡与策略的区别】
    /// - 策略说「**应该**允许吗」（基于 context、request、risk）
    /// - 关卡说「**现在能**允许吗」（基于系统状态、配额、容量）
    ///
    /// 所以即使策略允许了，关卡仍然可以说「需要审批」或「拒绝」。
    pub(crate) fn evaluate_gate(
        &self,
        request: &CapabilityRequest,
        policy: &PolicyDecision,
    ) -> GateDecision {
        if let Some(reason) = capability_risk_violation(request) {
            return GateDecision::Denied {
                reason: reason.to_owned(),
            };
        }
        if let PolicyDecision::Deny { reason } = policy {
            return GateDecision::Denied {
                reason: reason.clone(),
            };
        }
        let contribution = self.gates.evaluate(policy);
        match (policy, contribution) {
            (_, GateDecision::Denied { reason }) => GateDecision::Denied { reason },
            (
                PolicyDecision::Ask { reason: left },
                GateDecision::AwaitingApproval { reason: right },
            ) => GateDecision::AwaitingApproval {
                reason: super::capabilities::merge_requirements(left, &right),
            },
            (PolicyDecision::Ask { reason }, _) => GateDecision::AwaitingApproval {
                reason: reason.clone(),
            },
            (_, GateDecision::AwaitingApproval { reason }) => {
                GateDecision::AwaitingApproval { reason }
            }
            (
                PolicyDecision::Allow {
                    authorization_id: expected,
                },
                GateDecision::Allowed { authorization_id },
            ) if !expected.trim().is_empty() && expected == &authorization_id => {
                GateDecision::Allowed {
                    authorization_id: expected.clone(),
                }
            }
            _ => GateDecision::Denied {
                reason: "gate_authorization_changed".to_owned(),
            },
        }
    }

    /// 从已记录事件中查找审批对应的 Run 游标。
    ///
    /// 【调用者与数据流】
    /// 审批决策和重放路径使用此查询，判断 Approval 是否原本绑定了一个 Run。Host 重启后，
    /// `PendingInvocation` 运行时续体可能已不在内存，但 `approval.requested` 事件仍可提供
    /// RunId、原事件 request_id 和 sequence；这些字段组成后续查询/恢复终态所需的游标。
    ///
    /// 【输出与边界】
    /// 找到关联事件时返回 [`PersistedApprovalCursor`]；EventStore 读取错误传播为 `CoreError`。
    /// 返回 `None` 既可能表示读取接口不支持 `read_all`，也可能表示没有匹配的 Run 绑定，
    /// 所以 `None` 不能单独证明持久化审批从未关联 Run。
    pub(crate) async fn approval_cursor(
        &self,
        approval_id: ApprovalId,
    ) -> Result<Option<PersistedApprovalCursor>, CoreError> {
        let Some(events) = self.read_all_events().await? else {
            // 旧 adapter 可能只实现 read_request。此时 None 表示查询能力有限，
            // 并不能证明持久化审批没有 Run 关联。
            return Ok(None);
        };
        let approval_id = approval_id.to_string();
        Ok(events.iter().rev().find_map(|event| {
            if event.kind != "approval.requested"
                || event.data.get("approval_id").and_then(Value::as_str)
                    != Some(approval_id.as_str())
            {
                return None;
            }
            let run_id = event
                .data
                .get("run_id")
                .and_then(Value::as_str)
                .and_then(RunId::parse_str)?;
            Some(PersistedApprovalCursor {
                run_id,
                event_request_id: event.request_id,
                event_sequence: event.sequence,
                continuation_recorded: events.iter().any(|candidate| {
                    candidate.kind == "approval.continuation_unavailable"
                        && candidate.data.get("approval_id").and_then(Value::as_str)
                            == Some(approval_id.as_str())
                }),
            })
        }))
    }
}
