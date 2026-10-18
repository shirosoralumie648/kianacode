//! AUT-17 取消事实的 CAS 语义存储适配器（CI 语义）。
//!
//! # 本文件负责什么
//!
//! [`MemoryAutomationCancellationStore`] 实现 `kiana_ports::AutomationCancellationStore`：按
//! `(execution_id, attempt)` 聚合保存取消事实，并提供带 `expected_digest` 的比较交换（CAS）、
//! 取消代次单调性与同代次状态转移白名单。它是 `kiana-eventlog` 中围绕事实存储的一族本地能力
//! （与 `automation_dispatch`、`workflow_queue`、`automation_effect_reservation` 同列），不是
//! EventStore 端口实现。
//!
//! # 明确不是持久化
//!
//! 与同族适配器一样，本适配器**只**提供进程内语义原子性：
//!   * 没有 fsync —— 进程退出，取消事实全部消失；
//!   * 没有崩溃恢复 —— 重启后无法区分“从未提交”和“提交后崩溃”；
//!   * 没有跨进程围栏 —— generation 与摘要 CAS 只在当前对象内有意义。
//!
//! 因此它最多支撑 `local_behavior` 级别的 CI 证据，绝不能据此声称 durable。真正落盘时必须在
//! 同一 CAS 契约后面替换为 EventLog 支持的持久实现。
//!
//! # 关键语义
//!
//! ```text
//! 记录：commit_cancellation(fact, expected_digest = None)        // 第一条取消事实
//! 推进：commit_cancellation(Stopping/Stopped/?, expected_digest) // 同代次在途更新
//! 换代：commit_cancellation(fact_generation+1, expected_digest)  // 新取消代次
//! 隔离：commit_cancellation(Fenced, expected_digest)             // 迟到结果，不能复活执行
//! 重取：fetch_cancellation(execution_id, attempt)                // 提交不确定时的权威副本
//! ```
//!
//!   * 摘要 CAS 不匹配 → `automation_cancellation_digest_conflict`，不写事实；
//!   * 代次低于当前事实 → `automation_cancel_generation_stale`，陈旧取消不能推进事实；
//!   * 同代次但内容不同 → 必须落在白名单转移里，否则 `automation_cancel_transition_refused`；
//!     `Fenced`/`Stopped`/`NotExecuted` 都不能被改回“活着”的状态；
//!   * 同代次改 epoch → `automation_cancel_authority_epoch_conflict`；
//!   * 内容与当前事实完全相同 → 安全重放（`replayed = true`，不重写）。
//!
//! 本适配器不发出 stop、不执行外部动作、不调用 Broker/handler、不判断授权，也不把不确定提交写成
//! 成功。真实停止请求属于 `kiana-core`/`kiana-daemon` 注入的 stop 控制器边界。

use async_trait::async_trait;
use kiana_domain::{
    automation_cancel_transition_allowed, AutomationCancellationCommit, AutomationCancellationFact,
};
use kiana_ports::{AutomationCancellationStore, PortError};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 进程内 CAS 语义取消事实存储适配器。
///
/// 克隆共享同一份状态（内部 `Arc<Mutex<_>>`）。所有方法都是 `&self`，因为互斥锁负责串行化；
/// 调用方不需要也不应该持有 `&mut`。键是 `(execution_id, attempt)`，因此同一 execution 的不同
/// attempt 各自有独立的取消代次流。
#[derive(Clone, Debug, Default)]
pub struct MemoryAutomationCancellationStore {
    state: Arc<Mutex<BTreeMap<(String, u32), AutomationCancellationFact>>>,
}

