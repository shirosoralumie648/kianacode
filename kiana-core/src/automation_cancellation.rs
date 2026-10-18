//! # 文件职责
//!
//! 本文件是 AUT-17 取消编排的 Core 边界：
//!
//! * `validate_automation_cancel()` 是历史只读门面，把公开校验转交给
//!   `kiana-domain::validate_automation_cancellation()`；
//! * `cancel_automation_worker_command()` 是**协调器**：重取已提交命令与已记录取消事实，要求注入的
//!   stop 控制器报告停止结果，并把结果折叠成 `NotExecuted` / `Stopped` / `RecoveryRequired`；
//! * `record_automation_late_result()` 只允许把迟到结果**隔离**成 `Fenced`，绝不复活已取消的执行。
//!
//! 真正的字段、状态矩阵、代次/epoch 围栏与状态转移白名单在 Domain；这里只负责端口顺序、CAS 期望值
//! 与“绝不伪造成功”的收敛。
//!
//! # 输入、处理与输出
//!
//! ```text
//! AutomationDispatchStore::fetch_command        // 权威命令，不信任调用方快照
//!   -> AutomationCancellationStore::fetch_cancellation // 当前取消事实（可为空）
//!   -> Domain begin_automation_cancellation()    // 下一步事实 + 是否需要 stop
//!   -> AutomationCancellationStore::commit_cancellation // 先落库，再请求 stop
//!   -> AutomationWorkerStopController::request_stop     // 恰好一次
//!   -> Domain fold_automation_stop_report()      // Stopped / NotExecuted / 未确认
//!   -> AutomationCancellationStore::commit_cancellation // CAS 折叠结果
//! ```
//!
//! 【本文件实际检查什么】取消只对已提交命令的 execution/attempt/authority epoch 生效：queued
//! （`Prepared`）项直接判为 `NotExecuted` 且不请求 stop；已被 worker 取走的 attempt 必须经注入的
//! stop 控制器拿到报告；只有 `Stopped` 报告才会得到 `Stopped`，其余一律 `RecoveryRequired`。终态
//! `Observed` 执行拒绝取消，终态 `Unknown` 直接进入恢复。它不检查当前授权、预算或审批，因此不能
//! 证明调用方有权取消。
//!
//! 【副作用和调用边界】本函数据只调用注入的端口（命令 store、取消 store、stop 控制器）。它不写
//! EventLog、不调用 Broker/模型、不重试 effect、不递增任何持久 generation；真实 capability 执行
//! 必须继续经过 ControlPlane/Broker 脊柱。`Ok(Stopped)` 只表示一份已确认的停止报告被记录，
//! 不代表外部现实已经被对账。

use kiana_domain::{
    begin_automation_cancellation, fence_automation_late_result, fold_automation_stop_report,
    validate_automation_cancellation, AutomationCancelRequest, AutomationCancelState,
    AutomationCancellationCommit, AutomationCancellationFact, AutomationLateResult,
};
use kiana_ports::{
    AutomationCancellationStore, AutomationDispatchStore, AutomationWorkerStopController, PortError,
};

/// 将单条 cancellation fact 原样交给 Domain 校验器。
///
/// 有效时只返回 `Ok(())`，无效时返回 `automation_cancellation_fact_invalid`；没有其他检查或副作用，
/// 不会据此停止 execution，也不会阻止迟到结果写入。需要真正编排取消时使用
/// [`cancel_automation_worker_command`]。
pub fn validate_automation_cancel(fact: &AutomationCancellationFact) -> Result<(), &'static str> {
    validate_automation_cancellation(fact)
}

