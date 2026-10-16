//! EventLog 读模型的通用内存重放与检查点 helper，属于 `kiana-core`。
//!
//! 【数据流】调用者读取 EventLog 中已提交事件与 snapshot cursor → 选择已有 `ProjectionCheckpoint` 或初始 JSON state →
//! 本模块用调用者提供的 fold 函数逐个折叠未见过的事件 → 返回新的 state/checkpoint → 调用者决定是否持久化或发布。
//!
//! 本模块不持有 EventStore、ProjectionStore 或授权能力；出错时只返回错误，不修改源 journal。回调的正确性取决于调用者让 fold
//! 只根据 state 和 event 计算新 state；Rust 的 `FnMut` 类型本身不能阻止回调执行外部副作用。事件列表与 `source_cursor` 的对应关系也由调用者保证。
//! `notification_projector.rs` 中的 `NotificationProjection::apply_committed()` 直接使用 `ReplayProjection`，但当前仓库未发现该类型的生产实例化调用者；
//! `ProjectionDriver` 对外导出并由测试构造，未发现生产构造调用点。源码中有 helper/调用表达式，不等于它们已经处在实际运行请求路径。

use kiana_domain::{
    EventCursor, EventId, ProjectionCheckpoint, RuntimeEvent, MAX_SOURCE_EVENT_IDS,
};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionDriverStatus {
    /// 可以接受新的 committed event slice。
    Ready,
    /// 上一次 replay/fold 失败；调用 `retry()` 明确恢复前拒绝继续 apply。
    Paused,
    /// 旧 checkpoint 已清除，下一次 apply 应从调用者提供的初始状态重建。
    RebuildRequired,
}

/// 管理单个 projector 的内存 checkpoint、运行状态与显式重试计数。
///
/// 该 driver 只消费调用方提供的已提交事件，不读 EventStore；它生成的 checkpoint 尚未持久化，持久化需要另一个 adapter/port。
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionDriver {
    /// 当前 projector 的稳定名称，用于拒绝把其他 projector 的 checkpoint 混入。
    projector: String,
    /// 最近一次成功 replay 的内存 checkpoint；失败时旧值保留。
    checkpoint: Option<ProjectionCheckpoint>,
    /// 阻止 paused 状态下无明确恢复动作地再次折叠事件。
    status: ProjectionDriverStatus,
    /// 调用者显式执行 retry 的次数；不代表自动网络/事件重试次数。
    retry_count: u32,
    /// 最近一次 replay/fold 错误，retry 成功后仍保留，直到后续 apply 成功才清除。
    last_error: Option<String>,
}

impl ProjectionDriver {
    /// 创建一个尚无 checkpoint、可从零开始折叠事件的 driver。
    ///
    /// 【调用者】投影运行方；当前对外导出供 projector 复用，具体事件读取仍由调用者完成。
    /// 【输入/输出】接收稳定 projector 名称，成功返回 `Ready` 且 checkpoint/错误为空的内存状态。
    /// 【失败情况】名称为空或超过 256 UTF-8 字节时返回 `projection_driver_projector_invalid`。
    /// 【副作用】只初始化 Rust 对象，不打开 store 或启动后台任务。
    pub fn new(projector: impl Into<String>) -> Result<Self, String> {
        let projector = projector.into();
        if projector.trim().is_empty() || projector.len() > 256 {
            return Err("projection_driver_projector_invalid".to_owned());
        }
        Ok(Self {
            projector,
            checkpoint: None,
            status: ProjectionDriverStatus::Ready,
            retry_count: 0,
            last_error: None,
        })
    }

