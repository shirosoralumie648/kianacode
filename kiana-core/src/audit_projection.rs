//! 审计记录投影：从事件流到查询表的转换
//!
//! # 核心设计
//!
//! 审计事实（AuditRecord）永远保存在 EventLog 中。本模块负责：
//! 1. **投影计算**：将一段有序的、已提交的事件折叠为审计记录快照
//! 2. **游标验证**：确保投影的连贯性和幂等性，防止重放攻击和数据跳跃
//! 3. **查询过滤**：基于角色、会话、项目，从投影中提取可见的审计行
//!
//! # 为什么需要投影而不是直接查询事件
//!
//! EventLog 里的事件是**生的、无结构的**，需要特定的规则才能提取出「审计记录」。
//! 投影的存在让我们能够：
//! - 一次性计算，多次查询（避免每次都重新 fold 事件）
//! - 检测重复、验证连续性（防止数据损坏）
//! - 划分权限边界（同一个事件，不同角色看到的审计行不同）
//!
//! # 游标的作用
//!
//! - `EventCursor`：事件在 EventLog 中的位置（Page、Index）
//! - `AuditQueryCursor`：查询状态快照，用于分页查询
//! - 两者都是严格递增的，确保「新查询」一定比「旧查询」看到更多或相同的数据

use kiana_domain::{
    AuditActionKind, AuditDecision, AuditProjectionCheckpoint, AuditProjectionSnapshot,
    AuditQueryCursor, AuditQueryScan, AuditRecord, CoreResponse, EventCursor, EventId,
    RequestContext, RuntimeEvent, MAX_AUDIT_PROJECTION_RECORDS, MAX_AUDIT_QUERY_SCAN_EVENTS,
    MAX_SOURCE_EVENT_IDS,
};
use serde_json::Value;
use std::collections::HashSet;

use super::{ControlPlane, CoreError};

pub const AUDIT_PROJECTION_VERSION: u64 = 1;
const MAX_AUDIT_QUERY_LIMIT: usize = 1_000;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuditProjectionError {
    /// 事件集合为空，无法从空集推导审计记录
    #[error("audit_projection_source_empty")]
    SourceEmpty,
    /// 第一个事件的游标为 0 或不合法（游标应从 1 开始）
    #[error("audit_projection_source_cursor_invalid")]
    SourceCursorInvalid,
    /// 事件游标序列有间断。如预期是 1,2,3 但得到了 1,3（跳过 2）
    #[error("audit_projection_source_cursor_gap")]
    SourceCursorGap,
    /// 事件游标溢出（超过 u64::MAX）
    #[error("audit_projection_source_cursor_overflow")]
    SourceCursorOverflow,
    /// 同一个事件被投影了两次（duplicate event_id）
    #[error("audit_projection_source_event_duplicate")]
    SourceEventDuplicate,
    /// 审计记录数超过允许的最大值 MAX_AUDIT_PROJECTION_RECORDS
    #[error("audit_projection_record_limit")]
    RecordLimit,
    /// 从事件流计算审计记录时失败（domain 层算法错误）
    #[error("audit_projection_reduce_failed:{0}")]
    ReduceFailed(String),
    /// 投影快照的结构或约束不合法
    #[error("audit_projection_checkpoint_invalid:{0}")]
    CheckpointInvalid(String),
    /// 两个独立计算的快照内容不相同，表示算法非确定性或数据损坏
    #[error("audit_projection_checkpoint_mismatch")]
    CheckpointMismatch,
    /// 查询限制不在 1..MAX_AUDIT_QUERY_LIMIT 范围内
    #[error("audit_query_limit_invalid")]
    QueryLimitInvalid,
    /// 查询游标不合法（source_cursor 或 after_cursor 错误）
    #[error("audit_query_cursor_invalid")]
    QueryCursorInvalid,
    /// 查询游标已过时（投影版本变化或 epoch 不匹配）
    #[error("audit_query_cursor_stale")]
    QueryCursorStale,
    /// 查询过滤条件（如 target_kind）不合法
    #[error("audit_query_filter_invalid")]
    QueryFilterInvalid,
}

/// 审计查询的输入参数和过滤条件
///
/// 这个结构体由客户端提供，所有字段都被当作不可信输入进行验证。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditQueryInput {
    /// 投影版本检查点：调用方声称投影的最后事件游标是这个值。
    /// 用于检测查询过程中投影版本发生了变化（有新事件提交）
    pub source_cursor: Option<EventCursor>,

    /// 查询起点：只返回 source_cursor > after_cursor 的审计记录。
    /// 用于分页查询时跳过已读的部分
    pub after_cursor: EventCursor,

    /// 每次查询最多返回多少条记录。必须在 1..MAX_AUDIT_QUERY_LIMIT 之间
    pub limit: usize,

    /// 可选的过滤条件：只关心这种类型的审计操作（如 "grant_created"）
    pub action_kind: Option<AuditActionKind>,

    /// 可选的过滤条件：只关心这种决策的审计行（如 "approved"）
    pub decision: Option<AuditDecision>,

    /// 可选的过滤条件：只关心针对这种资源的操作（长度限制在 1-128 字符）
    pub target_kind: Option<String>,

    /// 分页游标：如果之前有查询并得到了 next_cursor，重新查询时传入这个值
    /// 以获取下一页的记录。Core 验证这个游标的完整性后才使用
    pub cursor: Option<AuditQueryCursor>,
}

