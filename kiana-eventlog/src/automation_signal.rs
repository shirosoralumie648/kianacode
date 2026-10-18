//! AUT-18 pause/resume signal 事实的 CAS 语义存储适配器（CI 语义）。
//!
//! # 本文件负责什么
//!
//! [`MemoryAutomationSignalStore`] 实现 `kiana_ports::AutomationSignalStore`：按 `execution_id`
//! 保存 signal 事实的**追加顺序**，并提供带 `expected_digest` 的比较交换（CAS）、signal_id
//! 去重、序号连续、状态转移白名单、身份绑定、authority epoch 单调与审批单次消费。它是
//! `kiana-eventlog` 中围绕事实存储的一族本地能力（与 `automation_dispatch`、
//! `automation_cancellation`、`workflow_queue` 同列），不是 EventStore 端口实现。
//!
//! # 明确不是持久化
//!
//! 与同族适配器一样，本适配器**只**提供进程内语义原子性：
//!   * 没有 fsync —— 进程退出，signal 历史全部消失；
//!   * 没有崩溃恢复 —— 重启后无法区分“从未提交”和“提交后崩溃”，恢复只能由调用方按既有
//!     EventLog 事实重放；
//!   * 没有跨进程围栏 —— 摘要 CAS 与审批单次消费只在当前对象内有意义。
//!
//! 因此它最多支撑 `local_behavior` 级别的 CI 证据，绝不能据此声称 durable。真正落盘时必须在
//! 同一 CAS 契约后面替换为 EventLog 支持的持久实现。
//!
//! # 关键语义
//!
//! ```text
//! 记录：commit_signal(Paused,      expected_digest = None)   // 第一条事实，序号必须是 1
//! 恢复：commit_signal(Resumed,     expected_digest)          // 必须绑定未消费审批
//! 消费：commit_signal(Consumed,    expected_digest)          // 只能引用恢复事实的审批摘要
//! 重取：fetch_signals(execution_id)                          // 权威历史，按追加顺序
//! ```
//!
//!   * 摘要 CAS 不匹配 → `automation_signal_digest_conflict`，不写事实；
//!   * 同 `signal_id` 同内容 → 安全重放（`replayed = true`，不重写）；不同内容 →
//!     `automation_signal_signal_conflict`；
//!   * 序号回退/跳号 → `automation_signal_sequence_regression` / `automation_signal_sequence_gap`；
//!   * 状态转移不在 pause→resume→consume 白名单（同状态的新事实也不允许）→
//!     `automation_signal_transition_refused`；
//!   * owner/definition/action/path 在同一个 execution 内漂移 → 稳定的 `*_drift` 码；
//!   * authority epoch 回退 → `automation_signal_authority_epoch_stale`；epoch 前进只允许出现在
//!     `Resumed` 事实（重启恢复），其余 → `automation_signal_authority_epoch_ahead`；
//!   * checkpoint 序号回退 → `automation_signal_checkpoint_sequence_regression`；
//!   * 已消费审批再次支撑恢复 → `automation_signal_approval_consumed`；`Consumed` 事实只能引用
//!     自己 `Resumed` 前驱的审批摘要，否则 → `automation_signal_approval_digest_drift`。
//!
//! 本适配器不暂停/恢复任何 worker、不执行外部动作、不调用 Broker/handler、不判断授权，也不把
//! 不确定提交写成成功。真实 pause/resume 边界属于 `kiana-core`/`kiana-runner` 的注入端口。

use async_trait::async_trait;
use kiana_domain::{
    automation_signal_transition_allowed, AutomationSignalCommit, AutomationSignalFact,
    AutomationSignalState,
};
use kiana_ports::{AutomationSignalStore, PortError};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tokio::sync::Mutex;

