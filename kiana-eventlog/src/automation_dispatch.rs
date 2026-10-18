//! AUT-15 可重取 dispatch command 的 CAS 语义存储适配器（CI 语义）。
//!
//! # 本文件负责什么
//!
//! [`MemoryAutomationDispatchStore`] 实现 `kiana_ports::AutomationDispatchStore`：把“某次 worker
//! attempt 的派发命令、唯一 claim、结构化观察结果”按 execution 聚合保存，并提供带
//! `expected_revision` 的比较交换（CAS）提交、幂等键去重与“提交后安全重取”。它是
//! `kiana-eventlog` 中围绕事实存储的一族本地能力（与 `workflow_queue`、
//! `automation_effect_reservation` 同列），不是 EventStore 端口实现。
//!
//! # 明确不是持久化
//!
//! 与 `MemoryWorkflowQueueStore`、`MemoryAutomationEffectReservationStore` 一样，本适配器**只**
//! 提供进程内语义原子性：
//!   * 没有 fsync —— 进程退出，命令事实全部消失；
//!   * 没有崩溃恢复 —— 重启后无法区分“从未提交”和“提交后崩溃”；
//!   * 没有跨进程围栏 —— revision 与 claim 只在当前对象内有意义。
//!
//! 因此它最多支撑 `local_behavior` 级别的 CI 证据，绝不能据此声称 durable。真正落盘时必须在
//! 同一 CAS 契约后面替换为 EventLog 支持的持久实现。
//!
//! # 数据如何流过
//!
//! ```text
//! 派发：commit_command(expected_revision = 0)        // Prepared 命令进入 execution 流（revision 1）
//! claim：claim_command(worker_id, attempt, expected_revision) // 唯一 attempt，revision +1
//! 执行：调用方/协调器跑一次 effect（本文件不执行任何动作）
//! 观察：observe_command(expected_revision, result)   // Observed/Unknown，revision +1
//! 重取：fetch_command / fetch_command_by_idempotency_key // 提交不确定时的权威副本
//! ```
//!
//! 关键语义：
//!   * 同幂等键 + 同**首次提交**摘要 = 安全重取，返回当前权威副本（可能已被 claim 或终态），
//!     `replayed=true`，且不要求 `expected_revision` 仍然匹配——这正是“commit 后崩溃后重取”的路径；
//!   * 同幂等键 + 不同首次摘要、同 command_id 换幂等键、一个 execution 第二个命令、
//!     revision 过期都稳定拒绝；
//!   * 一个命令只有一个 worker + attempt；第二个 worker 或第二个 attempt 被拒；
//!   * 终态只接受**完全相同**的结果重放；不同结果（包括伪造成成功）按 revision 新旧分别返回
//!     版本冲突或观察冲突，绝不覆盖已有结果。
//!
//! 本适配器不执行外部动作、不调用 Broker/handler、不判断授权，也不把不确定提交写成成功。

use async_trait::async_trait;
use kiana_domain::{
    claim_automation_dispatch_command, observe_automation_dispatch_command,
    AutomationDispatchClaim, AutomationDispatchClaimCommit, AutomationDispatchClaimRequest,
    AutomationDispatchCommand, AutomationDispatchCommit, AutomationDispatchObservationCommit,
    AutomationDispatchObservationRequest,
};
use kiana_ports::{AutomationDispatchStore, PortError};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 幂等索引条目：记住**首次提交**的命令身份与摘要，用于区分安全重取与键冲突。
///
/// 命令随后可能被 claim/观察推进 revision，`original_digest` 不会跟着变，因此“同一个键重放”与
/// “同一个键换内容”可以被稳定区分。
#[derive(Clone, Debug, Eq, PartialEq)]
struct DispatchIdempotencyEntry {
    /// 首次用该键提交的命令 ID。
    command_id: String,
    /// 首次提交时的命令摘要。
    original_digest: String,
}

/// 适配器的进程内状态；所有读写都在同一个 `Mutex` 下串行化。
#[derive(Clone, Debug, Default)]
struct DispatchStoreState {
    /// 每个 execution 当前命令的 revision；新命令从 `expected_revision = 0` 开始。
    revisions: BTreeMap<String, u64>,
    /// 当前命令副本，key = command_id。
    commands: BTreeMap<String, AutomationDispatchCommand>,
    /// 幂等索引，key = idempotency_key。
    by_idempotency: BTreeMap<String, DispatchIdempotencyEntry>,
}

/// 进程内 CAS 语义 dispatch command 存储适配器。
///
/// 克隆共享同一份状态（内部 `Arc<Mutex<_>>`）。所有方法都是 `&self`，因为互斥锁负责串行化；
/// 调用方不需要也不应该持有 `&mut`。
#[derive(Clone, Debug, Default)]
pub struct MemoryAutomationDispatchStore {
    state: Arc<Mutex<DispatchStoreState>>,
}

