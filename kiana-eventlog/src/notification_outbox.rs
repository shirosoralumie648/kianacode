//! **通知发件箱与可靠投递模式**
//!
//! # 职责：管理向外系统的通知队列
//!
//! 这个模块实现了一个**确定性的**通知发件箱，用于向 MCP、webhook、邮件等外部系统
//! 发送一次性的通知。它不承诺落盘持久化或跨进程协调，
//! 但承诺一件事的**幂等性**：同一个通知在同一时间内不会被重复发送。
//!
//! # 通知的生命周期
//!
//! ```text
//! Pending ──enqueue──→ Pending (等待被领取)
//!           ↓ claim_next
//!         Claimed (某个 worker 说"我来发")
//!           ↓ mark_submitted
//!         Submitted (已经发给外部系统了)
//!           ↓ acknowledge
//!         Delivered (外部系统说"收到了")
//!
//! 如果中途失败或超时：
//!   Claimed ──失败──→ Pending (lease_expires_at_unix_ms 过了重新领取)
//!   Submitted ──失败──→ reconcile_unknown (未知状态的恢复)
//! ```
//!
//! # 为什么叫 outbox
//!
//! 这是一个经典的 Saga 模式概念。事件被提交到 EventLog 后，
//! 通知被写到 Outbox（这个表），两个操作要么都成功要么都失败。
//! 然后独立的 worker 进程从 Outbox 取通知并发送，
//! 发送成功后标记为已送达，失败则保留以便重试。
//!
//! 【这个实现的限制】
//! - 不落盘（所以进程结束就没了）
//! - 不跨进程同步（多个进程各有一份）
//! - 适合测试和确定性 CI 场景，不适合生产

use async_trait::async_trait;
use kiana_domain::{NotificationOutboxLease, NotificationOutboxRecord, NotificationOutboxState};
use kiana_ports::{NotificationOutboxStore, PortError};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// ============================================================================
// 内部存储结构
// ============================================================================

/// 通知发件箱的所有记录。
///
/// 【为什么用 BTreeMap】
/// - 保持某种顺序（方便诊断和测试）
/// - 快速查找（O(log n)）
/// - 不需要 HashMap 的哈希计算
#[derive(Clone, Debug, Default)]
struct NotificationOutboxStateStore {
    records: BTreeMap<String, NotificationOutboxRecord>,
}

// ============================================================================
// MemoryNotificationOutboxStore: 进程内发件箱
// ============================================================================

/// 进程内通知发件箱。
///
/// 【用途】测试和确定性 CI 场景。每个进程有独立的发件箱，
/// 不与其他进程同步，进程结束就消失。
///
/// 【设计】
/// - 单个 Mutex 保护整个状态
/// - `records` 用 outbox_id 做 key
/// - 每个 worker 只能一次领一个通知
#[derive(Clone, Debug, Default)]
pub struct MemoryNotificationOutboxStore {
    state: Arc<Mutex<NotificationOutboxStateStore>>,
}

impl MemoryNotificationOutboxStore {
    /// 创建一个新的空发件箱。
    pub fn new() -> Self {
        Self::default()
    }

    /// 按 outbox_id 查读一个通知记录。
    ///
    /// 【用途】测试和诊断。返回该记录的当前状态。
    pub async fn read(&self, outbox_id: &str) -> Result<NotificationOutboxRecord, PortError> {
        let state = self.state.lock().await;
        state
            .records
            .get(outbox_id)
            .cloned()
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))
    }

    /// 生成 BTreeMap 的 key。
    fn key(record: &NotificationOutboxRecord) -> String {
        record.outbox_id.to_string()
    }
}

// ============================================================================
// NotificationOutboxStore trait implementation
// ============================================================================

#[async_trait]
impl NotificationOutboxStore for MemoryNotificationOutboxStore {
    // =========================================================================
    // enqueue_notification: 添加一个待发的通知
    // =========================================================================

