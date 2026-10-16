//! # 文件职责
//!
//! 本文件提供 `EventStorePort` 的装饰器（decorator）：它把提交操作转交给真正的存储 adapter，
//! 并在底层确认一个新的原子转换后，向注册的 `EventStoreCommitObserver` 顺序发送通知。
//! 通知用于唤醒观察者读取或更新自己的临时投影，不是 EventLog 的第二个写入入口，也不替代
//! 持久事件本身。
//!
//! # 在系统中的位置与边界
//!
//! ```text
//! 调用方
//!   -> StreamEventStore::commit_transition(batch)
//!   -> inner EventStorePort::commit_transition(batch)
//!   -> Committed：更新本实例内存 cursor，再依次 await observer.on_committed()
//!   -> 原样返回底层 CommitOutcome
//! ```
//!
//! 只有 `commit_transition()` 会发通知；`append()`、`append_expected()`、幂等追加和所有读取/健康
//! 方法都直接转发给 `inner`，不会经过观察者广播。底层错误、`Conflict`、`Replayed` 和 `Unknown`
//! 结果也不会广播；它们没有被本装饰器观察到的新提交。
//!
//! 底层 adapter 返回 `Committed` 才是这里的通知边界；具体 adapter 可以是内存存储，因此不能把
//! 这个状态一概说成“已写入磁盘”。通知是易失、尽力而为的旁路信号：观察者失败不会撤销已提交
//! 事实，进程重启后的恢复仍必须从 EventStore 的 `read_from()` 和自身持久游标继续扫描。
//!
//! 【当前调用关系】Daemon 的实时 UI Run 流由 `kiana-daemon::run_stream::StreamEventStore` 和
//! `RunStreamBus` 处理；它不是这里的 `kiana-eventlog::StreamEventStore`。当前 workspace 中，这个
//! observer 接口的注册/实现只在 `kiana-eventlog` 测试中直接出现，尚未找到生产观察者。不要把
//! 两个同名 wrapper 的能力或调用关系混为一谈。
//!
//! 【重要限制】成功提交后的 `commit_transition()` 会等待本方法依次完成所有 observer 回调；
//! 一个回调很慢会延迟提交调用方收到返回值，且本文件没有回调超时。回调只能观察已提交事实，
//! 不参与授权，也不能经此 trait 调用 Broker 执行副作用。

use async_trait::async_trait;
use kiana_domain::{CommitOutcome, EventStoreCapabilities, JournalPage, RequestId, RuntimeEvent};
use kiana_ports::{
    CommitObserverFailure, CommittedTransition, EventAppendResult, EventStoreCommitObserver,
    EventStorePort, PortError,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

const MAX_COMMIT_OBSERVERS: usize = 32;
const MAX_OBSERVER_FAILURES: usize = 256;

// ============================================================================
// StreamEventStore：为 EventStorePort 增加 commit observer 的装饰器
// ============================================================================

/// EventStore 装饰器，为原子转换提交添加观察者通知机制。
///
/// 【字段含义】`inner` 是真正负责读写的 EventStore adapter；`observers` 是受互斥锁保护的
/// callback 列表；`failures` 保存最近至多 256 条通知失败诊断；`committed_cursor` 是本装饰器
/// 实例创建后观察到的最大提交 cursor，只用于本进程中的提示。
///
/// 包装保留底层 commit outcome 和事件内容，但成功的 `commit_transition()` 会额外等待观察者回调；
/// 因此调用延迟会变化。该类型不替底层提供耐久性，也不把观察者失败转换成提交失败。
pub struct StreamEventStore {
    inner: Arc<dyn EventStorePort>,
    observers: Mutex<Vec<Arc<dyn EventStoreCommitObserver>>>,
    failures: Mutex<Vec<CommitObserverFailure>>,
    committed_cursor: AtomicU64,
}

impl std::fmt::Debug for StreamEventStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let observers = self
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        formatter
            .debug_struct("StreamEventStore")
            .field("observers", &observers)
            .field("failure_count", &self.observer_failures().len())
            .finish_non_exhaustive()
    }
}

impl StreamEventStore {
    // =========================================================================
    // 构造与注册
    // =========================================================================

    /// 用指定的 EventStore 创建一个新的观察者装饰器。
    ///
    /// 【输入】`inner` 是唯一执行实际 EventStore 操作的对象。
    /// 【初始化】观察者列表和诊断列表为空，进程内游标从 0 开始；不会查询底层现有 cursor，
    /// 因此已运行一段时间的底层存储不会自动反映在 `committed_cursor()` 中。
    /// 【副作用】只创建内存 wrapper，不写入任何事件。
    pub fn new(inner: Arc<dyn EventStorePort>) -> Self {
        Self {
            inner,
            observers: Mutex::new(Vec::new()),
            failures: Mutex::new(Vec::new()),
            committed_cursor: AtomicU64::new(0),
        }
    }