impl AuditQueryInput {
    /// 验证客户端提供的查询参数是否合法
    ///
    /// 这是一个防御性的验证函数，检查以下约束：
    /// 1. **limit 范围检查**：必须在 [1, MAX_AUDIT_QUERY_LIMIT] 之间。
    ///    - limit = 0 意味着客户端想查 0 条记录，这是无意义的
    ///    - limit > MAX_AUDIT_QUERY_LIMIT 意味着客户端试图一次性抓取过多数据，
    ///      可能导致内存爆炸或服务过载
    ///
    /// 2. **source_cursor 合法性**：
    ///    - source_cursor 不能是 0（游标从 1 开始，0 是非法的哨兵值）
    ///    - after_cursor 必须 <= source_cursor，否则查询是不可能满足的
    ///      （无法返回比最后事件还要新的记录）
    ///
    /// 3. **target_kind 内容检查**：
    ///    - 不能是纯空白字符串（例如 "   "）
    ///    - 长度必须在合理范围内（1-128 字符），防止巨大字符串
    ///
    /// 4. **分页游标一致性**：
    ///    - 如果客户端提供了 cursor（分页状态），则必须验证：
    ///      * cursor 自身的结构完整性
    ///      * cursor 的 source_cursor 与查询的 source_cursor 一致
    ///        （防止跨版本混用游标，即投影更新后还在用旧游标）
    ///      * cursor 的 after_cursor 与查询的 after_cursor 一致
    ///        （如果明确指定了新的 after_cursor，则不应该混用旧的游标）
    pub fn validate(&self) -> Result<(), AuditProjectionError> {
        // 检查 limit 是否在有效范围内
        if self.limit == 0 || self.limit > MAX_AUDIT_QUERY_LIMIT {
            return Err(AuditProjectionError::QueryLimitInvalid);
        }

        // 检查游标的单调性：source_cursor 不能是 0，after_cursor 必须小于等于 source_cursor
        // source_cursor 为 None 时，默认认为是 u64::MAX（视为投影尚无上界约束）
        if self.source_cursor == Some(0)
            || self.after_cursor > self.source_cursor.unwrap_or(u64::MAX)
        {
            return Err(AuditProjectionError::QueryCursorInvalid);
        }

        // 检查过滤条件 target_kind 的合法性
        if let Some(target_kind) = &self.target_kind {
            if target_kind.trim().is_empty() || target_kind.len() > 128 {
                return Err(AuditProjectionError::QueryFilterInvalid);
            }
        }

        // 如果客户端提供了分页游标，则验证其一致性
        if let Some(cursor) = &self.cursor {
            // 首先验证游标自身的结构完整性
            cursor
                .validate()
                .map_err(|_| AuditProjectionError::QueryCursorInvalid)?;

            // 检查游标与查询参数的一致性
            // source_cursor 不匹配意味着投影已更新，旧游标不可用
            // after_cursor 不匹配意味着查询的起点变了，旧游标不可用
            if self
                .source_cursor
                .is_some_and(|value| value != cursor.source_cursor)
                || (self.after_cursor != 0 && self.after_cursor != cursor.after_cursor)
            {
                return Err(AuditProjectionError::QueryCursorInvalid);
            }
        }
        Ok(())
    }
}

/// 计算查询过滤条件的确定性哈希摘要
///
/// # 为什么需要这个摘要
///
/// 审计查询支持分页，分页需要游标来记录「我已经读到第几条记录」。
/// 但是游标还必须编码「我在使用哪个过滤条件集合」。
///
/// 示例：
/// - 第一次查询：filter = {action_kind: "grant_created", limit: 10}
///   返回记录 1-10，next_cursor = Cursor(digest_A, after_cursor=10)
/// - 如果用户改变了 filter，比如改成 {action_kind: "grant_revoked", limit: 10}
///   此时不能复用旧的 next_cursor，因为 digest_B != digest_A
///   复用会导致「跳到记录 11 但用的是错误的过滤条件」
///
/// 注意这里用的是三个可选的过滤字段：
/// - action_kind：审计操作类型（如 "grant_created"、"permission_updated"）
/// - decision：审计决策结果（如 "approved"、"denied"）
/// - target_kind：目标资源类型（如 "run"、"request"）
///
/// 其他字段（limit、after_cursor）不影响记录的"身份"，只影响分页位置，
/// 所以不计入摘要。
fn query_filter_digest(query: &AuditQueryInput) -> String {
    kiana_domain::json_digest(&serde_json::json!({
        "action_kind": query.action_kind,
        "decision": query.decision,
        "target_kind": query.target_kind,
    }))
}