/// 一次取消编排的最终去向；四个变体都不把“尝试过 stop”伪装成已确认取消。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationCancellationOutcome {
    /// queued 项（或报告证明 effect 从未开始）：没有任何 effect，也没有请求 stop。
    ///
    /// 取消事实处于 `NotExecuted`，这是卡片定义的“未启动项 `not_executed`”路径。
    NotExecuted(AutomationCancellationFact),
    /// stop 已确认并带证据：事实处于 `Stopped`（`stop_confirmed = true` + 报告证据摘要）。
    ///
    /// 只有注入的 stop 控制器给出 `Stopped` 报告时才可能返回，因此“stop 未确认却标 cancelled”
    /// 不会经由此路径发生。
    Stopped(AutomationCancellationFact),
    /// 需要恢复/对账：stop 结果未知、报告与请求绑定不符、结果提交不确定，或命令本身就是 Unknown。
    ///
    /// 事实可能处于 `Requested`/`Stopping`/`ResultUnknown`/`Fenced`；调用方必须按
    /// `(execution_id, attempt)` 重取事实对账，不能当成成功，也不能假设 effect 没有发生。
    RecoveryRequired(AutomationCancellationFact),
    /// 同代次同一请求已记录过，且没有新的 stop 需要发出；本次没有写事实、没有请求 stop。
    Replayed(AutomationCancellationFact),
}

/// 重取事实、取得/记录取消代次、请求一次 stop，并把停止结果折叠进取消事实。
///
/// 【调用者】`kiana-daemon` 的 cancellation service 或任何受控组合根；每一步都通过端口进入，
/// 本函数自身不访问 EventStore、Broker、模型或文件系统。
///
/// 【核心流程】
/// 1. `store.fetch_command(command_id)` 取权威命令，调用方不提供自己的快照；
/// 2. `cancellations.fetch_cancellation(execution_id, attempt)` 取已记录事实（可为空）；
/// 3. Domain `begin_automation_cancellation()` 判定：`Prepared` → `NotExecuted`（不请求 stop），
///    终态 `Observed` → 拒绝，终态 `Unknown` → `ResultUnknown`（不请求 stop），`Claimed` →
///    `Requested` 且必须请求 stop；同代次同请求重放在事实仍是 `Requested` 时只重发 stop；
/// 4. 新事实先用 CAS 落库（`expected_digest` 为当前事实摘要，首次为 `None`）；
/// 5. 需要 stop 时调用 `stop.request_stop()` **恰好一次**；控制器报错、报告绑定漂移或折叠结果
///    提交不确定都返回 `RecoveryRequired`，绝不返回 `Stopped`；
/// 6. 折叠结果用 CAS 提交（期望值是第 4 步写入的事实摘要）。
///
/// 【失败】命令不存在、代次陈旧、epoch 与命令不符、execution/attempt 漂移、终态拒绝、CAS 冲突等
/// 都以稳定的 `PortError` 返回；这些路径都不会触碰 stop 控制器，因此“拒绝”意味着 0 次 stop。
/// 【副作用】只调用注入的三个端口；不写 EventLog、不调用 Broker/模型、不改变授权。
pub async fn cancel_automation_worker_command<S, C, T>(
    store: &S,
    cancellations: &C,
    stop: &T,
    command_id: &str,
    request: &AutomationCancelRequest,
) -> Result<AutomationCancellationOutcome, PortError>
where
    S: AutomationDispatchStore + ?Sized,
    C: AutomationCancellationStore + ?Sized,
    T: AutomationWorkerStopController + ?Sized,
{
    // 1. 权威命令；调用方快照一律不采信。
    let command = store.fetch_command(command_id).await?;
    // 2. 当前取消事实；没有记录时 first-generation 取消。
    let current = cancellations
        .fetch_cancellation(&command.intent.execution_id, command.intent.attempt)
        .await?;
    // 3. 代次/epoch/状态判定；拒绝路径到此为止，stop 调用次数为 0。
    let plan = begin_automation_cancellation(&command, request, current.as_ref())
        .map_err(cancellation_error)?;
    if plan.replayed && !plan.stop_required {
        return Ok(AutomationCancellationOutcome::Replayed(plan.fact));
    }
    // 4. 新事实先落库：即使随后崩溃，“取消意图/在途状态”也有可重取依据。
    let fact = if plan.replayed {
        plan.fact
    } else {
        let expected = current.as_ref().map(|fact| fact.fact_digest.as_str());
        cancellations
            .commit_cancellation(plan.fact, expected)
            .await?
            .fact
    };
    if !plan.stop_required {
        return Ok(outcome_from_fact(fact));
    }
    // 5. 恰好一次 stop；不确定即恢复，绝不伪造成功。
    let report = match stop.request_stop(&command, request).await {
        Ok(report) => report,
        Err(_) => return Ok(AutomationCancellationOutcome::RecoveryRequired(fact)),
    };
    let folded = match fold_automation_stop_report(&command, request, &fact, &report) {
        Ok(folded) => folded,
        Err(_) => return Ok(AutomationCancellationOutcome::RecoveryRequired(fact)),
    };
    // 6. 折叠结果 CAS 提交；提交不确定时保留在途事实。
    match cancellations
        .commit_cancellation(folded, Some(&fact.fact_digest))
        .await
    {
        Ok(commit) => Ok(outcome_from_fact(commit.fact)),
        Err(_) => Ok(AutomationCancellationOutcome::RecoveryRequired(fact)),
    }
}

