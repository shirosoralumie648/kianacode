//! # 文件职责
//!
//! 本文件是一个只读的 Core facade（门面）：把公开函数 `observe_automation_dispatch()` 转交给
//! `kiana-domain::validate_automation_observation()`。真正的字段、状态和摘要校验在 Domain；这里
//! 不拥有自动化队列，不选择执行 worker，也不调用 handler。
//!
//! # 输入、处理与输出
//!
//! ```text
//! 调用方准备 AutomationDispatchIntent（先前记录的 intent 快照）
//!       + AutomationObservation（调用方提交的执行结果快照）
//!       -> 本文件调用 Domain validator
//!       -> 校验两者各自有效且身份/authority epoch 一致
//!       -> 返回新的 intent：Observed 或 Unknown，并绑定 observation digest
//! ```
//!
//! 【本函数实际检查什么】Domain validator 要求 intent 当前处于 `Claimed`，并比较 worker ID、
//! execution ID、reservation ID、action digest 与 authority epoch；成功或失败结果都把状态转为
//! `Observed`，只有结果未知才转为 `Unknown`。它不读取当前权限、配额、容量或 wall clock，因此只
//! 能证明两份输入快照彼此一致，不能单独证明现场仍符合最新授权。
//!
//! 【副作用和调用边界】函数是同步纯校验：不写 EventStore、不发出 `dispatch.automated.*` 事件、
//! 不重试、不重新授权，也不会真的“观察”外部 worker。当前 workspace 中未找到生产代码调用本
//! facade；Domain 契约测试直接覆盖 validator。Core 在 `lib.rs` 重导出本函数，使外部 adapter
//! 可以显式调用，但重导出不等于已经接入产品分发路径。
//!
//! ⚠ 不要把此函数的 `Ok` 当作执行许可。调用方若要将结果用于真实分发，仍须通过 ControlPlane
//! 的当前授权/关卡检查，并自行把状态变更持久化；当前文件不完成这些步骤。
//!
//! # AUT-15：dispatch 与 observation 分离
//!
//! `dispatch_automation_worker_command()` 是本文件新增的协调器：它不信任调用方带来的快照，而是
//! 先从 `AutomationDispatchStore` 重取可重取命令，用 Domain 转移判定“能否执行”，只有 store 的
//! CAS claim 成功后才调用注入的 `AutomationWorkerEffectRunner` 恰好一次，最后把结构化观察结果
//! 提交回 store。任何拒绝、重放、并发失败或结果提交不确定都不返回成功：effect 已尝试但结果未确认
//! 时返回 `AutomationWorkerDispatchOutcome::Unknown`，由调用方按 command 状态对账。
//!
//! 该协调器仍不是授权边界：它只按已提交命令的 revision/worker 绑定决定能否执行，不检查当前
//! 授权、预算、审批或策略；真实 capability 执行必须继续经过 ControlPlane/Broker 脊柱。

use kiana_domain::{
    automation_dispatch_result, claim_automation_dispatch_command, validate_automation_observation,
    AutomationDispatchClaim, AutomationDispatchClaimRequest, AutomationDispatchCommand,
    AutomationDispatchIntent, AutomationDispatchObservationRequest, AutomationObservation,
    AutomationObservationOutcome,
};
use kiana_ports::{AutomationDispatchStore, AutomationWorkerEffectRunner, PortError};

/// 校验自动化分发 intent 与调用方提供的 observation 是否仍绑定同一 attempt。
///
/// 【作用】作为 Core 对 Domain 校验器的公开薄封装；`observe` 是历史 API 命名，本函数不会自行
/// 查询 worker 或系统状态。
/// 【调用者】由 `kiana-core/src/lib.rs` 重导出给 crate 使用方。当前仓库未找到生产调用；Domain
/// 层的状态/绑定规则由 `kiana-domain/tests/automation_dispatch.rs` 覆盖。
/// 【输入】`intent` 是此前准备并 claim 的快照；`observation` 是调用方收集后传入的完成结果。
/// 两者都由借用传入，因此本函数不会消耗调用方持有的数据。
/// 【输出】成功时返回 `intent` 的新副本：`Succeeded`/`Failed` outcome 变成 `Observed`，`Unknown`
/// outcome 变成 `Unknown`，并把 `result_digest` 写入 `observation_digest` 后重算 `intent_digest`。
/// 【失败】Domain 校验器返回静态错误码：例如 intent 自身无效、当前状态不是 `Claimed`、worker 或
/// execution/reservation/action/authority 绑定不同、observation 格式错误。此处不把错误写日志，
/// 是否拒绝、重试或恢复由调用方决定。
/// 【副作用】无；不会访问 EventStore、检查当前 epoch、调用 Policy/Gate、发送能力请求或产生审计事件。
///
/// 【核心流程】所有实质规则都在 `kiana-domain::validate_automation_observation()`：先分别验证 intent
/// 与 observation 的格式/摘要，再逐字段比较绑定身份；只有成功后才克隆并转换状态。因为本函数
/// 只比较调用者提供的数据，`Ok` 不足以证明这些数据来自可信 worker 或当前权限仍有效。
pub fn observe_automation_dispatch(
    intent: &AutomationDispatchIntent,
    observation: &AutomationObservation,
) -> Result<AutomationDispatchIntent, &'static str> {
    validate_automation_observation(intent, observation)
}