/// 验证并计算单个事件的游标位置
///
/// # 游标约束
///
/// EventLog 中的事件必须形成一个严格递增、没有间隙的序列。
/// 这确保了投影的连贯性和可重放性。
///
/// 游标的含义：
/// - first_cursor = 1：第一个事件的游标是 1
/// - 如果有 100 个事件，从游标 1 到 100，那么：
///   * 第 0 个事件应该在游标 1
///   * 第 1 个事件应该在游标 2
///   * 第 99 个事件应该在游标 100
///
/// # 检查什么
///
/// 1. **游标溢出检查**：expected = first_cursor + index
///    - 如果 first_cursor 已经很大，加上 index 会超过 u64::MAX，则返回溢出错误
///    - 这是防御性的，实际上很难触发（需要从 u64::MAX 附近的游标开始）
///
/// 2. **事件内的游标字段**：事件的 data 中可能存储了 "source_cursor" 字段
///    - 这个字段是事件发生时的"当时认为的游标"
///    - 必须验证这个字段与我们计算出的 expected 游标一致
///    - 如果不一致，说明事件序列有间隙或损坏
///
/// 3. **游标有效性**：source_cursor 必须 > 0
///    - 0 是非法的哨兵值，游标应该从 1 开始
fn event_cursor(
    event: &RuntimeEvent,
    first_cursor: EventCursor,
    index: usize,
) -> Result<EventCursor, AuditProjectionError> {
    // 计算这个事件"应该"所在的游标
    let expected = first_cursor
        .checked_add(index as u64)
        .ok_or(AuditProjectionError::SourceCursorOverflow)?;

    // 检查事件内的游标字段是否与期望值一致
    if let Some(cursor) = event.data.get("source_cursor") {
        let observed = cursor
            .as_u64()
            .filter(|cursor| *cursor > 0)
            .ok_or(AuditProjectionError::SourceCursorInvalid)?;
        if observed != expected {
            // 如果实际游标 != 期望游标，说明事件序列有间隙
            return Err(AuditProjectionError::SourceCursorGap);
        }
    }
    Ok(expected)
}

/// 提取并验证事件的 ID 列表，确保无重复和无超限
///
/// # 为什么需要检测重复
///
/// 如果同一个事件被投影了两次（例如，因为重放或复制错误），
/// 那么投影的结果就会错误地重复计算，导致审计记录失效。
///
/// 示例：
/// - 事件 E1, E2, E3
/// - 如果因为某个错误，我们收到了 E1, E2, E2, E3（E2 重复）
/// - 那么某些审计记录会被计算两次，数据就被污染了
///
/// 因此这个函数使用 HashSet 来检测重复。
///
/// # 为什么需要检测超限
///
/// 审计系统有资源限制，不能无限接纳事件。
/// MAX_SOURCE_EVENT_IDS 是一个架构级别的常量，定义了单次投影能处理的最多事件数。
/// 这防止：
/// - 某个恶意客户端一次性提交 1 百万条事件导致内存爆炸
/// - 某个 bug 导致事件列表意外膨胀
fn source_event_ids(events: &[RuntimeEvent]) -> Result<Vec<EventId>, AuditProjectionError> {
    // 超限检查：事件列表太长了
    if events.len() > MAX_SOURCE_EVENT_IDS {
        return Err(AuditProjectionError::RecordLimit);
    }

    // 预分配容量，避免分配时的重新分配
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(events.len());

    // 逐个事件提取 ID，并检测重复
    for event in events {
        if !seen.insert(event.event_id.to_string()) {
            // 如果 insert 返回 false，说明这个 event_id 已经存在过
            return Err(AuditProjectionError::SourceEventDuplicate);
        }
        ids.push(event.event_id);
    }

    Ok(ids)
}

/// 验证事件集合的整体完整性
///
/// 这是一个*组合验证*函数，汇总了对事件来源的所有约束检查：
///
/// # 验证流程
///
/// 1. **空集检查**：events 不能为空
///    - 空的事件列表无法产生任何投影
///
/// 2. **起始游标合法性**：first_cursor 不能是 0
///    - 游标从 1 开始，0 是非法的
///
/// 3. **事件 ID 去重和超限**：调用 source_event_ids()
///    - 确保没有重复事件
///    - 确保事件数没有超过 MAX_SOURCE_EVENT_IDS
///
/// 4. **游标连贯性**：逐个检查每个事件的游标是否形成连贯序列
///    - 使用 event_cursor() 函数检查每个事件
///    - 最后返回最后一个事件的游标
///
/// # 返回值
///
/// 返回最后一个事件的游标位置。这个值很重要，因为投影的状态就记录在这个游标上：
/// "我已经处理到游标 X 的所有事件"。
fn validate_source(
    events: &[RuntimeEvent],
    first_cursor: EventCursor,
) -> Result<EventCursor, AuditProjectionError> {
    // 检查事件列表是否为空或起始游标是否为 0
    if events.is_empty() || first_cursor == 0 {
        return Err(if events.is_empty() {
            AuditProjectionError::SourceEmpty
        } else {
            AuditProjectionError::SourceCursorInvalid
        });
    }

    // 检查事件 ID 的去重和超限
    source_event_ids(events)?;

    // 逐个验证每个事件的游标连贯性，记下最后的游标
    let mut last = first_cursor;
    for (index, event) in events.iter().enumerate() {
        last = event_cursor(event, first_cursor, index)?;
    }

    Ok(last)
}