    /// 将一个通知加入发件箱的待发队列。
    ///
    /// 【前置条件】
    /// - `record` 的所有字段必须通过 `validate()`（由 domain 类型保证）
    /// - `outbox_id` 不能重复
    ///
    /// 【错误】
    /// - `notification_outbox_record:<validation-error>`: 通知的字段无效
    /// - `notification_outbox_duplicate`: 该 outbox_id 已存在
    ///
    /// 【行为】成功插入后，通知进入 `Pending` 状态，等待 worker 去 `claim_next` 领取。
    async fn enqueue_notification(
        &self,
        record: NotificationOutboxRecord,
    ) -> Result<NotificationOutboxRecord, PortError> {
        record
            .validate()
            .map_err(|error| PortError::Failed(format!("notification_outbox_record:{error}")))?;
        let key = Self::key(&record);
        let mut state = self.state.lock().await;
        if state.records.contains_key(&key) {
            return Err(PortError::Conflict(
                "notification_outbox_duplicate".to_owned(),
            ));
        }
        state.records.insert(key, record.clone());
        Ok(record)
    }

    // =========================================================================
    // claim_next_notification: Worker 领一个通知去发
    // =========================================================================

    /// Worker 从队列中领取一个待发（或超期）的通知。
    ///
    /// 【工作流】
    /// 1. 扫描所有记录
    /// 2. 找到第一个满足以下条件的：
    ///    - 状态是 `Pending`，或
    ///    - 状态是 `Claimed` 但 lease 已过期（`lease_expires_at_unix_ms <= now_unix_ms`）
    /// 3. 调用 `record.claim(worker_id, ...)` 更新状态和 lease
    /// 4. 返回 (record, lease)
    ///
    /// 【参数】
    /// - `worker_id`: 标识这个 worker（用于诊断和幂等性）
    /// - `authority_epoch`: 当前的权威机构版本号
    /// - `now_unix_ms`: 当前时间戳（毫秒）
    /// - `lease_ttl_ms`: lease 有效期（例如 30 秒 = 30000ms）
    ///
    /// 【返回】
    /// - `Ok(Some((record, lease)))`: 成功领取了一个通知
    /// - `Ok(None)`: 没有待发的通知了
    /// - `Err(...)`: 记录校验失败
    ///
    /// 【关键点】
    /// - 如果 `record.claim()` 返回「当前有其他 worker 的 lease 还活着」，
    ///   则跳过这个记录继续找下一个
    /// - 只要有通知被成功领取，就立即返回（即使后面还有更多）
    async fn claim_next_notification(
        &self,
        worker_id: &str,
        authority_epoch: u64,
        now_unix_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<Option<(NotificationOutboxRecord, NotificationOutboxLease)>, PortError> {
        let mut state = self.state.lock().await;
        let keys = state.records.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            let Some(record) = state.records.get_mut(&key) else {
                continue;
            };
            // 【可领取条件】Pending 或者 Claimed 但 lease 过期
            let claimable = record.state == NotificationOutboxState::Pending
                || (record.state == NotificationOutboxState::Claimed
                    && record
                        .lease_expires_at_unix_ms
                        .is_some_and(|expires| expires <= now_unix_ms));
            if !claimable {
                continue;
            }
            match record.claim(worker_id, authority_epoch, now_unix_ms, lease_ttl_ms) {
                Ok(lease) => return Ok(Some((record.clone(), lease))),
                Err(error) if error == "notification_outbox_lease_active" => continue,
                Err(error) => return Err(PortError::Conflict(error)),
            }
        }
        Ok(None)
    }

    // =========================================================================
    // mark_notification_submitted: 已发送给外部系统
    // =========================================================================