impl MemoryAutomationCancellationStore {
    /// 构造一个空存储；等价于 `Default`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 以 CAS 语义提交一份取消事实。
    ///
    /// 【调用者】Core 协调器在 Domain `begin_automation_cancellation()` /
    /// `fold_automation_stop_report()` / `fence_automation_late_result()` 给出下一事实后调用。
    ///
    /// 【核心流程】
    /// 1. `fact.validate()` 失败返回 `PortError::Failed(automation_cancellation_fact_invalid)`；
    /// 2. 尚无事实：`expected_digest` 必须是 `None`，否则
    ///    `PortError::Conflict(automation_cancellation_digest_conflict)`；写入并返回；
    /// 3. 已有事实：`expected_digest` 必须等于当前摘要（否则同上冲突码），内容与当前完全相同时
    ///    返回当前副本与 `replayed = true`（不重写）；
    /// 4. 代次低于当前 → `PortError::Conflict(automation_cancel_generation_stale)`；
    /// 5. 同代次但 epoch 不同 → `PortError::Conflict(automation_cancel_authority_epoch_conflict)`；
    /// 6. 同代次且白名单不允许该转移 → `PortError::Conflict(automation_cancel_transition_refused)`；
    /// 7. 其余情况写入新事实。
    ///
    /// 【输出/失败】成功返回事实记录；所有拒绝路径都不改变存储。事实通过校验只说明它自洽，
    /// 不代表停止真的发生、代次真的最新或调用方有权取消。
    pub async fn commit_cancellation(
        &self,
        fact: AutomationCancellationFact,
        expected_digest: Option<&str>,
    ) -> Result<AutomationCancellationCommit, PortError> {
        fact.validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        let mut state = self.state.lock().await;
        let key = (fact.execution_id.clone(), fact.attempt);
        let Some(current) = state.get(&key).cloned() else {
            if expected_digest.is_some() {
                return Err(Self::conflict("automation_cancellation_digest_conflict"));
            }
            state.insert(key, fact.clone());
            return Ok(AutomationCancellationCommit {
                fact,
                replayed: false,
            });
        };
        if expected_digest != Some(current.fact_digest.as_str()) {
            return Err(Self::conflict("automation_cancellation_digest_conflict"));
        }
        if fact.fact_digest == current.fact_digest {
            return Ok(AutomationCancellationCommit {
                fact: current,
                replayed: true,
            });
        }
        if fact.cancel_generation < current.cancel_generation {
            return Err(Self::conflict("automation_cancel_generation_stale"));
        }
        if fact.cancel_generation == current.cancel_generation {
            if fact.authority_epoch != current.authority_epoch {
                return Err(Self::conflict("automation_cancel_authority_epoch_conflict"));
            }
            if !automation_cancel_transition_allowed(current.state, fact.state) {
                return Err(Self::conflict("automation_cancel_transition_refused"));
            }
        }
        state.insert(key, fact.clone());
        Ok(AutomationCancellationCommit {
            fact,
            replayed: false,
        })
    }

    /// 按 `(execution_id, attempt)` 重取当前权威事实；从未记录时返回 `None`。
    ///
    /// 纯读取：不消耗代次、不执行动作、不推进状态。
    pub async fn fetch_cancellation(
        &self,
        execution_id: &str,
        attempt: u32,
    ) -> Result<Option<AutomationCancellationFact>, PortError> {
        let state = self.state.lock().await;
        Ok(state.get(&(execution_id.to_owned(), attempt)).cloned())
    }

    /// 构造 `PortError::Conflict`；摘要过期、陈旧代次、非法转移都属于“调用方认知过期”。
    fn conflict(reason: &str) -> PortError {
        PortError::Conflict(reason.to_owned())
    }
}

#[async_trait]
impl AutomationCancellationStore for MemoryAutomationCancellationStore {
    async fn fetch_cancellation(
        &self,
        execution_id: &str,
        attempt: u32,
    ) -> Result<Option<AutomationCancellationFact>, PortError> {
        MemoryAutomationCancellationStore::fetch_cancellation(self, execution_id, attempt).await
    }

    async fn commit_cancellation(
        &self,
        fact: AutomationCancellationFact,
        expected_digest: Option<&str>,
    ) -> Result<AutomationCancellationCommit, PortError> {
        MemoryAutomationCancellationStore::commit_cancellation(self, fact, expected_digest).await
    }
}