/// 从一段有序的、已提交的事件重建完整的审计投影快照
///
/// # 投影的含义
///
/// 本质上，这个函数就是：
/// ```text
/// input: [Event1, Event2, Event3, ...]
/// output: AuditProjectionSnapshot {
///     records: [AuditRecord1, AuditRecord2, ...],  // 推导出的审计记录
///     source_cursor: 最后一个事件的游标,
///     source_event_ids: [Event1.id, Event2.id, ...],  // 事件ID去重列表
///     checkpoint: 这个投影的完整性证明
/// }
/// ```
///
/// # 重建流程
///
/// 1. **验证事件源**：validate_source()
///    - 确保事件列表合法、连贯、无重复
///    - 获取最后一个事件的游标
///
/// 2. **提取事件 ID**：source_event_ids()
///    - 为投影存储事件的身份信息，便于检测后续的重复或遗漏
///
/// 3. **计算审计记录**：reduce_audit_records()
///    - 这是领域层的函数，负责从事件序列"折叠"出审计记录
///    - 例如，一个 "request.created" 和后续的 "request.approved" 事件
///      可能会被折叠成一条 AuditRecord
///    - 结果可能包含 M 条审计记录（M 通常 < N 个事件）
///
/// 4. **验证记录数**：确保结果的审计记录数没有超过 MAX_AUDIT_PROJECTION_RECORDS
///
/// 5. **构造快照**：AuditProjectionSnapshot::new()
///    - 封装上述结果并生成 checkpoint（一个确定性的哈希）
///    - checkpoint 用于检测投影的一致性：
///      同样的事件，再计算一遍应该得到相同的 checkpoint
pub fn rebuild_audit_projection(
    events: &[RuntimeEvent],
    first_cursor: EventCursor,
) -> Result<AuditProjectionSnapshot, AuditProjectionError> {
    // 第一步：验证事件源的完整性和连贯性
    let last_cursor = validate_source(events, first_cursor)?;

    // 第二步：提取事件 ID 列表（用于追踪和去重）
    let source_ids = source_event_ids(events)?;

    // 第三步：从事件流"折叠"出审计记录
    let records = kiana_domain::reduce_audit_records(events, first_cursor)
        .map_err(AuditProjectionError::ReduceFailed)?;

    // 第四步：验证审计记录数没有超过限制
    if records.len() > MAX_AUDIT_PROJECTION_RECORDS {
        return Err(AuditProjectionError::RecordLimit);
    }

    // 第五步：构造快照，生成 checkpoint
    AuditProjectionSnapshot::new(
        AUDIT_PROJECTION_VERSION,
        last_cursor,
        source_ids,
        records,
        Vec::new(),
    )
    .map_err(AuditProjectionError::CheckpointInvalid)
}

/// 有状态的投影门面，用于投影的生命周期管理
///
/// # 设计目的
///
/// `AuditProjectionSnapshot` 是不可变的快照，用于存储和查询。
/// `AuditProjection` 则是一个有状态的包装，支持投影的逐步更新和恢复。
///
/// 用途场景：
/// - **初始化**：从一段事件重建投影（rebuild）
/// - **恢复**：从持久化存储恢复一个已保存的投影（restore）
/// - **增量更新**：收到新事件页后，追加更新投影（apply_page）
/// - **查询**：从投影中获取记录列表（records）
#[derive(Clone, Debug)]
pub struct AuditProjection {
    /// 内部维护的快照，所有查询操作都基于这个快照
    snapshot: AuditProjectionSnapshot,
}

impl AuditProjection {
    /// 从一段事件重建投影
    ///
    /// 这是一个"初始化"方法，适用于：
    /// - 系统启动时，从 EventLog 加载所有历史事件，构建初始投影
    /// - 诊断和审计时，重新计算投影以检查一致性
    pub fn rebuild(
        events: &[RuntimeEvent],
        first_cursor: EventCursor,
    ) -> Result<Self, AuditProjectionError> {
        Ok(Self {
            snapshot: rebuild_audit_projection(events, first_cursor)?,
        })
    }

    /// 从一个已保存的快照恢复投影
    ///
    /// 这是一个"恢复"方法，适用于：
    /// - 从数据库或文件系统加载之前保存的投影快照
    /// - 需要验证快照的结构完整性（通过 validate()）
    pub fn from_snapshot(snapshot: AuditProjectionSnapshot) -> Result<Self, AuditProjectionError> {
        snapshot
            .validate()
            .map_err(AuditProjectionError::CheckpointInvalid)?;
        Ok(Self { snapshot })
    }