    /// 标记一个通知已经发送给了外部系统。
    ///
    /// 【前提】Worker 已经用 `claim_next_notification` 领到这个通知，
    /// 现在它想说「我已经把这个通知发给外部系统了」。
    ///
    /// 【操作】
    /// - 验证 lease 还活着（通过 outbox_id 找到记录，调用 `record.mark_submitted(lease, now_unix_ms)`）
    /// - 状态转为 `Submitted`
    ///
    /// 【错误】
    /// - `notification_outbox_missing`: 没有找到这个 outbox_id（不应该发生）
    /// - 其他：来自 `record.mark_submitted()` 的校验
    ///
    /// 【下一步】外部系统处理后会调用 `acknowledge_notification` 或 `fail_notification`。
    async fn mark_notification_submitted(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(&lease.outbox_id.to_string())
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        record
            .mark_submitted(lease, now_unix_ms)
            .map_err(PortError::Conflict)?;
        Ok(record.clone())
    }

    // =========================================================================
    // acknowledge_notification: 外部系统确认已收到
    // =========================================================================

    /// 外部系统回复说「我收到了这个通知」。
    ///
    /// 【流程】
    /// - 状态从 `Submitted` 转为 `Delivered`
    /// - 保存外部系统返回的 `DeliveryReceipt`（时间戳、外部事务 ID 等）
    ///
    /// 【不幂等性提示】
    /// - 同一个 lease 多次 acknowledge 应该拒绝（由 domain 的 `record.acknowledge()` 实现）
    /// - 如果 worker 和外部系统之间网络分割了，可能看起来像「不知道状态」
    ///   这种情况用 `reconcile_notification_unknown` 处理
    async fn acknowledge_notification(
        &self,
        lease: &NotificationOutboxLease,
        receipt: &kiana_domain::DeliveryReceipt,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(&lease.outbox_id.to_string())
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        record
            .acknowledge(lease, receipt, now_unix_ms)
            .map_err(PortError::Conflict)?;
        Ok(record.clone())
    }

    // =========================================================================
    // fail_notification: 发送失败，回到 Pending 重试
    // =========================================================================

    /// 标记一个通知的发送失败。
    ///
    /// 【情景】
    /// - MCP 连接超时
    /// - Webhook 返回 5xx
    /// - 邮件网关不响应
    ///
    /// 【操作】
    /// - 状态从 `Submitted` 转回 `Pending`
    /// - 清除 lease 信息，这样 `claim_next_notification` 能再次领取
    /// - 可以设置重试计数或延迟（由 domain 类型实现）
    ///
    /// 【和 `reconcile_notification_unknown` 的区别】
    /// - `fail_notification`: 我确定它失败了
    /// - `reconcile_notification_unknown`: 我不知道它是否成功了
    async fn fail_notification(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(&lease.outbox_id.to_string())
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        record
            .fail(lease, now_unix_ms)
            .map_err(PortError::Conflict)?;
        Ok(record.clone())
    }

    // =========================================================================
    // reconcile_notification_unknown: 不确定性恢复
    // =========================================================================

    /// 处理「我不知道结果」的情况。
    ///
    /// 【场景】
    /// - Worker 发送了通知后立即崩溃
    /// - 外部系统可能收到了，也可能没有
    /// - Outbox lease 过期了，没人知道实际状态
    ///
    /// 【恢复策略】（由 domain 实现）
    /// - 如果 lease 超期很久了，假设「可能已成功」，标记为已投递
    /// - 或者直接 retry（取决于外部系统是否支持幂等）
    ///
    /// 【错误处理】
    /// - 如果 lease 已过期很长时间（超过合理的重试窗口），
    ///   返回 `PortError::Failed("result_unknown:...")`
    /// - 其他校验错误返回 `PortError::Conflict`
    async fn reconcile_notification_unknown(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(&lease.outbox_id.to_string())
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        record
            .reconcile_unknown(lease, now_unix_ms)
            .map_err(|error| {
                if error.contains("expired") {
                    PortError::Failed(format!("result_unknown:{error}"))
                } else {
                    PortError::Conflict(error)
                }
            })?;
        Ok(record.clone())
    }
}
