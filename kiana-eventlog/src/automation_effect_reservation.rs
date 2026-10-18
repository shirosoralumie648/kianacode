//! AUT-14 效果预留事实的 CAS 语义存储适配器（CI 语义）。
//!
//! # 本文件负责什么
//!
//! [`MemoryAutomationEffectReservationStore`] 把“某次执行提交了哪份效果预留、哪次唯一 attempt”
//! 这一组事实按 execution 聚合保存，并提供带 `expected_revision` 的比较交换（CAS）提交、
//! 幂等键去重和“提交后安全重取”。它是 `kiana-eventlog` 中围绕事实存储的一族本地能力
//! （与 `workflow_queue`、`retention_store` 同列），不是 EventStore 端口实现。
//!
//! # 明确不是持久化
//!
//! 与 `MemoryWorkflowQueueStore` 一样，本适配器**只**提供进程内语义原子性：
//!   * 没有 fsync —— 进程退出，事实全部消失；
//!   * 没有崩溃恢复 —— 重启后无法区分“从未提交”和“提交后崩溃”；
//!   * 没有跨进程围栏 —— revision 只在当前对象内有意义。
//!
//! 因此它最多支撑 `local_behavior` 级别的 CI 证据，绝不能据此声称 durable。真正落盘时必须在
//! 同一 CAS 契约后面替换为 EventLog 支持的持久实现。
//!
//! # 数据如何流过
//!
//! ```text
//! Core 先用 Domain ledger 完成 authority/scope/permit 校验
//!     -> commit_reservation(expected_revision, reservation)   // CAS 提交
//!     -> commit_attempt(expected_revision, attempt)           // 消耗 permit 后的唯一 attempt
//!     -> fetch_reservation / fetch_reservation_by_idempotency_key / fetch_attempt  // 安全重取
//! ```
//!
//! 每个 execution 是一条只增不改的事实流：reservation 与 attempt 各推进一次 revision。
//! 相同幂等键重放只在内容摘要相同时返回已存记录（`replayed=true`），不新建事实；同键不同摘要、
//! 同 reservation_id 不同幂等键、attempt 换 ID/换 permit、revision 过期都以稳定错误码拒绝。
//! 本适配器不执行外部动作、不调用 Broker/handler，也不判断授权；它只回答“这份事实能不能按当前
//! revision 被接受”。

use kiana_domain::{AutomationEffectAttempt, AutomationEffectReservation};
use kiana_ports::PortError;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 一份已提交 reservation 的事实记录，包含提交时分配的 execution 流 revision。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredAutomationReservation {
    /// 通过 `validate()` 的预留事实；提交后本适配器不会改写它。
    pub reservation: AutomationEffectReservation,
    /// 该事实进入 execution 流时占据的 revision（从 1 开始）。
    pub revision: u64,
}

/// 一次 attempt 的事实记录，包含提交时分配的 execution 流 revision。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredAutomationAttempt {
    /// 调用方用 Domain ledger 消耗 permit 后形成的 attempt 事实。
    pub attempt: AutomationEffectAttempt,
    /// 该事实进入 execution 流时占据的 revision。
    pub revision: u64,
}

/// `commit_reservation` 的结果：`replayed=true` 表示没有新建事实，`stored` 是既有副本。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationEffectReservationCommit {
    /// 最终生效的已提交事实（新建或重放原记录）。
    pub stored: StoredAutomationReservation,
    /// 本次调用是否命中幂等键并重放了既有记录。
    pub replayed: bool,
}

/// `commit_attempt` 的结果：`replayed=true` 表示同一 attempt 已被提交过。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationEffectAttemptCommit {
    /// 最终生效的 attempt 记录。
    pub stored: StoredAutomationAttempt,
    /// 本次调用是否重放了既有 attempt。
    pub replayed: bool,
}

/// 适配器的进程内状态；所有读写都在同一个 `Mutex` 下串行化。
#[derive(Clone, Debug, Default)]
struct ReservationStoreState {
    /// 每个 execution 的已提交事实计数；新事实从 `expected_revision` 做 CAS。
    revisions: BTreeMap<String, u64>,
    /// 已提交 reservation，key = reservation_id。
    reservations: BTreeMap<String, StoredAutomationReservation>,
    /// 幂等索引，key = idempotency_key -> reservation_id。
    by_idempotency: BTreeMap<String, String>,
    /// 已提交 attempt，key = reservation_id；一份 reservation 至多一条。
    attempts: BTreeMap<String, StoredAutomationAttempt>,
}

/// 进程内 CAS 语义 reservation 存储适配器。
///
/// 克隆共享同一份状态（内部 `Arc<Mutex<_>>`）。所有方法都是 `&self`，因为互斥锁负责串行化；
/// 调用方不需要也不应该持有 `&mut`。
#[derive(Clone, Debug, Default)]
pub struct MemoryAutomationEffectReservationStore {
    state: Arc<Mutex<ReservationStoreState>>,
}

