//! 进程内事件存储 adapter（适配器），实现 `EventStorePort` 的同一原子转换契约。
//!
//! 【本模块在系统中的位置】
//! ```text
//! DaemonHost / 测试装配代码选择 EventStore 实现
//!       ↓ 注入 Arc<dyn EventStorePort>
//! MemoryEventLog（本文件）保存本进程中的 JournalState
//!       ↓
//! Core 按 command ID、逻辑游标、RequestId 或聚合流读取事件
//! ```
//!
//! 本实现把共用的 `JournalState` 放在 Tokio `RwLock` 内：写锁保护计划与应用过程，读锁保护
//! 查询快照。事务校验、冲突、重放、事件顺序和分页边界与 `JsonlEventLog` 共用
//! `journal_core.rs`；差别在于这里不写文件、不跨重启恢复，也不协调另一个进程。
//!
//! `DaemonHost::local()` 装配 JSONL 日志；显式注入或不带外部 EventStore 的构造路径可以使用
//! 本实现，例如测试和短生命周期的本地流程。只要调用方需要重启后保留事实，就必须选择具备
//! 相应持久化能力且经过验证的 adapter。`MemoryEventLog` 返回的 Committed 只说明当前进程内
//! 的原子状态更新完成，不是跨进程 durable 回执。

use crate::event_store_core::{validate_idempotency_key, AppendPlan};
use crate::journal_core::{append_result, capabilities, JournalState, TransitionPlan};
use async_trait::async_trait;
use kiana_domain::*;
use kiana_ports::{EventAppendResult, EventStorePort, PortError};
use tokio::sync::RwLock;

/// 本进程内的 EventStore adapter。
///
/// `state` 是 `JournalState` 的唯一内存副本：多个只读查询可同时持有读锁，追加和事务提交则
/// 获取独占写锁，使它们之间不会交错。该锁只管当前进程中的并发，不是跨进程锁，也不会把
/// 状态写到磁盘。创建流程是 `new()` → 空的 `JournalState` → 通过 `EventStorePort` 累积事件；
/// 进程结束后这些事件一并释放。
#[derive(Debug, Default)]
pub struct MemoryEventLog {
    /// 共享事务规划器维护的事件主列表、命令回执及按请求/聚合建立的查询索引。
    state: RwLock<JournalState>,
}
impl MemoryEventLog {
    /// 创建一个空的进程内事件日志。
    ///
    /// `RwLock::default()` 内含空 `JournalState`，此时没有事件、命令回执或索引项；返回后可
    /// 立即通过 `EventStorePort` 使用，无需连接文件系统。错误恢复和持久化不属于此构造函数。
    pub fn new() -> Self {
        Self::default()
    }
}
#[async_trait]
impl EventStorePort for MemoryEventLog {
    /// 声明本 adapter 可在当前进程中执行原子转换。
    ///
    /// 【含义】
    /// `true` 表示写锁内的计划会得到 Replayed、Conflict 或 Append 三种语义结果之一，
    /// Append 才应用整个 batch。无效输入仍可能以 `PortError` 失败；原子性只覆盖进程内状态，
    /// 不表示文件持久化或跨进程互斥。
    fn supports_atomic_transitions(&self) -> bool {
        true
    }

    /// 返回本适配器的功能声明。
    ///
    /// 返回共用 journal 语义对应的能力声明，并将持久化能力设为 false。
    ///
    /// 本实现仍声明支持事务、command receipt 和游标读取；`false` 只表示这些提交不跨进程
    /// 持久化。能力对象是 adapter 的声明，不是独立的运行证明。
    fn capabilities(&self) -> EventStoreCapabilities {
        capabilities(false)
    }