/// 把一份迟到结果隔离成 `Fenced` 事实；没有已记录的取消事实时拒绝隔离。
///
/// 【核心流程】按 `(execution_id, attempt)` 重取当前事实——没有取消记录时返回
/// `PortError::Conflict(automation_cancel_fact_unknown)`（迟到结果只能在已有取消代次的执行上隔离，
/// 普通观察必须走 AUT-15 的 observe 路径）→ Domain `fence_automation_late_result()` 校验代次/epoch
/// 不得超前并拒绝二次隔离 → 用当前摘要做 CAS 提交。
///
/// 【输出】返回提交（或重放）的 `Fenced` 事实；该事实的停止历史保持不变，迟到结果不会覆盖它。
/// 【失败】事实不存在、绑定漂移、代次/epoch 超前、已经 `Fenced`、白名单不允许、CAS 冲突都稳定拒绝。
/// 【副作用】只调用注入的取消 store；不丢弃迟到结果本身（调用方需保留其摘要），不写 EventLog。
pub async fn record_automation_late_result<C>(
    cancellations: &C,
    late: &AutomationLateResult,
) -> Result<AutomationCancellationCommit, PortError>
where
    C: AutomationCancellationStore + ?Sized,
{
    let current = cancellations
        .fetch_cancellation(&late.execution_id, late.attempt)
        .await?
        .ok_or_else(|| PortError::Conflict("automation_cancel_fact_unknown".to_owned()))?;
    let fenced = fence_automation_late_result(&current, late).map_err(cancellation_error)?;
    cancellations
        .commit_cancellation(fenced, Some(&current.fact_digest))
        .await
}

/// 把已提交事实映射成对外去向：只有 `Stopped`/`NotExecuted` 是确定的关闭，其余都是恢复。
fn outcome_from_fact(fact: AutomationCancellationFact) -> AutomationCancellationOutcome {
    match fact.state {
        AutomationCancelState::NotExecuted => AutomationCancellationOutcome::NotExecuted(fact),
        AutomationCancelState::Stopped => AutomationCancellationOutcome::Stopped(fact),
        _ => AutomationCancellationOutcome::RecoveryRequired(fact),
    }
}

/// 把 Domain 静态错误码映射成端口错误；两种分类都不改变任何状态。
///
/// `*_invalid` 是输入或内部契约问题（重试不修复），其余是调用方认知过期或状态冲突。
fn cancellation_error(reason: &'static str) -> PortError {
    if reason.ends_with("_invalid") {
        PortError::Failed(reason.to_owned())
    } else {
        PortError::Conflict(reason.to_owned())
    }
}