/// 一次 worker dispatch 的最终去向。
///
/// 三个变体互斥且都不把“尝试过 effect”伪装成成功：`Observed` 只在确定结果（Succeeded/Failed）
/// 已提交时返回；`Unknown` 表示 effect 已尝试但结果未确认，或记录的结果本身就是 Unknown；
/// `Replayed` 表示提交事实已经存在，本次没有执行 effect。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationWorkerDispatchOutcome {
    /// 本 worker 取得唯一 attempt、恰好执行一次 effect，并提交了确定结果。
    ///
    /// 命令处于 `Observed` 终态，`result` 里带结构化观察（已知失败含结构化失败码）。
    Observed(AutomationDispatchCommand),
    /// effect 已尝试但结果不确定；命令仍为 `Claimed`（提交不确定）或已记录 `Unknown`。
    ///
    /// 调用方必须按 `command_id` 重取/对账，不能当成成功，也不能假设动作没有发生。
    Unknown(AutomationDispatchCommand),
    /// 命令已经 claim 过或已是终态；本次没有执行 effect。
    ///
    /// 崩溃重启后携带过期 snapshot 的 worker 也走这条路径：身份相同则是安全重取，身份不同会在
    /// claim 阶段被拒。
    Replayed(AutomationDispatchCommand),
}

/// 重取命令、取得唯一 attempt、执行一次 effect，并把结构化结果提交回 store。
///
/// 【调用者】`kiana-daemon` 的 dispatch service 或任何受控的组合根；每一步都通过端口进入，
/// 本函数自身不访问 EventStore、Broker、模型或文件系统。
///
/// 【核心流程】
/// 1. `store.fetch_command(command_id)` 取权威副本——调用方不提供自己的快照，所以崩溃重启后
///    不会用旧 revision 或旧 worker 绑定派发；
/// 2. Domain `claim_automation_dispatch_command()` 判定：`AlreadyClaimed`/`Terminal` 直接返回
///    `Replayed`，**不触碰 runner**；只有 `Claimed` 继续；
/// 3. `store.claim_command()` 用当前 revision 做 CAS；并发竞争者在这里被拒（返回错误）或得到
///    `claimed=false`（同一 attempt 已占），两种情况都不执行 effect；
/// 4. CAS 成功后调用 `runner.run_dispatch_attempt()` **恰好一次**；runner 只能返回 outcome 与
///    摘要，execution/worker/attempt/epoch 由 Domain 从命令复制；
/// 5. `store.observe_command()` 提交结构化结果；结果本身为 Unknown 时返回 `Unknown` 终态命令，
///    提交失败（结果不确定）时返回 `Unknown(Claimed 命令)`，绝不返回 `Observed`。
///
/// 【失败】命令不存在、worker 冲突、revision 过期、并发 CAS 失败等都以稳定的 `PortError`
/// 返回，且此时 effect 次数为 0；runner 返回非法形状（例如非 `sha256:` 摘要）时 effect 已经
/// 尝试过，因此按 `Unknown` 返回而不是伪造成功。
/// 【副作用】只调用注入的 store 与 runner；不写 EventLog、不调用 Broker/模型、不改变授权。
pub async fn dispatch_automation_worker_command<S, R>(
    store: &S,
    runner: &R,
    command_id: &str,
    worker_id: &str,
    attempt: u32,
) -> Result<AutomationWorkerDispatchOutcome, PortError>
where
    S: AutomationDispatchStore + ?Sized,
    R: AutomationWorkerEffectRunner + ?Sized,
{
    // 1. 重取权威副本；不信任调用方快照。
    let current = store.fetch_command(command_id).await?;
    let claim = claim_automation_dispatch_command(&current, worker_id, attempt, current.revision)
        .map_err(dispatch_error)?;
    match claim {
        AutomationDispatchClaim::Claimed(next) => {
            // 2/3. 只有 CAS claim 成功才允许执行 effect。
            let commit = store
                .claim_command(AutomationDispatchClaimRequest {
                    command_id: next.command_id.clone(),
                    worker_id: worker_id.to_owned(),
                    attempt,
                    expected_revision: current.revision,
                })
                .await?;
            if !commit.claimed {
                return Ok(AutomationWorkerDispatchOutcome::Replayed(commit.command));
            }
            let claimed = commit.command;
            // 4. 恰好一次 effect；runner 不能自报身份。
            let outcome = runner.run_dispatch_attempt(&claimed).await;
            let result = match automation_dispatch_result(&claimed, outcome) {
                Ok(result) => result,
                // runner 返回的形状非法：effect 已经尝试，无法确认结果，保持 Claimed。
                Err(_) => return Ok(AutomationWorkerDispatchOutcome::Unknown(claimed)),
            };
            let unknown_result =
                result.observation.outcome == AutomationObservationOutcome::Unknown;
            // 5. 提交观察；失败即“提交不确定”，不返回成功。
            match store
                .observe_command(AutomationDispatchObservationRequest {
                    command_id: claimed.command_id.clone(),
                    expected_revision: claimed.revision,
                    result,
                })
                .await
            {
                Ok(commit) if unknown_result => {
                    Ok(AutomationWorkerDispatchOutcome::Unknown(commit.command))
                }
                Ok(commit) => Ok(AutomationWorkerDispatchOutcome::Observed(commit.command)),
                Err(_) => Ok(AutomationWorkerDispatchOutcome::Unknown(claimed)),
            }
        }
        AutomationDispatchClaim::AlreadyClaimed(command)
        | AutomationDispatchClaim::Terminal(command) => {
            Ok(AutomationWorkerDispatchOutcome::Replayed(command))
        }
    }
}

/// 把 Domain 静态错误码映射成端口错误；两种分类都不改变任何状态。
///
/// `*_invalid`/`*_overflow` 是输入或内部契约问题（重试不修复），其余是调用方认知过期或状态冲突。
fn dispatch_error(reason: &'static str) -> PortError {
    if reason.ends_with("_invalid") || reason.ends_with("_overflow") {
        PortError::Failed(reason.to_owned())
    } else {
        PortError::Conflict(reason.to_owned())
    }
}