    /// 包装并返回共享所有权的 Arc。
    ///
    /// `Arc`（原子引用计数指针）允许多个需要 `EventStorePort` 的组件共同持有同一个 wrapper；
    /// 它不复制底层存储，也不代表 wrapper 的内存状态能跨进程保存。
    pub fn wrap(inner: Arc<dyn EventStorePort>) -> Arc<Self> {
        Arc::new(Self::new(inner))
    }

    /// 创建装饰器并立即注册一个观察者。
    ///
    /// 【作用】创建 wrapper 后立即登记一个观察者，避免调用方分开管理构造与注册两步。
    /// 【失败】注册表锁中毒或容量已满时返回对应 PortError；不会暴露半注册状态。
    pub fn with_observer(
        inner: Arc<dyn EventStorePort>,
        observer: Arc<dyn EventStoreCommitObserver>,
    ) -> Result<Arc<Self>, PortError> {
        let store = Arc::new(Self::new(inner));
        store.register_observer(observer)?;
        Ok(store)
    }

    /// 向观察者列表添加一个新观察者。
    ///
    /// 【限制】最多 32 个观察者。每个新提交都要顺序等待每个 observer，固定上限限制单次提交
    /// 可能触发的回调数量；需要大量订阅者时应采用有独立队列和消费节奏的广播设施。
    ///
    /// 【错误】
    /// - `eventlog_observer_registry_poisoned`: 互斥锁被毒化（panic 后遗留）
    /// - `eventlog_observer_limit`: 已有 32 个观察者，拒绝新增
    pub fn register_observer(
        &self,
        observer: Arc<dyn EventStoreCommitObserver>,
    ) -> Result<(), PortError> {
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| PortError::Failed("eventlog_observer_registry_poisoned".to_owned()))?;
        if observers.len() >= MAX_COMMIT_OBSERVERS {
            return Err(PortError::Conflict("eventlog_observer_limit".to_owned()));
        }
        observers.push(observer);
        Ok(())
    }

    // =========================================================================
    // 光标与诊断
    // =========================================================================

    /// 获取观察者失败的诊断历史。
    ///
    /// 【设计】这是一个环形缓冲区（最多 256 条），用于调试。
    /// 如果观察者频繁失败，会过期旧的失败记录，只保留最近的。
    pub fn observer_failures(&self) -> Vec<CommitObserverFailure> {
        self.failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// 返回本 wrapper 自创建以来观察到的最大提交光标。
    ///
    /// 【注意】这**不是**一个持久化的检查点。
    /// `fetch_max` 避免并发提交的返回顺序使数值倒退；这个数值不是底层全局游标、持久化检查点，
    /// 也没有涵盖通过其他 EventStore handle 或 `append*` 方法完成的写入。投影**必须**在重启时
    /// 用 `read_from` 从自己的持久检查点扫描。
    ///
    /// 【为什么不能是检查点】
    /// - 这个值只在进程内更新，进程结束就消失
    /// - 观察者回调可能被丢弃或失败，真实的提交不会丢
    /// - 如果把它当真相，重启就会丢事件
    pub fn committed_cursor(&self) -> kiana_domain::EventCursor {
        self.committed_cursor.load(Ordering::SeqCst)
    }

    // =========================================================================
    // 内部：失败记录与通知
    // =========================================================================

    /// 记录一个观察者失败（内部方法）。
    ///
    /// 【策略】环形缓冲区。当失败记录达到 256 条时，
    /// 移除最老的一条再加入新的，这样保持诊断数据不会无限增长。
    fn record_failure(&self, failure: CommitObserverFailure) {
        let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        if failures.len() >= MAX_OBSERVER_FAILURES {
            failures.remove(0);
        }
        failures.push(failure);
    }

    /// 通知所有观察者有新的提交。
    ///
    /// 【执行顺序】
    /// 1. 构建 `CommittedTransition`（包含完整的批次和收据）
    /// 2. 克隆观察者列表（避免持有锁太久）
    /// 3. 逐个 await 每个观察者；新增注册者从下一次克隆 observer 列表后才会收到通知
    /// 4. 如果某个观察者失败，记录诊断但不中止其他的
    ///
    /// 【幂等性】
    /// - 这个方法被调用后，底层 EventStore 已经提交
    /// - 即使所有观察者都失败，事实已经保存
    /// - 观察者的职责是去查询或投影，而不是决定事实
    ///
    /// ⚠ 不要在观察者回调里等待一个必须等当前 `commit_transition()` 返回才能发生的操作，
    /// 否则会形成等待环；任何长时间挂起的回调都会阻塞该提交调用的返回。
    async fn notify_committed(
        &self,
        batch: kiana_domain::TransitionBatch,
        receipt: kiana_domain::CommandReceipt,
    ) {
        let command_id = batch.command_id;
        let commit_id = receipt.commit_id;
        let source_cursor = receipt.cursor;
        let notification = match CommittedTransition::new(batch, receipt) {
            Ok(notification) => notification,
            Err(error) => {
                // 底层已经提交。这是一个不变式违反。
                // 保持这个事实，但把检查失败记为诊断而不是伪造一个拒绝。
                self.record_failure(CommitObserverFailure {
                    command_id,
                    commit_id,
                    source_cursor,
                    reason: format!("eventlog_commit_notification_invalid:{error}"),
                });
                return;
            }
        };
        let observers = self
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for observer in observers {
            if let Err(error) = observer.on_committed(notification.clone()).await {
                self.record_failure(CommitObserverFailure {
                    command_id: notification.batch.command_id,
                    commit_id: notification.receipt.commit_id,
                    source_cursor: notification.source_cursor(),
                    reason: error.to_string(),
                });
            }
        }
    }
}