impl MemoryAutomationDispatchStore {
    /// 构造一个空存储；等价于 `Default`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 以 CAS 语义提交一份命令事实。
    ///
    /// 【调用者】Core 协调器/组合根在 Domain `prepare_automation_dispatch_command()` 成功后调用。
    ///
    /// 【核心流程】
    /// 1. `command.validate()` 失败返回 `PortError::Failed(automation_dispatch_command_invalid)`；
    /// 2. 幂等键已存在：**首次提交**摘要相同返回当前权威副本（`replayed=true`，忽略
    ///    `expected_revision`，这是提交不确定后的安全重取路径）；摘要不同返回
    ///    `PortError::Conflict(automation_dispatch_idempotency_conflict)`；
    /// 3. command_id 已被其他幂等键占用返回
    ///    `PortError::Conflict(automation_dispatch_command_conflict)`；
    /// 4. 同一 execution 已有命令、`expected_revision` 不是当前流 revision、或
    ///    `command.revision` 不是 `expected_revision + 1` 时返回
    ///    `PortError::Conflict(automation_dispatch_revision_conflict)`；
    /// 5. 写入命令并推进 execution 流 revision。
    ///
    /// 【输出/失败】成功返回事实记录；所有拒绝路径都不改变存储。命令通过校验只说明它自洽，
    /// 不代表外部动作被允许或已执行。
    pub async fn commit_command(
        &self,
        command: AutomationDispatchCommand,
        expected_revision: u64,
    ) -> Result<AutomationDispatchCommit, PortError> {
        command
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        let mut state = self.state.lock().await;
        if let Some(entry) = state.by_idempotency.get(&command.idempotency_key) {
            if entry.original_digest != command.command_digest {
                return Err(Self::conflict("automation_dispatch_idempotency_conflict"));
            }
            let stored = state
                .commands
                .get(&entry.command_id)
                .cloned()
                .ok_or_else(|| Self::failed("automation_dispatch_store_corrupt"))?;
            return Ok(AutomationDispatchCommit {
                command: stored,
                replayed: true,
            });
        }
        if state.commands.contains_key(&command.command_id) {
            return Err(Self::conflict("automation_dispatch_command_conflict"));
        }
        let execution_id = command.intent.execution_id.clone();
        let current = state.revisions.get(&execution_id).copied().unwrap_or(0);
        if expected_revision != current || command.revision != current + 1 {
            return Err(Self::conflict("automation_dispatch_revision_conflict"));
        }
        state.revisions.insert(execution_id, command.revision);
        state.by_idempotency.insert(
            command.idempotency_key.clone(),
            DispatchIdempotencyEntry {
                command_id: command.command_id.clone(),
                original_digest: command.command_digest.clone(),
            },
        );
        state
            .commands
            .insert(command.command_id.clone(), command.clone());
        Ok(AutomationDispatchCommit {
            command,
            replayed: false,
        })
    }

    /// 以 CAS 语义把命令绑定到唯一 worker + attempt。
    ///
    /// 【调用者】Core 协调器在 Domain `claim_automation_dispatch_command()` 给出可执行判定后调用；
    /// 本方法在锁内**重新读取**当前命令并再次运行同一 Domain 判定，因此并发竞争者不会因为持有
    /// 过期快照而误判。
    ///
    /// 【核心流程】
    /// 1. 命令不存在返回 `PortError::Conflict(automation_dispatch_command_unknown)`；
    /// 2. Domain 判定为 `Claimed` 新占 → 写入并推进 revision，返回 `claimed=true`；
    /// 3. Domain 判定为 `AlreadyClaimed`/`Terminal` → 返回当前副本与 `claimed=false`（不写事实）；
    /// 4. Domain 拒绝（worker 冲突、revision 过期、命令无效）→ 映射成 `PortError`，不写事实。
    pub async fn claim_command(
        &self,
        request: AutomationDispatchClaimRequest,
    ) -> Result<AutomationDispatchClaimCommit, PortError> {
        let mut state = self.state.lock().await;
        let current = state
            .commands
            .get(&request.command_id)
            .cloned()
            .ok_or_else(|| Self::conflict("automation_dispatch_command_unknown"))?;
        match claim_automation_dispatch_command(
            &current,
            &request.worker_id,
            request.attempt,
            request.expected_revision,
        )
        .map_err(Self::domain_error)?
        {
            AutomationDispatchClaim::Claimed(next) => {
                state
                    .revisions
                    .insert(next.intent.execution_id.clone(), next.revision);
                state.commands.insert(next.command_id.clone(), next.clone());
                Ok(AutomationDispatchClaimCommit {
                    command: next,
                    claimed: true,
                })
            }
            AutomationDispatchClaim::AlreadyClaimed(command)
            | AutomationDispatchClaim::Terminal(command) => Ok(AutomationDispatchClaimCommit {
                command,
                claimed: false,
            }),
        }
    }

