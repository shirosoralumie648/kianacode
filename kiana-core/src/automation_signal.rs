//! # 文件职责
//!
//! 本文件是 AUT-18 pause/resume/consume signal 的 Core 边界：
//!
//! * `validate_automation_signal_fact()` 是只读门面，把公开校验转交给
//!   `kiana_domain::validate_automation_signal()`；
//! * `apply_automation_signal()` 是**协调器**：从注入的 `AutomationSignalStore` 重取该 execution
//!   的权威 signal 历史，按既有事实重建 Domain 账本，调用 Domain `plan_automation_signal()` 做
//!   owner/definition/action/path/epoch/checkpoint/审批判定，再用摘要 CAS 单次提交下一事实。
//!
//! 真正的字段、状态矩阵、单次消费与 checkpoint 绑定都在 Domain；这里只负责端口顺序、CAS 期望值
//! 与“绝不派发”的边界。
//!
//! # 输入、处理与输出
//!
//! ```text
//! 请求/审批/checkpoint 形状校验（拒绝路径到此为止，0 次 store 调用）
//!   -> AutomationSignalStore::fetch_signals        // 权威历史，不信任调用方快照
//!   -> Domain record_recovered() 重建账本          // 序号/转移/审批单次消费在装填时再校验
//!   -> Domain plan_automation_signal()             // 真实拒绝面：漂移/重复/epoch/过期/UI context
//!   -> AutomationSignalStore::commit_signal        // 摘要 CAS 单次落库
//!   -> AutomationSignalOutcome                     // Paused / Resumed / Consumed / Replayed
//! ```
//!
//! 【本文件实际检查什么】协调器只信任注入 store 返回的历史：它不采信调用方自己的“当前状态”快照，
//! 不重新实现授权（approver 角色越权/自批由 Domain DTO 校验），也不读时钟：`now_tick` 由调用方提供
//! 且不被认证。store 返回的历史若跳号、摘要被篡改或转移非法，重建账本就会以稳定错误码失败，因此
//! 篡改历史不会被折叠成成功。
//!
//! 【副作用和调用边界】本函数只调用注入的 signal store。它不写 EventLog、不调用 Broker/模型、不暂停
//! 或恢复任何 worker、不创建 checkpoint、不派发 effect；`AutomationSignalPlan::dispatch_permitted`
//! 恒为 `false`，真实执行必须继续经过既有 ControlPlane/Broker 脊柱。`Ok(Resumed)` 只表示一条已提交
//! 的 signal 事实，不代表外部现实已经被对账。

use kiana_domain::{
    plan_automation_signal, validate_automation_signal, AutomationCheckpoint,
    AutomationSignalApproval, AutomationSignalFact, AutomationSignalLedger,
    AutomationSignalRequest, AutomationSignalState,
};
use kiana_ports::{AutomationSignalStore, PortError};

/// 将单条 signal fact 交给 Domain 校验器；错误码原样返回。
///
/// 【输入】调用方已构造或反序列化的 `AutomationSignalFact`。
/// 【输出】结构自洽时返回 `Ok(())`，否则返回 `automation_signal_fact_invalid`。
/// 【副作用/边界】无 I/O、无日志、无存储写入，也不校验历史 signal_id 是否重复；需要真正编排
/// pause/resume 时使用 [`apply_automation_signal`]。
pub fn validate_automation_signal_fact(fact: &AutomationSignalFact) -> Result<(), &'static str> {
    validate_automation_signal(fact)
}

/// 一次 signal 协调的最终去向；四个变体都只描述已提交事实，不含任何“已派发/已执行”语义。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationSignalOutcome {
    /// 已提交（或重放命中）一条 `Paused` 事实；模型循环应保持挂起，等待审批恢复。
    Paused(AutomationSignalFact),
    /// 已提交一条 `Resumed` 事实；恢复绑定 checkpoint 与审批摘要，`dispatch_permitted` 仍为 false。
    Resumed(AutomationSignalFact),
    /// 已提交一条 `Consumed` 事实；signal 生命周期到达终态，不能再次恢复。
    Consumed(AutomationSignalFact),
    /// 同一 `signal_id` 同一内容已记录过：没有新事实，也没有第二次审批消费。
    Replayed(AutomationSignalFact),
}

impl AutomationSignalOutcome {
    /// 本次协调是否新建了事实；`false` 表示幂等重放。
    pub fn committed(&self) -> bool {
        !matches!(self, Self::Replayed(_))
    }

    /// 当前事实副本。
    pub fn fact(&self) -> &AutomationSignalFact {
        match self {
            Self::Paused(fact)
            | Self::Resumed(fact)
            | Self::Consumed(fact)
            | Self::Replayed(fact) => fact,
        }
    }
}