impl MemoryAutomationEffectReservationStore {
    /// 构造一个空存储；等价于 `Default`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 以 CAS 语义提交一份已校验的预留事实。
    ///
    /// 【调用者】Core 在 Domain ledger `reserve()` 成功后调用；本方法不重复 authority/scope 判断，
    /// 只负责事实去重与流 revision。
    ///
    /// 【核心流程】
    /// 1. `reservation.validate()` 失败即返回 `PortError::Failed(automation_effect_reservation_invalid)`；
    /// 2. 幂等键已存在：摘要相同返回既有记录（`replayed=true`，忽略 `expected_revision`，这是提交
    ///    不确定时“安全重取”的路径）；摘要不同返回
    ///    `PortError::Conflict(automation_effect_reservation_idempotency_conflict)`；
    /// 3. reservation_id 已被其他幂等键占用返回
    ///    `PortError::Conflict(automation_effect_reservation_conflict)`；
    /// 4. `expected_revision` 必须等于该 execution 当前 revision，否则返回
    ///    `PortError::Conflict(automation_effect_reservation_revision_conflict)`；
    /// 5. 写入事实并分配新 revision。
    ///
    /// 【输出/失败】成功返回提交记录；所有拒绝路径都不改变存储。事实通过校验只说明它自洽，
    /// 不代表外部动作被允许或已执行。
    pub async fn commit_reservation(
        &self,
        expected_revision: u64,
        reservation: AutomationEffectReservation,
    ) -> Result<AutomationEffectReservationCommit, PortError> {
        reservation
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        let mut state = self.state.lock().await;
        if let Some(reservation_id) = state.by_idempotency.get(&reservation.idempotency_key) {
            let stored = state
                .reservations
                .get(reservation_id)
                .ok_or_else(|| Self::failed("automation_effect_reservation_store_corrupt"))?;
            if stored.reservation.reservation_digest == reservation.reservation_digest {
                return Ok(AutomationEffectReservationCommit {
                    stored: stored.clone(),
                    replayed: true,
                });
            }
            return Err(Self::conflict(
                "automation_effect_reservation_idempotency_conflict",
            ));
        }
        if state.reservations.contains_key(&reservation.reservation_id) {
            return Err(Self::conflict("automation_effect_reservation_conflict"));
        }
        let current = state
            .revisions
            .get(&reservation.execution_id)
            .copied()
            .unwrap_or(0);
        if expected_revision != current {
            return Err(Self::conflict(
                "automation_effect_reservation_revision_conflict",
            ));
        }
        let revision = current
            .checked_add(1)
            .ok_or_else(|| Self::failed("automation_effect_reservation_revision_overflow"))?;
        let stored = StoredAutomationReservation {
            reservation: reservation.clone(),
            revision,
        };
        state.by_idempotency.insert(
            reservation.idempotency_key.clone(),
            reservation.reservation_id.clone(),
        );
        state
            .reservations
            .insert(reservation.reservation_id.clone(), stored.clone());
        state
            .revisions
            .insert(reservation.execution_id.clone(), revision);
        Ok(AutomationEffectReservationCommit {
            stored,
            replayed: false,
        })
    }