/// 进程内 CAS 语义 signal 事实存储适配器。
///
/// 克隆共享同一份状态（内部 `Arc<Mutex<_>>`）。所有方法都是 `&self`，因为互斥锁负责串行化；
/// 调用方不需要也不应该持有 `&mut`。键是 `execution_id`，值是按追加顺序保存的事实列表。
#[derive(Clone, Debug, Default)]
pub struct MemoryAutomationSignalStore {
    state: Arc<Mutex<MemoryAutomationSignalState>>,
}

#[derive(Debug, Default)]
struct MemoryAutomationSignalState {
    /// execution_id → 按追加顺序排列的 signal 事实。
    records: BTreeMap<String, Vec<AutomationSignalFact>>,
    /// 已被恢复/消费事实引用过的审批摘要。
    consumed_approvals: BTreeSet<String>,
}

impl MemoryAutomationSignalStore {
    /// 构造一个空存储；等价于 `Default`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 以 CAS 语义提交一条 signal 事实。
    ///
    /// 【调用者】Core 协调器在 Domain `plan_automation_signal()` 给出下一事实后调用。
    ///
    /// 【核心流程】
    /// 1. `fact.validate()` 失败返回 `PortError::Failed(automation_signal_fact_invalid)`；
    /// 2. 同 `signal_id` 已记录：同内容返回既有副本与 `replayed = true`（不重写），不同内容返回
    ///    `automation_signal_signal_conflict`；
    /// 3. 尚无事实：`expected_digest` 必须是 `None`，序号必须是 1，状态必须是 `Paused`；
    /// 4. 已有事实：`expected_digest` 必须等于当前摘要；序号必须恰好是下一个连续值；状态转移必须
    ///    在 pause→resume→consume 白名单内且不得同状态新建；
    /// 5. owner/definition/action/path 必须与上一条事实一致；authority epoch 不得回退且只允许在
    ///    `Resumed` 上前进；checkpoint 序号不得回退；
    /// 6. 审批摘要已被消费时只允许 `Consumed` 事实引用自己 `Resumed` 前驱的摘要，否则拒绝。
    ///
    /// 【输出/失败】成功返回事实记录；所有拒绝路径都不改变存储。事实通过校验只说明它自洽，
    /// 不代表审批人真实身份、checkpoint 已落盘或调用方有权暂停/恢复。
    pub async fn commit_signal(
        &self,
        fact: AutomationSignalFact,
        expected_digest: Option<&str>,
    ) -> Result<AutomationSignalCommit, PortError> {
        fact.validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        let mut state = self.state.lock().await;
        // 重放判定与全部校验先在不持有 `records` 可变借用的情况下完成（`records` 只按共享
        // 借用读取），随后先把审批消费写入 `consumed_approvals`，最后才取 `records` 的可变
        // 借用并 push。这样任何拒绝路径都发生在第一次状态写入之前，且两处可变借用不重叠。
        {
            let records = state.records.entry(fact.execution_id.clone()).or_default();
            if let Some(existing) = records
                .iter()
                .find(|existing| existing.signal_id == fact.signal_id)
            {
                if existing.signal_digest == fact.signal_digest {
                    return Ok(AutomationSignalCommit {
                        fact: existing.clone(),
                        replayed: true,
                    });
                }
                return Err(Self::conflict("automation_signal_signal_conflict"));
            }
            let Some(current) = records.last().cloned() else {
                if expected_digest.is_some() {
                    return Err(Self::conflict("automation_signal_digest_conflict"));
                }
                if fact.signal_sequence != 1 {
                    return Err(Self::conflict("automation_signal_sequence_gap"));
                }
                if fact.state != AutomationSignalState::Paused {
                    return Err(Self::conflict("automation_signal_transition_refused"));
                }
                records.push(fact.clone());
                return Ok(AutomationSignalCommit {
                    fact,
                    replayed: false,
                });
            };
            if expected_digest != Some(current.signal_digest.as_str()) {
                return Err(Self::conflict("automation_signal_digest_conflict"));
            }
            if fact.signal_sequence < current.signal_sequence {
                return Err(Self::conflict("automation_signal_sequence_regression"));
            }
            if fact.signal_sequence != current.signal_sequence + 1 {
                return Err(Self::conflict("automation_signal_sequence_gap"));
            }
            if current.state == fact.state
                || !automation_signal_transition_allowed(current.state, fact.state)
            {
                return Err(Self::conflict("automation_signal_transition_refused"));
            }
            if fact.owner_id != current.owner_id {
                return Err(Self::conflict("automation_signal_owner_drift"));
            }
            if fact.definition_digest != current.definition_digest {
                return Err(Self::conflict("automation_signal_definition_digest_drift"));
            }
            if fact.action_digest != current.action_digest {
                return Err(Self::conflict("automation_signal_action_digest_drift"));
            }
            if fact.path_scope_digest != current.path_scope_digest {
                return Err(Self::conflict("automation_signal_path_scope_drift"));
            }
            if fact.authority_epoch < current.authority_epoch {
                return Err(Self::conflict("automation_signal_authority_epoch_stale"));
            }
            if fact.authority_epoch > current.authority_epoch
                && fact.state != AutomationSignalState::Resumed
            {
                return Err(Self::conflict("automation_signal_authority_epoch_ahead"));
            }
            if fact.checkpoint_sequence < current.checkpoint_sequence {
                return Err(Self::conflict(
                    "automation_signal_checkpoint_sequence_regression",
                ));
            }
            let reuse_allowed = fact.state == AutomationSignalState::Consumed
                && current.state == AutomationSignalState::Resumed
                && current.approval_digest.is_some()
                && current.approval_digest == fact.approval_digest;
            if fact.state == AutomationSignalState::Consumed && !reuse_allowed {
                return Err(Self::conflict("automation_signal_approval_digest_drift"));
            }
            // 审批单次消费判定：拒绝路径必须在写入 `consumed_approvals` 之前返回。
            if let Some(digest) = fact.approval_digest.as_deref() {
                if state.consumed_approvals.contains(digest) && !reuse_allowed {
                    return Err(Self::conflict("automation_signal_approval_consumed"));
                }
            }
        }
        // 所有校验已通过：先写审批消费，再追加事实（此顺序保证失败不会留下半写状态）。
        if let Some(digest) = fact.approval_digest.clone() {
            state.consumed_approvals.insert(digest);
        }
        state
            .records
            .entry(fact.execution_id.clone())
            .or_default()
            .push(fact.clone());
        Ok(AutomationSignalCommit {
            fact,
            replayed: false,
        })
    }