    /// 增量更新投影：追加新的一页事件
    ///
    /// # 增量更新的意义
    ///
    /// EventLog 可能很大，无法一次性全部加载。因此支持"分页更新"：
    /// - 第一页：事件 1-1000
    /// - 第二页：事件 1001-2000
    /// - 第三页：事件 2001-3000
    /// 每次调用 apply_page() 会追加新的事件和记录。
    ///
    /// # 约束检查
    ///
    /// 1. 新页的 first_cursor 必须恰好是 投影.source_cursor + 1
    ///    （即相邻、无间隙）
    /// 2. 新页内的事件必须合法（通过 validate_source）
    /// 3. 新页的事件 ID 不能与已有的事件 ID 重复
    /// 4. 合并后的总事件数和记录数都不能超过限制
    pub fn apply_page(
        &mut self,
        events: &[RuntimeEvent],
        first_cursor: EventCursor,
    ) -> Result<(), AuditProjectionError> {
        // 检查新页是否与当前投影相邻
        let expected = self
            .snapshot
            .source_cursor
            .checked_add(1)
            .ok_or(AuditProjectionError::SourceCursorOverflow)?;
        if first_cursor != expected {
            return Err(if first_cursor < expected {
                // first_cursor < expected: 新页开始位置太早，说明重放或回溯
                AuditProjectionError::SourceCursorInvalid
            } else {
                // first_cursor > expected: 新页开始位置太晚，说明有间隙
                AuditProjectionError::SourceCursorGap
            });
        }

        // 验证新页的事件源完整性
        let last_cursor = validate_source(events, first_cursor)?;
        let page_ids = source_event_ids(events)?;

        // 检查新页的事件 ID 与已有事件 ID 是否有重复
        let mut all_ids = self.snapshot.source_event_ids.clone();
        let mut existing_ids = all_ids
            .iter()
            .map(ToString::to_string)
            .collect::<HashSet<_>>();

        for event_id in page_ids {
            if !existing_ids.insert(event_id.to_string()) {
                return Err(AuditProjectionError::SourceEventDuplicate);
            }
            all_ids.push(event_id);
            // 同时检查总事件数是否超限
            if all_ids.len() > MAX_SOURCE_EVENT_IDS {
                return Err(AuditProjectionError::RecordLimit);
            }
        }

        // 从新页事件计算增量审计记录
        let appended = kiana_domain::reduce_audit_records(events, first_cursor)
            .map_err(AuditProjectionError::ReduceFailed)?;

        // 追加到现有记录中
        let mut records = self.snapshot.records.clone();
        records.extend(appended);
        if records.len() > MAX_AUDIT_PROJECTION_RECORDS {
            return Err(AuditProjectionError::RecordLimit);
        }

        // 构造新的快照（版本号保持不变，游标更新）
        self.snapshot = AuditProjectionSnapshot::new(
            self.snapshot.projection_version,
            last_cursor,
            all_ids,
            records,
            self.snapshot.limitations.clone(),
        )
        .map_err(AuditProjectionError::CheckpointInvalid)?;
        Ok(())
    }

    /// 返回当前投影的快照引用
    pub fn snapshot(&self) -> &AuditProjectionSnapshot {
        &self.snapshot
    }

    /// 返回投影快照的 checkpoint（用于一致性验证）
    pub fn checkpoint(&self) -> &AuditProjectionCheckpoint {
        &self.snapshot.checkpoint
    }

    /// 返回投影中的所有审计记录
    pub fn records(&self) -> &[AuditRecord] {
        &self.snapshot.records
    }

    /// 从持久化的 checkpoint 和记录恢复投影
    ///
    /// 这与 from_snapshot() 类似，但更细粒度：
    /// - from_snapshot()：直接恢复一个完整的快照对象
    /// - restore()：分别提供 checkpoint 和 records，然后重建快照
    ///
    /// restore() 的用途：
    /// - 当 checkpoint 和 records 被分别存储时（例如，checkpoint 存在元数据表，
    ///   records 存在分片表），需要分别加载后再组合
    /// - 需要验证 checkpoint 与 records 是否匹配（通过比对重新计算的 checkpoint）
    pub fn restore(
        checkpoint: AuditProjectionCheckpoint,
        records: Vec<AuditRecord>,
    ) -> Result<Self, AuditProjectionError> {
        // 首先验证 checkpoint 自身的结构
        checkpoint
            .validate()
            .map_err(AuditProjectionError::CheckpointInvalid)?;

        // 根据 checkpoint 和 records 重建快照
        let snapshot = AuditProjectionSnapshot::new(
            checkpoint.projection_version,
            checkpoint.source_cursor,
            checkpoint.source_event_ids.clone(),
            records,
            Vec::new(),
        )
        .map_err(AuditProjectionError::CheckpointInvalid)?;

        // 验证新计算的 checkpoint 是否与恢复的 checkpoint 一致
        // 如果不一致，说明数据损坏或版本不匹配
        if snapshot.checkpoint != checkpoint {
            return Err(AuditProjectionError::CheckpointMismatch);
        }

        Ok(Self { snapshot })
    }
}

