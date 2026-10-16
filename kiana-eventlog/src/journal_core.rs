//! EventStore adapter 共用的事务规划、索引更新和完整帧重放逻辑。
//!
//! 【本模块在系统中的位置】
//! ```text
//! MemoryEventLog / JsonlEventLog 收到 EventStorePort 操作
//!       ↓
//! JournalState::plan_transition / plan_legacy 只计算“能否提交”及回执
//!       ↓ adapter 写盘或在内存锁内确认计划
//! JournalState::apply_transition / apply_legacy 更新事件索引
//!       ↓
//! 查询按 RequestId、聚合流或 commit-boundary cursor 返回事件
//! ```
//!
//! 把“计划”与“应用”分开，是为了让 JSONL adapter 能先将一个完整事务帧写入并同步，再让
//! 缓存看见新事件；内存 adapter 则在同一临界区内立即应用。两种 adapter 共用这里的冲突、
//! 重放、幂等、流版本和分页规则，避免同一 EventStorePort 在不同存储介质上语义不一致。
//! 本模块只维护一个可重建的索引视图；JSONL 模式下文件是跨重启输入，内存模式下状态随进程消失。
use crate::event_store_core::{
    plan_append, plan_idempotent_append, validate_event_for_storage, AppendPlan,
};
use kiana_domain::*;
use kiana_ports::{EventAppendResult, PortError};
use std::collections::{BTreeMap, HashMap, HashSet};