    /// 用新事件更新 driver 的内存投影状态和 checkpoint。
    ///
    /// 【调用者】已从 EventLog 读取一段 committed events 的 projector。
    /// 【输入】`initial_state` 只在没有 checkpoint 时使用；`source_cursor` 是调用方声称的事件快照末端；`fold` 把每个未见事件映射到新 JSON state。
    /// 【核心流程】Paused 时拒绝；有 checkpoint 则从其 state 重放 tail，没有 checkpoint 则从零状态重放；成功才替换 checkpoint，失败则暂停并记录错误。
    /// 【失败情况】参数/检查点边界或 fold 错误以 `String` 返回；失败时既不推进旧 checkpoint，也不自动重试。
    /// 【为什么显式暂停】重复尝试不确定的 fold 可能重复调用方副作用；因此需要调用者先确认原因，再调用 `retry()` 或 `rebuild()`。
    /// ⚠ `fold` 应是确定、无副作用的纯状态转换；本类型无法从 trait 类型上强制这个约束。
    pub fn apply<F>(
        &mut self,
        initial_state: Value,
        events: &[RuntimeEvent],
        source_cursor: EventCursor,
        fold: F,
    ) -> Result<(), String>
    where
        F: FnMut(Value, &RuntimeEvent) -> Result<Value, String>,
    {
        // 暂停保护必须早于 replay：否则调用者连续调用 apply 就会隐式变成自动重试。
        if self.status == ProjectionDriverStatus::Paused {
            return Err("projection_driver_paused".to_owned());
        }
        // checkpoint 是否存在决定数据从哪里起步：有则从 checkpoint 继续，None 则从完整输入片段的初始 state 重新构建。
        let result = match self.checkpoint.as_ref() {
            Some(checkpoint) => ReplayProjection::from_checkpoint(
                &self.projector,
                checkpoint,
                events,
                source_cursor,
                fold,
            ),
            None => ReplayProjection::from_zero(
                &self.projector,
                initial_state,
                events,
                source_cursor,
                fold,
            ),
        };
        match result {
            Ok(projection) => {
                // 只有完成全部 fold 后才替换旧 checkpoint，避免半成品 state 覆盖最近一次成功状态。
                self.checkpoint = Some(projection.checkpoint()?);
                self.status = ProjectionDriverStatus::Ready;
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                // 保留旧 checkpoint 作为最后一次成功锚点；错误本身由调用者处理，不在这里自动循环。
                self.status = ProjectionDriverStatus::Paused;
                self.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    /// 由调用者明确解除 Paused 状态；本方法只更新 driver 状态，不会重放任何事件。
    ///
    /// 下一次仍需调用 `apply()` 并再次提供事件/fold。若状态不是 Paused 返回错误；计数溢出也会拒绝转换。
    pub fn retry(&mut self) -> Result<(), String> {
        if self.status != ProjectionDriverStatus::Paused {
            return Err("projection_driver_not_paused".to_owned());
        }
        self.retry_count = self
            .retry_count
            .checked_add(1)
            .ok_or_else(|| "projection_driver_retry_overflow".to_owned())?;
        self.status = ProjectionDriverStatus::Ready;
        Ok(())
    }

    /// 丢弃当前 checkpoint，并标记下次 `apply()` 从初始 state 重建。
    ///
    /// 该动作会失去内存中的加速锚点，但不删除 EventLog；调用方须提供足以重建目标视图的事件。
    pub fn rebuild(&mut self) {
        self.checkpoint = None;
        self.status = ProjectionDriverStatus::RebuildRequired;
        self.last_error = None;
    }

    /// 查看最近一次成功生成的 checkpoint；从未成功 replay 时为 None。
    pub fn checkpoint(&self) -> Option<&ProjectionCheckpoint> {
        self.checkpoint.as_ref()
    }

    /// 读取 Ready/Paused/RebuildRequired 状态，供调用者决定是否需要人工恢复。
    pub fn status(&self) -> ProjectionDriverStatus {
        self.status
    }

    /// 读取显式 `retry()` 的累计次数，不包含失败后被拒绝的 `apply()` 调用。
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// 查看最近记录的 fold/replay 错误；只有后续 apply 成功才清除。
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

/// 一次重放完成后的纯内存 projector 快照。
///
/// 它本身不持有存储或授权能力；`state` 的含义完全由调用者传入的 fold 函数定义。
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayProjection {
    /// 当前投影器名称，用于生成/恢复 checkpoint 时保持身份一致。
    projector: String,
    /// 调用者提供的已处理事件快照末端游标。
    source_cursor: EventCursor,
    /// 被 fold 过的事件身份集合；生成 checkpoint 前会排序并去重。
    source_event_ids: Vec<EventId>,
    /// fold 回调归并得到的读模型状态。
    state: Value,
}

impl ReplayProjection {
    /// 从初始 JSON state 折叠一段事件，建立新 projection。
    ///
    /// 【调用者】没有可复用 checkpoint，或明确要求全量 rebuild 的 projector。
    /// 【输入/输出】接收 projector 名称、初始 state、事件 slice、代表 slice 所属快照的最终 cursor 和 fold 回调；成功返回内存结果。
    /// 【失败情况】事件 ID 无效/超过合同数量、fold 返回错误或生成 checkpoint 无效时返回 `String`。
    /// 【限制】不检查每个 RuntimeEvent 与全局 EventLog cursor 的逐条映射，事件 slice 必须由调用者从一致快照中提供。
    pub fn from_zero<F>(
        projector: &str,
        initial_state: Value,
        events: &[RuntimeEvent],
        source_cursor: EventCursor,
        mut fold: F,
    ) -> Result<Self, String>
    where
        F: FnMut(Value, &RuntimeEvent) -> Result<Value, String>,
    {
        let (state, source_event_ids) = apply_events(initial_state, Vec::new(), events, &mut fold)?;
        Self::build(projector, source_cursor, source_event_ids, state)
    }

    /// 从已验证 checkpoint 继续折叠 tail，或检测 cursor/身份不一致后拒绝恢复。
    ///
    /// 【输入/输出】使用 checkpoint 中的 state 和已见 event IDs，接收新增事件 slice 及其最终 source cursor，返回更新后的内存快照。
    /// 【边界规则】projector 名必须相同；tail 为空时 source cursor 必须不变；tail 非空时最终 cursor 必须大于 checkpoint cursor。
    /// 【失败情况】checkpoint 无效、projector 不同、cursor 未推进或 fold 失败时返回错误；这里不回退到从零，也不改原 checkpoint。
    /// 【重要限制】本层不证明 tail 连续覆盖两个 cursor 之间的全部 EventLog 记录；调用者须从权威 store 获取一致 tail。
    pub fn from_checkpoint<F>(
        projector: &str,
        checkpoint: &ProjectionCheckpoint,
        tail: &[RuntimeEvent],
        source_cursor: EventCursor,
        mut fold: F,
    ) -> Result<Self, String>
    where
        F: FnMut(Value, &RuntimeEvent) -> Result<Value, String>,
    {
        checkpoint.validate()?;
        if checkpoint.projector != projector {
            return Err("projection_checkpoint_projector_mismatch".to_owned());
        }
        if tail.is_empty() {
            // 空 tail 不应该让 cursor 静默前进，否则报告会声称处理了实际未交给 fold 的事件。
            if source_cursor != checkpoint.source_cursor {
                return Err("projection_checkpoint_cursor_gap".to_owned());
            }
        } else if source_cursor <= checkpoint.source_cursor {
            // 有新事件时最终 cursor 必须前进；更细的连续性由事件读取端保证。
            return Err("projection_checkpoint_cursor_not_advanced".to_owned());
        }
        let (state, source_event_ids) = apply_events(
            checkpoint.state.clone(),
            checkpoint.source_event_ids.clone(),
            tail,
            &mut fold,
        )?;
        Self::build(projector, source_cursor, source_event_ids, state)
    }

    /// 将当前内存结果重新包装为经过校验的可序列化 Domain checkpoint。
    ///
    /// 不做持久化；返回值仍由调用者决定写到何处。
    pub fn checkpoint(&self) -> Result<ProjectionCheckpoint, String> {
        ProjectionCheckpoint::new(
            &self.projector,
            self.source_cursor,
            self.source_event_ids.clone(),
            self.state.clone(),
        )
    }

    /// 读取 projector 名称。
    pub fn projector(&self) -> &str {
        &self.projector
    }

    /// 读取调用者提供的 source cursor。
    pub fn source_cursor(&self) -> EventCursor {
        self.source_cursor
    }

    /// 读取已纳入状态的 EventId 列表。
    pub fn source_event_ids(&self) -> &[EventId] {
        &self.source_event_ids
    }

    /// 读取 projector 的 JSON state。
    pub fn state(&self) -> &Value {
        &self.state
    }

    /// 计算当前 JSON state 的内容摘要，便于比较/序列化；不会因此授予执行能力。
    pub fn state_digest(&self) -> String {
        kiana_domain::json_digest(&self.state)
    }

    /// 先通过 Domain checkpoint 构造器规范化和校验结果，再复制其规范化字段。
    ///
    /// 复用同一个构造器可以确保 ReplayProjection 与持久化 checkpoint 使用一致的 ID 排序、限制和摘要规则。
    fn build(
        projector: &str,
        source_cursor: EventCursor,
        source_event_ids: Vec<EventId>,
        state: Value,
    ) -> Result<Self, String> {
        let checkpoint = ProjectionCheckpoint::new(
            projector,
            source_cursor,
            source_event_ids.clone(),
            state.clone(),
        )?;
        Ok(Self {
            projector: checkpoint.projector,
            source_cursor: checkpoint.source_cursor,
            source_event_ids: checkpoint.source_event_ids,
            state: checkpoint.state,
        })
    }
}

/// 将事件片段折叠进 JSON state，同时合并已处理 EventId 集合。
///
/// 【调用者】`ReplayProjection::from_zero()` 和 `from_checkpoint()`。
/// 【去重】先把 checkpoint 中已有 ID 放入 `BTreeSet`；重复 ID 不再调用 fold，避免重放同一事件两次，并使输出 ID 顺序稳定。
/// 【失败情况】空 UUID 或超过最大 ID 数量时拒绝；fold 错误附加 `projection_fold_failed:` 前缀后传回 driver/投影调用者。
/// 【边界】重复 ID 的事件内容不在此处比较；EventLog 权威边界负责保证 ID 唯一且事件 slice/顺序正确。
fn apply_events<F>(
    mut state: Value,
    source_event_ids: Vec<EventId>,
    events: &[RuntimeEvent],
    fold: &mut F,
) -> Result<(Value, Vec<EventId>), String>
where
    F: FnMut(Value, &RuntimeEvent) -> Result<Value, String>,
{
    let mut seen = source_event_ids.into_iter().collect::<BTreeSet<_>>();
    if seen.len() > MAX_SOURCE_EVENT_IDS {
        return Err("projection_checkpoint_source_event_limit".to_owned());
    }
    for event in events {
        if event.event_id.as_uuid().is_nil() {
            return Err("projection_event_id_invalid".to_owned());
        }
        if !seen.insert(event.event_id) {
            // checkpoint 已包含该 ID 时跳过，不重复应用其 payload。
            continue;
        }
        if seen.len() > MAX_SOURCE_EVENT_IDS {
            return Err("projection_checkpoint_source_event_limit".to_owned());
        }
        state = fold(state, event).map_err(|reason| format!("projection_fold_failed:{reason}"))?;
    }
    Ok((state, seen.into_iter().collect()))
}