    /// 以 CAS 语义提交一份唯一的 attempt 事实。
    ///
    /// 【调用者】Core 用 Domain ledger 消耗 permit、得到 `Started(attempt)` 后转调；attempt 必须
    /// 已经 `validate()` 通过且绑定同一 reservation。
    ///
    /// 【核心流程】
    /// 1. attempt 自身校验失败返回 `PortError::Failed(automation_effect_attempt_invalid)`；
    /// 2. 对应 reservation 尚未提交返回
    ///    `PortError::Conflict(automation_effect_attempt_reservation_unknown)`；
    /// 3. attempt 的 reservation_digest/execution_id/action_digest/authority_epoch 与已提交
    ///    reservation 任一不一致返回
    ///    `PortError::Conflict(automation_effect_attempt_binding_drift)`；
    /// 4. 该 reservation 已有 attempt：完全相同（attempt_id + permit_id + attempt_digest）则
    ///    返回既有记录（`replayed=true`）；否则返回
    ///    `PortError::Conflict(automation_effect_attempt_conflict)`；
    /// 5. `expected_revision` CAS 失败返回
    ///    `PortError::Conflict(automation_effect_reservation_revision_conflict)`；
    /// 6. 写入 attempt 并推进 revision。
    ///
    /// 【输出/失败】成功返回 attempt 记录；拒绝路径不写入任何事实，也不触发外部动作。
    pub async fn commit_attempt(
        &self,
        expected_revision: u64,
        attempt: AutomationEffectAttempt,
    ) -> Result<AutomationEffectAttemptCommit, PortError> {
        attempt
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        let mut state = self.state.lock().await;
        let reservation = state
            .reservations
            .get(&attempt.reservation_id)
            .ok_or_else(|| Self::conflict("automation_effect_attempt_reservation_unknown"))?
            .clone();
        if reservation.reservation.reservation_digest != attempt.reservation_digest
            || reservation.reservation.execution_id != attempt.execution_id
            || reservation.reservation.action_digest != attempt.action_digest
            || reservation.reservation.authority_epoch != attempt.authority_epoch
        {
            return Err(Self::conflict("automation_effect_attempt_binding_drift"));
        }
        if let Some(stored) = state.attempts.get(&attempt.reservation_id) {
            if stored.attempt.attempt_id == attempt.attempt_id
                && stored.attempt.permit_id == attempt.permit_id
                && stored.attempt.attempt_digest == attempt.attempt_digest
            {
                return Ok(AutomationEffectAttemptCommit {
                    stored: stored.clone(),
                    replayed: true,
                });
            }
            return Err(Self::conflict("automation_effect_attempt_conflict"));
        }
        let current = state
            .revisions
            .get(&reservation.reservation.execution_id)
            .copied()
            .unwrap_or(0);
        if expected_revision != current {
            return Err(Self::conflict(
                "automation_effect_reservation_revision_conflict",
            ));
        }
        let revision = current
            .checked_add(1)
            .ok_or_else(|| Self::failed("automation_effect_reservation_revision_overflow"))?;
        let stored = StoredAutomationAttempt { attempt, revision };
        state
            .revisions
            .insert(reservation.reservation.execution_id.clone(), revision);
        state
            .attempts
            .insert(stored.attempt.reservation_id.clone(), stored.clone());
        Ok(AutomationEffectAttemptCommit {
            stored,
            replayed: false,
        })
    }

    /// 按 reservation_id 重取已提交事实；未提交返回
    /// `PortError::Conflict(automation_effect_reservation_unknown)`。
    ///
    /// 纯读取：不消耗 revision、不产生 attempt，也不执行动作；“提交后可安全重取”依赖这条路径。
    pub async fn fetch_reservation(
        &self,
        reservation_id: &str,
    ) -> Result<StoredAutomationReservation, PortError> {
        let state = self.state.lock().await;
        state
            .reservations
            .get(reservation_id)
            .cloned()
            .ok_or_else(|| Self::conflict("automation_effect_reservation_unknown"))
    }

    /// 按幂等键重取已提交事实；未提交返回
    /// `PortError::Conflict(automation_effect_reservation_unknown)`。
    ///
    /// 调用方在提交结果不确定时先查幂等键，命中的既有记录即为权威副本。
    pub async fn fetch_reservation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<StoredAutomationReservation, PortError> {
        let state = self.state.lock().await;
        let reservation_id = state
            .by_idempotency
            .get(idempotency_key)
            .ok_or_else(|| Self::conflict("automation_effect_reservation_unknown"))?;
        state
            .reservations
            .get(reservation_id)
            .cloned()
            .ok_or_else(|| Self::failed("automation_effect_reservation_store_corrupt"))
    }

    /// 按 reservation_id 重取已提交 attempt；未提交返回
    /// `PortError::Conflict(automation_effect_attempt_unknown)`。
    pub async fn fetch_attempt(
        &self,
        reservation_id: &str,
    ) -> Result<StoredAutomationAttempt, PortError> {
        let state = self.state.lock().await;
        state
            .attempts
            .get(reservation_id)
            .cloned()
            .ok_or_else(|| Self::conflict("automation_effect_attempt_unknown"))
    }

    /// 返回某个 execution 当前的流 revision；从未提交过事实时为 0。
    ///
    /// 调用方用它在下一次 CAS 提交前取得期望值；本方法不推进状态。
    pub async fn revision(&self, execution_id: &str) -> u64 {
        let state = self.state.lock().await;
        state.revisions.get(execution_id).copied().unwrap_or(0)
    }

    /// 构造 `PortError::Conflict`；见 `MemoryWorkflowQueueStore::lease_error` 的分类理由：
    /// 版本过期、重复消费与状态冲突都属于“调用方认知过期”，可重试但当前必须拒绝。
    fn conflict(reason: &str) -> PortError {
        PortError::Conflict(reason.to_owned())
    }

    /// 构造 `PortError::Failed`；用于输入不满足契约或内部状态损坏，重试本身不会修复。
    fn failed(reason: &str) -> PortError {
        PortError::Failed(reason.to_owned())
    }
}
