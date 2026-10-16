//! CI-semantics notification intent deduplication.
//!
//! The map is deliberately an in-process adapter. Its mutex demonstrates the semantic atomicity
//! required by NM-07, but it does not claim restart, durable or cross-process proof. The adapter
//! stops at the notification intent boundary; outbox insertion and channel delivery belong to a
//! later step.

use async_trait::async_trait;
use kiana_domain::{NotificationDedupRecord, NotificationDedupRequest};
use kiana_ports::{NotificationDedupOutcome, NotificationDedupStore, PortError};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// ===== 通知去重状态结构 =====
// 使用 BTreeMap 存储去重记录，以支持：
// 1. 按 dedup_key 有序存储和查询
// 2. CI 环境中的确定性结果
// 3. 快速的 O(log n) 查找和更新
#[derive(Clone, Debug, Default)]
struct NotificationDedupState {
    // 去重记录表：dedup_key -> 通知记录
    // 每条记录包含内容摘要、订阅版本、收据和版本号
    records: BTreeMap<String, NotificationDedupRecord>,
}

/// ===== 内存通知去重存储实现 =====
/// 作用：供 CI 和确定性测试使用的非持久化去重引擎。
///
/// 核心职责：
/// 1. 防止重复发送相同的通知（基于 content_digest）
/// 2. 管理通知的生命周期版本（乐观锁模式）
/// 3. 支持"索赔"语义：首次声明时创建，重复相同内容时返回已有记录
///
/// 限制说明：
/// - 内存存储在进程重启后丢失
/// - 无跨进程同步保证（单进程 CI 适用）
/// - 实际发送由 notification_outbox 负责（这里仅做意图去重）
#[derive(Clone, Debug, Default)]
pub struct MemoryNotificationDedupStore {
    state: Arc<Mutex<NotificationDedupState>>,
}

impl MemoryNotificationDedupStore {
    pub fn new() -> Self {
        Self::default()
    }

    // ===== 验证去重键有效性 =====
    // 去重键的格式要求：
    // 1. 非空且非空白
    // 2. 长度 ≤ 512 字符（防止过大的键值）
    // 3. 不含空字符、换行符等控制字符（保证可存储和查询）
    fn validate_key(key: &str) -> Result<(), PortError> {
        if key.trim().is_empty() || key.len() > 512 || key.contains(['\0', '\n', '\r']) {
            return Err(PortError::Failed(
                "notification_dedup_key_invalid".to_owned(),
            ));
        }
        Ok(())
    }

    // 便捷函数：生成一致的冲突错误格式
    fn conflict(reason: &'static str) -> PortError {
        PortError::Conflict(reason.to_owned())
    }
}

#[async_trait]
impl NotificationDedupStore for MemoryNotificationDedupStore {
    // ===== 声明并去重通知 =====
    // 根据去重键和内容摘要，判断是否需要发送新通知，或返回已有的记录。
    //
    // 处理流程：
    // 1. 验证请求的完整性和去重键的有效性
    // 2. 检查是否存在相同的已声明记录：
    //    - 若存在且内容摘要匹配 → 返回 Replayed（重放现有记录）
    //    - 若存在但内容/版本不匹配 → 返回错误（冲突）
    // 3. 若不存在，创建新的去重记录
    // 4. 幂等性保证：重复相同请求返回相同结果
    //
    // 返回值：
    // - Claimed: 新创建的记录（首次声明）
    // - Replayed: 已存在的记录（重复声明相同内容）
    async fn claim_notification(
        &self,
        request: NotificationDedupRequest,
    ) -> Result<NotificationDedupOutcome, PortError> {
        // 第一步：验证请求的数据完整性
        request
            .validate()
            .map_err(|error| PortError::Failed(format!("notification_dedup_request:{error}")))?;
        Self::validate_key(&request.dedup_key)?;

        let mut state = self.state.lock().await;

        // 第二步：检查是否已存在该去重键的记录
        if let Some(existing) = state.records.get(&request.dedup_key) {
            // 存在记录时，需验证三个关键字段是否一致
            if existing.content_digest != request.content_digest {
                return Err(Self::conflict("notification_dedup_content_conflict"));
            }
            if existing.subscription_revision != request.subscription_revision {
                return Err(Self::conflict(
                    "notification_dedup_subscription_revision_conflict",
                ));
            }
            // 若调用者指定了期望版本号，检查是否匹配
            if request
                .expected_revision
                .is_some_and(|revision| revision != existing.revision)
            {
                return Err(Self::conflict("notification_dedup_stale_revision"));
            }
            // 所有检查通过，返回已有记录（重放语义）
            return Ok(NotificationDedupOutcome::Replayed(existing.clone()));
        }

        // 第三步：新记录必须不指定 expected_revision（只有 CAS 操作才能指定）
        if request.expected_revision.is_some() {
            return Err(Self::conflict("notification_dedup_expected_record_missing"));
        }

        // 第四步：从请求创建新记录（初始版本号 = 1）
        let record = NotificationDedupRecord::from_request(&request, 1)
            .map_err(|error| PortError::Failed(format!("notification_dedup_record:{error}")))?;
        state.records.insert(request.dedup_key, record.clone());
        Ok(NotificationDedupOutcome::Claimed(record))
    }