    /// 以 CAS 语义把结构化结果折叠进已 claim 的命令。
    ///
    /// 【调用者】Core 协调器在 runner 返回后调用；本方法同样在锁内重新读取当前命令。
    ///
    /// 【核心流程】
    /// 1. 命令不存在返回 `PortError::Conflict(automation_dispatch_command_unknown)`；
    /// 2. Domain 折叠成功且不是重放 → 写入终态命令并推进 revision，返回 `replayed=false`；
    /// 3. Domain 判定为**完全相同**结果的安全重放 → 返回当前副本与 `replayed=true`（不写事实）；
    /// 4. 陈旧 revision 的异构结果、终态覆盖尝试、未 claim 的观察都映射成稳定 `PortError`。
    ///
    /// 【并发语义】两个观察者提交同一 revision 时，先到者写入终态；后到者的异构结果按 revision
    /// 返回冲突，相同结果则重放同一事实，所以观察竞态不会产生第二个结果。
    pub async fn observe_command(
        &self,
        request: AutomationDispatchObservationRequest,
    ) -> Result<AutomationDispatchObservationCommit, PortError> {
        let mut state = self.state.lock().await;
        let current = state
            .commands
            .get(&request.command_id)
            .cloned()
            .ok_or_else(|| Self::conflict("automation_dispatch_command_unknown"))?;
        let (next, replayed) = observe_automation_dispatch_command(
            &current,
            &request.result,
            request.expected_revision,
        )
        .map_err(Self::domain_error)?;
        if replayed {
            return Ok(AutomationDispatchObservationCommit {
                command: current,
                replayed: true,
            });
        }
        state
            .revisions
            .insert(next.intent.execution_id.clone(), next.revision);
        state.commands.insert(next.command_id.clone(), next.clone());
        Ok(AutomationDispatchObservationCommit {
            command: next,
            replayed: false,
        })
    }

    /// 按 command_id 重取当前权威副本；不存在返回
    /// `PortError::Conflict(automation_dispatch_command_unknown)`。
    ///
    /// 纯读取：不消耗 revision、不执行动作、不推进状态。
    pub async fn fetch_command(
        &self,
        command_id: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        let state = self.state.lock().await;
        state
            .commands
            .get(command_id)
            .cloned()
            .ok_or_else(|| Self::conflict("automation_dispatch_command_unknown"))
    }

    /// 按幂等键重取当前权威副本；不存在返回
    /// `PortError::Conflict(automation_dispatch_command_unknown)`。
    ///
    /// 调用方在提交结果不确定时先查幂等键，命中的既有记录即为权威副本；返回的是**当前**副本
    /// （可能已被 claim 或终态），而不是首次提交时的快照。
    pub async fn fetch_command_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        let state = self.state.lock().await;
        let entry = state
            .by_idempotency
            .get(idempotency_key)
            .ok_or_else(|| Self::conflict("automation_dispatch_command_unknown"))?;
        state
            .commands
            .get(&entry.command_id)
            .cloned()
            .ok_or_else(|| Self::failed("automation_dispatch_store_corrupt"))
    }

    /// 返回某个 execution 当前的命令 revision；从未提交过命令时为 0。
    ///
    /// 调用方用它在下一次 CAS 提交前取得期望值；本方法不推进状态。
    pub async fn revision(&self, execution_id: &str) -> u64 {
        let state = self.state.lock().await;
        state.revisions.get(execution_id).copied().unwrap_or(0)
    }

    /// 把 Domain 静态错误码映射成端口错误：`*_invalid` 属于输入不满足契约，其余属于调用方
    /// 认知过期/状态冲突。两种都保持原状态不变。
    fn domain_error(reason: &'static str) -> PortError {
        if reason.ends_with("_invalid") || reason.ends_with("_not_prepared") {
            PortError::Failed(reason.to_owned())
        } else {
            PortError::Conflict(reason.to_owned())
        }
    }

    /// 构造 `PortError::Conflict`；版本过期、重复 worker/attempt、终态覆盖都属于“调用方认知过期”。
    fn conflict(reason: &str) -> PortError {
        PortError::Conflict(reason.to_owned())
    }

    /// 构造 `PortError::Failed`；用于输入不满足契约或内部状态损坏，重试本身不会修复。
    fn failed(reason: &str) -> PortError {
        PortError::Failed(reason.to_owned())
    }
}

#[async_trait]
impl AutomationDispatchStore for MemoryAutomationDispatchStore {
    async fn commit_command(
        &self,
        command: AutomationDispatchCommand,
        expected_revision: u64,
    ) -> Result<AutomationDispatchCommit, PortError> {
        MemoryAutomationDispatchStore::commit_command(self, command, expected_revision).await
    }

    async fn claim_command(
        &self,
        request: AutomationDispatchClaimRequest,
    ) -> Result<AutomationDispatchClaimCommit, PortError> {
        MemoryAutomationDispatchStore::claim_command(self, request).await
    }

    async fn observe_command(
        &self,
        request: AutomationDispatchObservationRequest,
    ) -> Result<AutomationDispatchObservationCommit, PortError> {
        MemoryAutomationDispatchStore::observe_command(self, request).await
    }

    async fn fetch_command(
        &self,
        command_id: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        MemoryAutomationDispatchStore::fetch_command(self, command_id).await
    }

    async fn fetch_command_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        MemoryAutomationDispatchStore::fetch_command_by_idempotency_key(self, idempotency_key).await
    }
}