/// EventStore 已接受事件的内存索引。
///
/// JsonlEventLog 打开或追平文件时由帧重放重建；MemoryEventLog 则在当前进程中直接持有它。
/// 所有字段都由 `index_event` / `apply_*` 一起维护，不应从单独索引反推比事件本身更强的事实。
#[derive(Clone, Debug, Default)]
pub(crate) struct JournalState {
    /// 按提交顺序保存的逻辑事件，分页和全量查询的基础数据。
    pub events: Vec<RuntimeEvent>,
    /// 按 command_id 查找已提交回执，用于防重放和 Unknown 结果恢复。
    pub commands: HashMap<RequestId, CommandReceipt>,
    /// 每个 `(aggregate_type, aggregate_id)` 当前观察到的 stream_version。
    versions: BTreeMap<(String, String), u64>,
    /// 已见事件 ID，用于阻止不同提交重复使用同一 ID。
    event_ids: HashSet<EventId>,
    /// 已见幂等键到事件索引的映射。
    keys: HashMap<String, usize>,
    /// 聚合流键到 `events` 中索引位置的列表。
    streams: HashMap<(String, String), Vec<usize>>,
    /// 请求 ID 到 `events` 中索引位置的列表。
    requests: HashMap<RequestId, Vec<usize>>,
    /// 每次完整事务或兼容单事件追加之后的游标边界；分页只能从这里开始/结束。
    boundaries: Vec<u64>,
}
/// `plan_transition` 的纯计划结果；生成此值尚未改变 EventStore 状态。
#[derive(Debug)]
pub(crate) enum TransitionPlan {
    /// 新命令通过所有校验，包含待追加 batch 及预先计算的回执。
    Append {
        batch: TransitionBatch,
        receipt: CommandReceipt,
    },
    /// 相同 command_id 和摘要此前已提交；复用已有回执，不再追加事件。
    Replay(CommandReceipt),
    /// 一个或多个聚合当前版本与 batch 预期版本不同，携带实际版本供调用方重算/报告冲突。
    Conflict(Vec<AggregateVersion>),
}
fn invalid(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}
fn conflict(reason: &str) -> PortError {
    PortError::Conflict(reason.into())
}
impl JournalState {
    /// 基于当前内存索引规划一笔原子转换，不修改 `self`。
    ///
    /// 【调用者】MemoryEventLog 和 JsonlEventLog 的 `commit_transition`，以及 JSONL 重放。
    /// 【输入】`batch` 是 Core 给出的事件和预期版本集合；`commit_id` 是新回执拟使用的提交标识。
    /// 【输出】返回 Append（需由 adapter 真正提交后再调用 `apply_transition`）、Replay（原回执）
    /// 或 Conflict（实际变化版本）。
    /// 【核心流程】先校验身份和事件形状；检查同 command 是否已存在；比较 read set 当前版本；
    /// 检查事件/幂等键唯一性及每个 stream_version 连续递增；最后构造并校验 receipt/frame 大小。
    /// 【失败情况】数据格式、大小和重复 ID 等返回结构化 PortError；版本并发冲突是正常的
    /// `TransitionPlan::Conflict`，不是可当成功处理的异常。函数自身不会写盘或增加事件。
    ///
    /// ⚠ 不能在 adapter 尚未确认 Append 提交前调用 `apply_transition`：JSONL 需要先写并同步帧，
    /// 否则内存缓存会先于文件显示出一个重启后消失的“提交成功”。
    pub fn plan_transition(
        &self,
        batch: TransitionBatch,
        commit_id: EventId,
    ) -> Result<TransitionPlan, PortError> {
        batch.validate_identity().map_err(invalid)?;
        for event in &batch.events {
            validate_event_for_storage(event)?;
        }
        if let Some(original) = self.commands.get(&batch.command_id) {
            if original.command_digest != batch.command_digest {
                return Err(conflict("event_store_command_digest_mismatch"));
            }
            return Ok(TransitionPlan::Replay(original.clone()));
        }
        batch.validate().map_err(invalid)?;
        if self.events.len().saturating_add(batch.events.len()) > MAX_JOURNAL_EVENTS {
            return Err(invalid("eventlog_event_limit"));
        }
        let mut changed = Vec::new();
        let mut versions = BTreeMap::new();
        for expected in &batch.expected_versions {
            let key = (
                expected.aggregate_type.clone(),
                expected.aggregate_id.clone(),
            );
            let actual = self.versions.get(&key).copied().unwrap_or(0);
            if actual != expected.version {
                changed.push(AggregateVersion::new(&key.0, &key.1, actual));
            }
            versions.insert(key, actual);
        }
        if !changed.is_empty() {
            changed.sort();
            return Ok(TransitionPlan::Conflict(changed));
        }
        let mut new_ids = HashSet::new();
        let mut new_keys = HashSet::new();
        for event in &batch.events {
            if self.event_ids.contains(&event.event_id) || !new_ids.insert(event.event_id) {
                return Err(conflict("event_id_duplicate"));
            }
            if let Some(key) = &event.idempotency_key {
                if self.keys.contains_key(key) || !new_keys.insert(key) {
                    return Err(conflict("event_idempotency_key_duplicate"));
                }
            }
            let key = (
                event.aggregate_type.clone().expect("validated aggregate"),
                event.aggregate_id.clone().expect("validated aggregate"),
            );
            let version = versions.get_mut(&key).expect("validated read set");
            *version = version
                .checked_add(1)
                .ok_or_else(|| invalid("event_stream_version_exhausted"))?;
            if event.stream_version != Some(*version) {
                return Err(conflict("event_stream_version_not_contiguous"));
            }
        }
        let receipt = CommandReceipt {
            command_id: batch.command_id,
            command_digest: batch.command_digest.clone(),
            commit_id,
            first_cursor: self.events.len() as u64 + 1,
            cursor: (self.events.len() + batch.events.len()) as u64,
            event_ids: batch.events.iter().map(|e| e.event_id).collect(),
            versions: versions
                .into_iter()
                .map(|((kind, id), version)| AggregateVersion::new(kind, id, version))
                .collect(),
        };
        // 内存和 JSONL adapter 都在暴露任何事务事件前执行同一完整帧编码上限检查，避免
        // 一个 adapter 接受、另一个 adapter 因帧过大而无法重放同一 batch。
        JournalFrame::new(JournalFramePayload::Transition {
            batch: batch.clone(),
            receipt: receipt.clone(),
        })
        .map_err(invalid)?;
        Ok(TransitionPlan::Append { batch, receipt })
    }
    /// 将 adapter 已确认提交的 batch 和 receipt 原子地折叠进内存索引。
    ///
    /// 本方法假设调用方已完成 `plan_transition` 和实际 adapter 边界；它不再次做冲突校验，也
    /// 不写磁盘。调用方须持有对 JournalState 的独占访问；JSONL 写入成功后、MemoryEventLog 在
    /// 持锁的计划成功分支中调用它。事件索引、事务边界和 command receipt 必须一起更新，少一步
    /// 都会让查询/恢复视图不一致。
    pub fn apply_transition(&mut self, batch: TransitionBatch, receipt: CommandReceipt) {
        for event in batch.events {
            self.index_event(event);
        }
        self.boundaries.push(self.events.len() as u64);
        self.commands.insert(receipt.command_id, receipt);
    }
    /// 规划旧式逐事件 append 的兼容操作，不修改索引。
    ///
    /// `expected` 有值时检查目标事件流的预期版本；`idempotent` 决定是否根据事件幂等键返回已有事件。
    /// 该路径保留旧调用者语义，不提供多聚合 `TransitionBatch` 的原子状态转换。无效 ID、空
    /// 类型/序号、过大事件、冲突游标、重复幂等键或事件总量超限均返回错误。
    pub fn plan_legacy(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
        idempotent: bool,
    ) -> Result<AppendPlan, PortError> {
        if event.event_id.as_uuid().is_nil() || event.request_id.as_uuid().is_nil() {
            return Err(invalid("event_identity_invalid"));
        }
        if event.sequence == 0 || event.kind.is_empty() {
            return Err(invalid("eventlog_event_invalid"));
        }
        if canonical_journal_bytes(&event).map_err(invalid)?.len() > MAX_JOURNAL_EVENT_BYTES {
            return Err(invalid("eventlog_event_size_limit"));
        }
        let plan = if idempotent {
            plan_idempotent_append(&self.events, event, expected)?
        } else {
            AppendPlan::Append(plan_append(&self.events, event, expected)?)
        };
        if matches!(plan, AppendPlan::Append(_)) && self.events.len() >= MAX_JOURNAL_EVENTS {
            return Err(invalid("eventlog_event_limit"));
        }
        if let AppendPlan::Append(event) = &plan {
            if let Some(key) = &event.idempotency_key {
                if self.keys.contains_key(key) {
                    return Err(conflict("event_idempotency_key_duplicate"));
                }
            }
        }
        Ok(plan)
    }
    /// 将单条已接受的兼容事件加入索引并记录一个独立提交边界。
    ///
    /// 调用者负责先通过 `plan_legacy`，再确认实际追加；不要用它应用事务 batch 中的一部分事件。
    pub fn apply_legacy(&mut self, event: RuntimeEvent) {
        self.index_event(event);
        self.boundaries.push(self.events.len() as u64);
    }
    /// 同步维护事件主列表及所有辅助索引；传入事件会成为列表和索引中的同一逻辑事实。
    fn index_event(&mut self, event: RuntimeEvent) {
        let index = self.events.len();
        self.event_ids.insert(event.event_id);
        if let Some(key) = &event.idempotency_key {
            self.keys.insert(key.clone(), index);
        }
        self.requests
            .entry(event.request_id)
            .or_default()
            .push(index);
        if let (Some(kind), Some(id)) = (&event.aggregate_type, &event.aggregate_id) {
            let key = (kind.clone(), id.clone());
            self.versions
                .insert(key.clone(), event.stream_version.unwrap_or(event.sequence));
            self.streams.entry(key).or_default().push(index);
        }
        self.events.push(event);
    }
    /// 校验并重放一个已解码的 journal frame。
    ///
    /// 先运行 `JournalFrame::validate`，然后对事务 body 重新规划并要求帧内 receipt 与本地
    /// 重新计算的 receipt 完全一致；单事件 body 则经兼容追加规则检查。任一事件、版本或回执
    /// 与此前状态冲突都会停止重放，避免从后续记录继续构造一个无法解释的日志视图。
    pub fn accept_frame(&mut self, frame: JournalFrame) -> Result<(), PortError> {
        frame.validate().map_err(invalid)?;
        match frame.body {
            JournalFramePayload::Transition { batch, receipt } => {
                match self.plan_transition(batch, receipt.commit_id)? {
                    TransitionPlan::Append {
                        batch,
                        receipt: expected,
                    } if receipt == expected => {
                        self.apply_transition(batch, receipt);
                        Ok(())
                    }
                    _ => Err(invalid("eventlog_transaction_replay_conflict")),
                }
            }
            JournalFramePayload::Event { event } => {
                let AppendPlan::Append(event) = self.plan_legacy(event, None, false)? else {
                    return Err(invalid("eventlog_legacy_replay_conflict"));
                };
                self.apply_legacy(event);
                Ok(())
            }
        }
    }
    /// 返回属于某个入口 RequestId 的事件副本，顺序与全局提交事件顺序一致。
    pub fn request(&self, id: &RequestId) -> Vec<RuntimeEvent> {
        self.requests
            .get(id)
            .into_iter()
            .flatten()
            .map(|i| self.events[*i].clone())
            .collect()
    }
    /// 返回指定聚合流中的事件副本；聚合键由类型和 ID 共同组成。
    pub fn stream(&self, kind: &str, id: &str) -> Vec<RuntimeEvent> {
        self.streams
            .get(&(kind.into(), id.into()))
            .into_iter()
            .flatten()
            .map(|i| self.events[*i].clone())
            .collect()
    }
    /// 按逻辑事件游标读取完整提交边界之间的一页数据。
    ///
    /// `cursor` 是调用方已消费的位置，0 表示开头；非零值必须正好位于一个提交边界，且不能
    /// 大于当前事件总数。`limit` 限制通常的返回事件数，但若页首的完整事务本身更大，会先整笔
    /// 返回，避免把原子 batch 拆开。成功的 `JournalPage.cursor` 是新位置，后续页从它继续；
    /// 非法游标返回冲突、limit 为 0 或超过最大页大小返回 invalid。
    pub fn page(&self, cursor: u64, limit: usize) -> Result<JournalPage, PortError> {
        if limit == 0 || limit > MAX_JOURNAL_PAGE_EVENTS {
            return Err(invalid("eventlog_page_limit"));
        }
        if cursor > self.events.len() as u64
            || (cursor != 0 && self.boundaries.binary_search(&cursor).is_err())
        {
            return Err(conflict("eventlog_cursor_not_commit_boundary"));
        }
        let mut end = cursor;
        let start = self
            .boundaries
            .partition_point(|boundary| *boundary <= cursor);
        for boundary in self.boundaries[start..].iter().copied() {
            if end != cursor && boundary - cursor > limit as u64 {
                break;
            }
            end = boundary;
            if end - cursor >= limit as u64 {
                break;
            }
        }
        Ok(JournalPage {
            events: self.events[cursor as usize..end as usize].to_vec(),
            cursor: end,
            has_more: end < self.events.len() as u64,
        })
    }
}
pub(crate) fn append_result(event: RuntimeEvent, replayed: bool) -> EventAppendResult {
    EventAppendResult { event, replayed }
}
pub(crate) fn capabilities(durable: bool) -> EventStoreCapabilities {
    EventStoreCapabilities {
        atomic_transitions: true,
        durable_commits: durable,
        command_receipts: true,
        cursor_reads: true,
        writer_format_version: JOURNAL_WRITER_VERSION,
        max_frame_bytes: MAX_JOURNAL_FRAME_BYTES,
        max_batch_events: MAX_TRANSITION_EVENTS,
    }
}