    // ===== 比较并交换通知记录（乐观锁更新） =====
    // 原子地更新已存在的记录。使用版本号防止并发冲突。
    //
    // 实现的是 Compare-And-Swap 模式：
    // 1. 验证当前版本号 = expected_revision
    // 2. 若验证成功，将版本号递增并更新内容
    // 3. 若验证失败或记录不存在，返回错误（StaleRevision）
    //
    // 约束条件：
    // 1. 下一版本号 = expected_revision + 1（必须严格递增）
    // 2. 去重键、内容摘要、订阅版本号必须保持不变
    // 3. 通知 ID 必须保持不变（同一通知的多个转移必须用同一 ID）
    async fn compare_and_swap_notification(
        &self,
        dedup_key: &str,
        expected_revision: u64,
        next: NotificationDedupRecord,
    ) -> Result<NotificationDedupRecord, PortError> {
        // 第一步：验证去重键和期望版本号的有效性
        Self::validate_key(dedup_key)?;
        if expected_revision == 0 {
            return Err(PortError::Failed(
                "notification_dedup_expected_revision_invalid".to_owned(),
            ));
        }

        // 第二步：验证新记录的完整性
        next.validate()
            .map_err(|error| PortError::Failed(format!("notification_dedup_record:{error}")))?;
        if next.dedup_key != dedup_key {
            return Err(Self::conflict("notification_dedup_key_conflict"));
        }

        // 第三步：计算下一版本号（防止溢出）
        let next_revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| PortError::Failed("notification_dedup_revision_overflow".to_owned()))?;

        let mut state = self.state.lock().await;

        // 第四步：原子读取当前记录并验证版本号
        let current = state
            .records
            .get(dedup_key)
            .cloned()
            .ok_or_else(|| Self::conflict("notification_dedup_record_missing"))?;
        if current.revision != expected_revision {
            return Err(Self::conflict("notification_dedup_stale_revision"));
        }

        // 第五步：验证新记录的版本号是否符合预期递增
        if next.revision != next_revision {
            return Err(Self::conflict("notification_dedup_next_revision_invalid"));
        }

        // 第六步：验证不可变字段保持一致性
        // 内容摘要和订阅版本号不能改变（这些是去重的依据）
        if next.content_digest != current.content_digest
            || next.subscription_revision != current.subscription_revision
        {
            return Err(Self::conflict("notification_dedup_content_conflict"));
        }
        // 通知 ID 必须保持一致（同一条通知的所有状态转移）
        if next.notification.notification_id != current.notification.notification_id {
            return Err(Self::conflict(
                "notification_dedup_notification_identity_conflict",
            ));
        }

        // 第七步：原子地更新记录
        state.records.insert(dedup_key.to_owned(), next.clone());
        Ok(next)
    }
}