    /// 在写锁内计划并应用一个原子事件转换。
    ///
    /// 【调用者】Core 经 `EventStorePort::commit_transition` 提交状态转换时调用。
    /// 【输入】`batch` 携带命令 ID/摘要、expected_versions 和需要一起提交的 RuntimeEvent。
    /// 【输出】`Replayed` 返回已存在的原回执；`Conflict` 返回已变化聚合的实际版本；`Committed`
    /// 返回刚分配的 command receipt。格式错误、重复身份或超限输入则返回 `PortError`。
    /// 【副作用】仅在 `Append` 时把整批事件、事务边界和回执加入当前进程的 JournalState；不写
    /// 文件、不产生跨进程 durable 事实，也不调用 Broker 执行外部副作用。
    /// 【为什么这样设计】同一写锁覆盖 `plan_transition` 与 `apply_transition`，使另一个写者
    /// 不会插入到“检查旧版本”和“更新新版本”之间。即使结果是 Committed，进程退出后状态仍消失。
    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        let mut state = self.state.write().await;
        match state.plan_transition(batch, EventId::new())? {
            TransitionPlan::Replay(original) => Ok(CommitOutcome::Replayed { original }),
            TransitionPlan::Conflict(changed) => Ok(CommitOutcome::Conflict { changed }),
            TransitionPlan::Append { batch, receipt } => {
                state.apply_transition(batch, receipt.clone());
                Ok(CommitOutcome::Committed { receipt })
            }
        }
    }

    /// 按 command ID 查询当前内存日志中的事务回执。
    ///
    /// `Some` 表示此进程状态中已有该命令的提交回执；`None` 只表示本 adapter 当前未找到记录，
    /// 不能代表别的进程或重启前的存储里也没有。内存读锁允许其他查询并行，结果是本次读取时
    /// 的一致快照；本方法不触发重试或业务执行。
    async fn read_command(&self, id: &RequestId) -> Result<Option<CommandReceipt>, PortError> {
        Ok(self.state.read().await.commands.get(id).cloned())
    }

    /// 按逻辑事件游标读取一页，并保持已提交事务的边界。
    ///
    /// 【输入】`cursor` 是调用方已经消费到的位置，0 表示从开头读取；下一页直接传上一页返回的
    /// `JournalPage.cursor`，不加 1。`limit` 是希望读取的事件数上限。
    /// 【输出】按提交顺序返回事件、新游标和 `has_more`。页首的整笔事务大于 limit 时仍会完整
    /// 返回；非法游标（不是提交边界/超过末尾）或不合法 limit 会返回错误。
    /// 【并发】读锁允许多个查询共享状态；写操作等待读锁释放，因此每次返回都来自同一索引快照。
    /// 【数据去向】Core 的恢复、投影或审计调用者可从游标继续扫描；游标是逻辑事件数，不是文件偏移。
    async fn read_from(&self, cursor: u64, limit: usize) -> Result<JournalPage, PortError> {
        self.state.read().await.page(cursor, limit)
    }

    /// 兼容地追加一条事件，不按幂等键识别历史重放。
    ///
    /// 调用仍会经过事件身份、顺序、契约和密钥校验；重复 ID 或 stream version 冲突会报错。
    /// 但若同一事实以新身份再次提交，这个方法不会仅凭内容判断它是重试。调用者只有在明确
    /// 不需要幂等语义时才应使用此入口；需要按请求重放时改用 `append_idempotent`。
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.append_expected(event, None).await
    }

    /// 按兼容单事件路径追加，并可选核对目标流的预期版本。
    ///
    /// `expected` 是当前事件所属聚合流版本（旧式无聚合字段事件则按 request 流比较），不是
    /// 全局文件字节数或磁盘容量。传入后仅当当前流版本相等才追加；不相等返回冲突，不会更新
    /// JournalState。无版本前置条件时继续执行 `plan_legacy` 的身份、顺序和内容规则。
    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<(), PortError> {
        let mut state = self.state.write().await;
        let AppendPlan::Append(event) = state.plan_legacy(event, expected, false)? else {
            unreachable!("non-idempotent append");
        };
        state.apply_legacy(event);
        Ok(())
    }

    /// 使用事件幂等键追加，或返回已有的同一事件。
    ///
    /// 【输入】`RuntimeEvent` 必须带非空 `idempotency_key`。
    /// 【返回】`EventAppendResult.event` 是最终采用的新事件或原事件；`replayed=true` 表示命中
    /// 相同幂等键且内容匹配，本次没有增加第二条记录。相同 key 对应不同 payload 时返回冲突。
    /// 【流程】先验证幂等键，再获取写锁；共享规划器先查已提交键，再检查版本并计划追加。
    /// 这样已成功事件在流版本推进后重试时仍能取回原事件，而不会被错误解释成新写入。
    /// ⚠ 结果只存在当前进程内；幂等保护不等同于持久化，也不应被描述为跨重启有效。
    async fn append_idempotent(&self, event: RuntimeEvent) -> Result<EventAppendResult, PortError> {
        self.append_idempotent_expected(event, None).await
    }

    /// 在幂等追加之外，再核对目标流当前版本。
    ///
    /// 与 `append_idempotent` 相同，需要非空幂等键并比较候选事件与已有同键事件的完整相关字段；
    /// 新事件还必须满足 `expected` 版本约束。幂等键错误、内容冲突或版本不匹配时返回错误且不
    /// 应用事件。幂等命中返回 `replayed=true`；新增事件才更新 JournalState。
    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        validate_idempotency_key(&event)?;
        let mut state = self.state.write().await;
        match state.plan_legacy(event, expected, true)? {
            AppendPlan::Replay(event) => Ok(append_result(event, true)),
            AppendPlan::Append(event) => {
                state.apply_legacy(event.clone());
                Ok(append_result(event, false))
            }
        }
    }

    /// 返回 `request_id` 等于给定 RequestId 的事件副本，按原有追加顺序排列。
    ///
    /// 此查询使用 JournalState 建立的请求索引，不从 UI transcript 重建事件。没有匹配项时返回
    /// 空列表；在本 adapter 中读操作本身不会访问外部存储或触发副作用。
    async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.state.read().await.request(id))
    }

    /// 克隆返回当前内存日志中的全部事件。
    ///
    /// 数据按原追加顺序排列；由于需要复制完整 `Vec`，事件数量较大时会占用与全量数据同阶的
    /// 临时内存。只需要局部范围时使用 `read_from`，以逻辑游标分批读取。
    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.state.read().await.events.clone())
    }

    /// 按聚合类型和聚合 ID 读取该实体事件流。
    ///
    /// 输入的 `kind` 是 `aggregate_type`，`id` 是 `aggregate_id`，二者共同定位一条有版本顺序的
    /// 领域流；它们不是 `event.kind` 和一个通用 `entity_id` 字段。返回列表沿用该流的追加顺序，
    /// 无匹配时为空。调用方继续依赖 `stream_version` 时应使用同一流身份与顺序规则。
    async fn read_stream(&self, kind: &str, id: &str) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.state.read().await.stream(kind, id))
    }
}