impl ControlPlane {
    /// 重建完整的审计投影（从全部已提交事件）
    ///
    /// # 为什么这是一个 ControlPlane 方法
    ///
    /// ControlPlane 是组织的权限中心，掌握所有已提交事件的读取权限。
    /// 审计查询可能来自多个角色、多个会话，因此需要：
    /// - 读取**全局**事件日志（而不是单个会话的事件）
    /// - 根据查询者的权限，对投影进行过滤
    ///
    /// # 调用流程
    ///
    /// 1. 从适配器读取所有已提交事件（read_all_events）
    /// 2. 获取第一个事件的游标（默认为 1）
    /// 3. 调用 rebuild_audit_projection() 计算投影
    /// 4. 返回投影快照
    ///
    /// # 可能的错误
    ///
    /// - 如果适配器不支持"读全部事件"语义，会返回 Unavailable 错误
    /// - 如果事件重建过程中失败（例如，事件序列损坏），返回 Failed 错误
    pub async fn audit_projection(&self) -> Result<AuditProjectionSnapshot, CoreError> {
        // 从适配器读取全部已提交事件
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("audit_projection_read_all_unsupported".to_owned())
        })?;

        // 获取第一个事件的游标（通常是 1，除非有特殊配置）
        let first_cursor = events
            .first()
            .and_then(|event| event.data.get("source_cursor"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1);

        // 重建投影
        rebuild_audit_projection(&events, first_cursor)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }

    /// 在认证会话/项目范围内查询服务派生的审计记录
    ///
    /// # 查询流程总览
    ///
    /// 这个方法是**权限过滤的查询引擎**：
    /// 1. 验证查询参数的合法性
    /// 2. 读取 adapter 的已提交游标快照（不支持时显式降级为限制）
    /// 3. 在读取前拒绝越界游标和已越过保留前缀（撤销）的游标
    /// 4. 有界读取事件并重建完整的审计投影（全球范围）
    /// 5. 用已提交边界检测投影滞后/超前（查询期间的新提交、重放或不可见的已提交事件）
    /// 6. 检查投影的版本一致性（防止跨版本混用游标）
    /// 7. **四层权限过滤**：
    ///    - 第 1 层：找出查询者拥有的所有 run
    ///    - 第 2 层：找出这些 run 对应的所有 request
    ///    - 第 3 层：找出这些 run 对应的所有 approval
    ///    - 第 4 层：找出与上述任何实体关联的事件
    /// 8. 从投影记录中，只返回与这些事件关联的审计记录
    /// 9. 应用查询过滤（action_kind、decision、target_kind）
    /// 10. 应用有界分页（limit、after_cursor）并输出扫描统计
    ///
    /// # 设计原则
    ///
    /// - **不暴露原始事件**：返回的只是推导出的 AuditRecord，不包含事件的原始 data
    /// - **不信任调用者的 owner/scope 字符串**：通过事件关系推导，而不是相信字符串参数
    /// - **拒绝过时游标**：如果投影版本已变化，即使部分数据仍有效，也拒绝旧游标
    ///   （fail-closed 原则：宁可让分页重新开始，也不要返回可能错误的数据）
    /// - **拒绝滞后/撤销的边界**：投影跟不上已提交事实，或游标已越过被撤销的前缀时，
    ///   返回结构化拒绝而不是一页看起来完整的旧数据
    pub async fn query_audit(
        &self,
        context: &RequestContext,
        query: AuditQueryInput,
    ) -> Result<CoreResponse, CoreError> {
        // 第一步：验证查询参数的合法性
        query
            .validate()
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;

        // 第二步：读取 adapter 的已提交游标快照。
        // 支持快照的 adapter 让我们在读取前就能拒绝越界/被撤销的游标并检测投影滞后；
        // 不支持时必须显式降级为限制，而不是猜一个游标或把日志当成空的。
        let snapshot = match self.events.cursor_snapshot().await {
            Ok(snapshot) => {
                snapshot.validate().map_err(|error| {
                    kiana_ports::PortError::Failed(format!(
                        "eventlog_cursor_snapshot_invalid:{error}"
                    ))
                })?;
                Some(snapshot)
            }
            Err(kiana_ports::PortError::Unavailable(reason))
                if reason == "event_store_cursor_snapshot_unsupported" =>
            {
                None
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(snapshot) = &snapshot {
            let requested_source = query
                .cursor
                .as_ref()
                .map(|cursor| cursor.source_cursor)
                .or(query.source_cursor);
            let requested_after = query
                .cursor
                .as_ref()
                .map(|cursor| cursor.after_cursor)
                .unwrap_or(query.after_cursor);
            if requested_source.is_some_and(|value| value > snapshot.last_committed_cursor)
                || requested_after > snapshot.last_committed_cursor
            {
                return Err(kiana_ports::PortError::Conflict(
                    "audit_query_cursor_invalid".to_owned(),
                )
                .into());
            }
            // 游标已经越过保留前缀：客户端见过的记录可能已被撤销，继续翻页会跨过缺口。
            if requested_after.saturating_add(1) < snapshot.first_retained_cursor {
                return Err(kiana_ports::PortError::Conflict(
                    "audit_query_cursor_revoked".to_owned(),
                )
                .into());
            }
        }

        // 第三步：读取全部事件，重建投影。
        // 读取量必须落在有界扫描窗口内；超限时拒绝，而不是无界读取或静默截断。
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("audit_query_read_all_unsupported".to_owned())
        })?;
        if events.len() > MAX_AUDIT_QUERY_SCAN_EVENTS {
            return Err(
                kiana_ports::PortError::Unavailable("audit_query_scan_limit".to_owned()).into(),
            );
        }
        let first_cursor = events
            .first()
            .and_then(|event| event.data.get("source_cursor"))
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let projection = rebuild_audit_projection(&events, first_cursor)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;

        // 第四步：把本次读取与 adapter 的已提交边界对齐。
        // 投影读到的事件数多于边界说明查询期间有新提交（重放/并发查询）；少于边界说明投影
        // 滞后于已提交事实。两者都不能返回一页“看起来完整”的旧数据，必须 fail closed。
        if let Some(snapshot) = &snapshot {
            let observed = events.len() as u64;
            let expected = snapshot.retained_event_count();
            if observed > expected {
                return Err(kiana_ports::PortError::Conflict(
                    "audit_query_snapshot_stale".to_owned(),
                )
                .into());
            }
            if observed < expected {
                return Err(kiana_ports::PortError::Unavailable(
                    "audit_query_projection_lag".to_owned(),
                )
                .into());
            }
        }

        // 第五步：检查投影版本一致性
        // 如果客户端声称的 source_cursor 与实际投影不符，说明投影已更新
        // 此时应该拒绝旧的分页游标，要求客户端重新查询
        if query
            .source_cursor
            .is_some_and(|cursor| cursor != projection.source_cursor)
        {
            return Err(
                kiana_ports::PortError::Conflict("audit_query_cursor_stale".to_owned()).into(),
            );
        }

        // 检查 after_cursor 是否超过了投影范围（例如，客户端传入了无效的游标）
        if query.after_cursor > projection.source_cursor {
            return Err(
                kiana_ports::PortError::Conflict("audit_query_cursor_invalid".to_owned()).into(),
            );
        }

        // 计算过滤条件摘要（用于分页游标的一致性检查）
        let filter_digest = query_filter_digest(&query);
        let epoch = projection.checkpoint.checkpoint_digest.clone();

        // 验证分页游标的一致性
        // 分页游标编码了投影版本、过滤条件等信息，如果这些信息变化了，游标就失效了
        if let Some(cursor) = &query.cursor {
            if cursor.source_cursor != projection.source_cursor
                || cursor.projection_version != projection.projection_version
                || cursor.epoch != epoch
                || cursor.filter_digest != filter_digest
            {
                return Err(kiana_ports::PortError::Conflict(
                    "audit_query_cursor_stale".to_owned(),
                )
                .into());
            }
        }

        // 确定真实的 after_cursor：优先使用分页游标中的值，否则使用查询参数
        let after_cursor = query
            .cursor
            .as_ref()
            .map(|cursor| cursor.after_cursor)
            .unwrap_or(query.after_cursor);

        // 第六步：权限过滤（四层构建）
        // ========================================
        // 首先验证查询者的身份
        let actor = context
            .actor_id
            .as_deref()
            .filter(|actor| !actor.trim().is_empty())
            .ok_or_else(|| {
                kiana_ports::PortError::Failed("audit_query_unauthenticated".to_owned())
            })?;

        // 第 1 层过滤：找出查询者在当前会话/项目中拥有的所有 run
        // 条件：
        // - 事件类型是 "run.authorized"（run 授权事件）
        // - actor_id 匹配当前查询者
        // - session_id 匹配当前会话
        // - project_root 匹配当前项目（规范化后对比）
        let owned_runs = events
            .iter()
            .filter(|event| event.kind == "run.authorized")
            .filter(|event| {
                event.data.get("actor_id").and_then(Value::as_str) == Some(actor)
                    && event.data.get("session_id").and_then(Value::as_str)
                        == Some(context.session_id.as_str())
                    && event
                        .data
                        .get("project_root")
                        .and_then(Value::as_str)
                        .is_some_and(|project| {
                            Self::canonical_project_root(project)
                                == Self::canonical_project_root(&context.project_root)
                        })
            })
            .filter_map(|event| event.data.get("run_id").and_then(Value::as_str))
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>();

        // 第 2 层过滤：找出 owned_runs 中的 run 所发起的所有 request
        // 条件：
        // - 事件的 run_id 在 owned_runs 中
        let owned_requests = events
            .iter()
            .filter(|event| {
                event
                    .data
                    .get("run_id")
                    .and_then(Value::as_str)
                    .is_some_and(|run| owned_runs.contains(run))
            })
            .map(|event| event.request_id.to_string())
            .collect::<std::collections::HashSet<_>>();

        // 第 3 层过滤：找出 owned_runs 中的 run 所关联的所有 approval
        // 条件：
        // - 事件的 run_id 在 owned_runs 中
        // - 事件数据中包含 approval_id 字段
        let owned_approvals = events
            .iter()
            .filter(|event| {
                event
                    .data
                    .get("run_id")
                    .and_then(Value::as_str)
                    .is_some_and(|run| owned_runs.contains(run))
            })
            .filter_map(|event| event.data.get("approval_id").and_then(Value::as_str))
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>();

        // 第 4 层过滤：找出与上述任何实体关联的事件
        // 条件（任意一项满足即可）：
        // 1. 事件的 run_id 在 owned_runs 中
        // 2. 事件的 request_id 在 owned_requests 中
        // 3. 事件的 approval_id 在 owned_approvals 中
        // 4. 事件的 aggregate_type 是 "run" 且 aggregate_id 在 owned_runs 中
        let owned_event_ids = events
            .iter()
            .filter(|event| {
                event
                    .data
                    .get("run_id")
                    .and_then(Value::as_str)
                    .is_some_and(|run| owned_runs.contains(run))
                    || owned_requests.contains(&event.request_id.to_string())
                    || event
                        .data
                        .get("approval_id")
                        .and_then(Value::as_str)
                        .is_some_and(|approval| owned_approvals.contains(approval))
                    || (event.aggregate_type.as_deref() == Some("run")
                        && event
                            .aggregate_id
                            .as_deref()
                            .is_some_and(|run| owned_runs.contains(run)))
            })
            .map(|event| event.event_id.to_string())
            .collect::<std::collections::HashSet<_>>();

        // 第七步：从投影记录中过滤出与上述事件关联的审计记录
        // 然后再应用额外的过滤条件和分页
        let scoped_records = projection
            .records
            .iter()
            // 只返回与 owned_event_ids 关联的记录
            .filter(|record| {
                record
                    .source_event_ids
                    .iter()
                    .any(|event_id| owned_event_ids.contains(&event_id.to_string()))
            })
            .cloned()
            .collect::<Vec<_>>();
        let matched_records = scoped_records
            .iter()
            // 应用分页过滤：只返回游标之后的记录
            .filter(|record| record.source_cursor > after_cursor)
            // 应用 action_kind 过滤
            .filter(|record| {
                query
                    .action_kind
                    .is_none_or(|kind| record.action_kind == kind)
            })
            // 应用 decision 过滤
            .filter(|record| {
                query
                    .decision
                    .is_none_or(|decision| record.decision == decision)
            })
            // 应用 target_kind 过滤
            .filter(|record| {
                query
                    .target_kind
                    .as_deref()
                    .is_none_or(|target| record.target_kind == target)
            })
            .cloned()
            .collect::<Vec<_>>();
        let projected_records = projection.records.len();
        let mut records = matched_records.clone();
        let matched_count = matched_records.len();

        // 第八步：分页处理
        // 如果结果多于请求的 limit，则：
        // 1. 取前 limit 条记录
        // 2. 生成 next_cursor，告诉客户端还有更多数据
        // 否则，next_cursor = None 表示已到末尾
        let next_cursor = if records.len() > query.limit {
            let cursor = records[query.limit - 1].source_cursor;
            records.truncate(query.limit);
            Some(
                AuditQueryCursor::new(
                    epoch.clone(),
                    projection.projection_version,
                    projection.source_cursor,
                    cursor,
                    filter_digest.clone(),
                )
                .map_err(|error| kiana_ports::PortError::Failed(error.to_owned()))?,
            )
        } else {
            None
        };
        let page_records = records.len();

        // 有界扫描统计：计数值来自本次实际扫描，分类不改变页内容。
        let scan = AuditQueryScan::new(
            u32::try_from(events.len()).unwrap_or(u32::MAX),
            u32::try_from(projected_records).unwrap_or(u32::MAX),
            u32::try_from(scoped_records.len()).unwrap_or(u32::MAX),
            u32::try_from(matched_count).unwrap_or(u32::MAX),
            u32::try_from(page_records).unwrap_or(u32::MAX),
        )
        .map_err(|error| kiana_ports::PortError::Failed(error.to_owned()))?;

        // 不支持游标快照的 adapter 显式降级为限制；裁剪过的前缀也如实记录。
        let mut limitations = projection.limitations;
        match &snapshot {
            Some(snapshot) if snapshot.first_retained_cursor > 1 => {
                limitations.push("eventlog_retention_prefix_trimmed".to_owned());
            }
            Some(_) => {}
            None => limitations.push("eventlog_cursor_snapshot_unsupported".to_owned()),
        }

        // 构造响应
        Ok(CoreResponse::completed(
            context.request_id,
            serde_json::json!({
                "schema": "kiana.audit-query.v1",
                "records": records,
                "next_cursor": next_cursor,
                "source_cursor": projection.source_cursor,
                "projection_version": projection.projection_version,
                "epoch": epoch,
                "filter_digest": filter_digest,
                "snapshot": snapshot,
                "scan": scan,
                "limitations": limitations,
            }),
        ))
    }
}