/// 重取权威 signal 历史、重建账本、计划下一步事实并 CAS 单次提交。
///
/// 【调用者】`kiana-daemon` 的受控组合根、Core facade 的调用方或任何已经过授权的入口；每一步都通过
/// 端口进入，本函数自身不访问 EventStore、Broker、模型、时钟或文件系统。
///
/// 【核心流程】
/// 1. 请求、审批与 checkpoint 只做形状校验：UI 自带 context、自批/越权审批、坏摘要等拒绝路径在
///    触碰 store 之前就返回，因此这些路径的 store 调用次数为 0；
/// 2. `signals.fetch_signals(execution_id)` 取权威历史；调用方不提供自己的状态快照；
/// 3. 用 Domain `AutomationSignalLedger::record_recovered()` 逐条重放历史，序号回退/跳号、转移非法、
///    摘要篡改或审批重复消费都会在这里失败（`PortError::Conflict`），不会被当成空历史；
/// 4. Domain `plan_automation_signal()`：同 signal_id 同内容 → 幂等重放；owner/definition/action/path
///    漂移、序号冲突、epoch 回退/非法前进、审批过期/漂移/已消费、checkpoint 缺失/漂移各自稳定拒绝；
/// 5. 非重放时用“当前最新事实摘要”（历史为空则是 `None`）作为 CAS 期望值提交新事实；提交冲突原样
///    返回，绝不把不确定提交写成成功。
///
/// 【失败】所有拒绝都以稳定的 `PortError` 返回：形状/契约问题为 `Failed(*_invalid)`，其余为
/// `Conflict(automation_signal_*)`；拒绝路径不追加事实，也不触碰任何执行边界。
/// 【副作用】只调用注入的 signal store；不写 EventLog、不调用 Broker/模型、不暂停/恢复 worker、
/// 不创建 checkpoint。
pub async fn apply_automation_signal<S>(
    signals: &S,
    request: &AutomationSignalRequest,
    approval: Option<&AutomationSignalApproval>,
    checkpoint: Option<&AutomationCheckpoint>,
    now_tick: u64,
) -> Result<AutomationSignalOutcome, PortError>
where
    S: AutomationSignalStore + ?Sized,
{
    // 1. 形状校验先于任何端口调用：拒绝路径 0 次 store 调用。
    request.validate().map_err(automation_signal_error)?;
    if let Some(approval) = approval {
        approval.validate().map_err(automation_signal_error)?;
    }
    if let Some(checkpoint) = checkpoint {
        checkpoint.validate().map_err(automation_signal_error)?;
    }
    // 2. 权威历史；不采信调用方快照。
    let history = signals.fetch_signals(&request.execution_id).await?;
    // 3. 装填账本：历史自身必须可重放，否则 fail closed。
    let ledger = rebuild_signal_ledger(&history)?;
    // 4. 计划下一步事实；真实拒绝面在这里。
    let plan = plan_automation_signal(request, approval, checkpoint, &ledger, now_tick)
        .map_err(automation_signal_error)?;
    if plan.replayed {
        return Ok(AutomationSignalOutcome::Replayed(plan.fact));
    }
    // 5. 摘要 CAS 单次提交。
    let expected = ledger
        .latest(&request.execution_id)
        .map(|fact| fact.signal_digest.as_str());
    let commit = signals.commit_signal(plan.fact, expected).await?;
    Ok(outcome_from_fact(commit.fact, commit.replayed))
}

/// 用已提交历史重建账本；任一条事实不可重放都返回稳定冲突码。
///
/// 【边界】`record_recovered()` 只校验单条事实自洽与跨记录顺序，不证明历史来源真实；本函数不写
/// EventStore、不创建 checkpoint，也不消费新的审批。
fn rebuild_signal_ledger(
    history: &[AutomationSignalFact],
) -> Result<AutomationSignalLedger, PortError> {
    let mut ledger = AutomationSignalLedger::new();
    for fact in history {
        ledger
            .record_recovered(fact.clone())
            .map_err(automation_signal_error)?;
    }
    Ok(ledger)
}

/// 把已提交事实映射成对外去向：只有状态本身就是结论时才返回对应变体，重放单独标记。
fn outcome_from_fact(fact: AutomationSignalFact, replayed: bool) -> AutomationSignalOutcome {
    if replayed {
        return AutomationSignalOutcome::Replayed(fact);
    }
    match fact.state {
        AutomationSignalState::Paused => AutomationSignalOutcome::Paused(fact),
        AutomationSignalState::Resumed => AutomationSignalOutcome::Resumed(fact),
        AutomationSignalState::Consumed => AutomationSignalOutcome::Consumed(fact),
    }
}

/// 把 Domain 静态错误码映射成端口错误；两种分类都不改变任何状态。
///
/// `*_invalid` 是输入或内部契约问题（重试不修复），其余是调用方认知过期或状态冲突。
fn automation_signal_error(reason: &'static str) -> PortError {
    if reason.ends_with("_invalid") {
        PortError::Failed(reason.to_owned())
    } else {
        PortError::Conflict(reason.to_owned())
    }
}