    /// 按 `execution_id` 重取权威事实历史（追加顺序）；从未记录时返回空列表。
    ///
    /// 纯读取：不消费审批、不执行动作、不推进状态。返回顺序就是提交顺序，调用方不得自行重排。
    pub async fn fetch_signals(
        &self,
        execution_id: &str,
    ) -> Result<Vec<AutomationSignalFact>, PortError> {
        let state = self.state.lock().await;
        Ok(state.records.get(execution_id).cloned().unwrap_or_default())
    }

    /// 某个审批摘要是否已被恢复/消费事实引用过；纯读取，不消费。
    pub async fn approval_consumed(&self, approval_digest: &str) -> bool {
        let state = self.state.lock().await;
        state.consumed_approvals.contains(approval_digest)
    }

    /// 构造 `PortError::Conflict`；摘要过期、序号冲突、身份漂移都属于“调用方认知过期”。
    fn conflict(reason: &str) -> PortError {
        PortError::Conflict(reason.to_owned())
    }
}

#[async_trait]
impl AutomationSignalStore for MemoryAutomationSignalStore {
    async fn fetch_signals(
        &self,
        execution_id: &str,
    ) -> Result<Vec<AutomationSignalFact>, PortError> {
        MemoryAutomationSignalStore::fetch_signals(self, execution_id).await
    }

    async fn commit_signal(
        &self,
        fact: AutomationSignalFact,
        expected_digest: Option<&str>,
    ) -> Result<AutomationSignalCommit, PortError> {
        MemoryAutomationSignalStore::commit_signal(self, fact, expected_digest).await
    }
}