/// 向后兼容别名。某些调用者不使用「StreamEventStore」这个历史名称。
pub type CommitObservedEventStore = StreamEventStore;

// ============================================================================
// EventStorePort implementation: 代理所有操作给内部实现
// ============================================================================

#[async_trait]
impl EventStorePort for StreamEventStore {
    // 以下方法完全代理给 `inner`，不做任何修改。
    // 只有 `commit_transition` 会在成功时通知观察者。

    fn supports_atomic_transitions(&self) -> bool {
        self.inner.supports_atomic_transitions()
    }

    fn capabilities(&self) -> EventStoreCapabilities {
        self.inner.capabilities()
    }

    async fn flush(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.flush().await
    }

    async fn health(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.health().await
    }

    async fn last_durable_cursor(&self) -> Result<kiana_domain::EventCursor, PortError> {
        self.inner.last_durable_cursor().await
    }

    async fn close(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.close().await
    }

    /// 【关键方法】把原子转换委托给底层，只有首次确认提交后才通知观察者。
    ///
    /// 【流程】
    /// 1. 调用 `inner.commit_transition(batch)`
    /// 2. 等待底层返回结果
    /// 3. 如果结果是 `CommitOutcome::Committed`：
    ///    - 更新 `committed_cursor`（用原子操作更新为最大值）
    ///    - 调用 `notify_committed()` 向所有观察者广播
    /// 4. 返回原始结果（不因为观察者失败而改变）
    ///
    /// 【幂等性保证】
    /// - `Conflict`、`Replayed` 和 `Unknown` 不会触发通知；只有 `Committed` 有收据并代表本次
    ///   wrapper 观察到的新提交
    /// - 即使所有观察者都失败，返回值还是 `Committed`
    /// - 观察者的失败不影响事实的存储
    /// - 若底层 `commit_transition()` 返回 PortError，本方法直接传播错误，不更新 cursor，也不通知
    ///
    /// 注意：底层提交成功后才开始 observer callback；callback 错误仅保存诊断，callback 等待时间
    /// 却包含在本方法返回时间内。此类错误不能当作重新提交 batch 的理由。
    async fn commit_transition(
        &self,
        batch: kiana_domain::TransitionBatch,
    ) -> Result<CommitOutcome, PortError> {
        let outcome = self.inner.commit_transition(batch.clone()).await?;
        if let CommitOutcome::Committed { receipt } = &outcome {
            self.committed_cursor
                .fetch_max(receipt.cursor, Ordering::SeqCst);
            self.notify_committed(batch, receipt.clone()).await;
        }
        Ok(outcome)
    }

    async fn read_command(
        &self,
        id: &RequestId,
    ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
        self.inner.read_command(id).await
    }

    async fn read_from(
        &self,
        cursor: kiana_domain::EventCursor,
        limit: usize,
    ) -> Result<JournalPage, PortError> {
        self.inner.read_from(cursor, limit).await
    }

    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }

    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        self.inner.append_expected(event, expected_version).await
    }

    async fn append_idempotent(&self, event: RuntimeEvent) -> Result<EventAppendResult, PortError> {
        self.inner.append_idempotent(event).await
    }

    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        self.inner
            .append_idempotent_expected(event, expected_version)
            .await
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(request_id).await
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_all().await
    }

    async fn read_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_stream(aggregate_type, aggregate_id).await
    }
}
