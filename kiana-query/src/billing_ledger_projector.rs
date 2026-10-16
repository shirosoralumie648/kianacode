//! BQ-20 只读账单投影器，属于 `kiana-query` 查询读模型层。
//!
//! 【文件职责与系统位置】
//! 本文件把调用方提供的、已经提交的 `RuntimeEvent` 历史，重新整理为可查询的
//! `BillingProjectionSnapshot`。上游事实由账单成本观察（BQ-13）、正式账本记录与修正
//!（BQ-14）、成本分摊记录（BQ-19）产生；事实权威仍是 EventLog，本文件只在内存中重放
//! 它们，不追加事件、不批准操作，也不决定是否可以扣费。
//!
//! 数据流：
//! 已提交事件历史（由调用方读取并传入）
//!     ↓
//! `BillingLedgerProjector::apply_page()` / `rebuild()`
//!     ↓ 识别账单事件、校验事件内容与事件信封、折叠为当前事实
//!     ↓ 无法解释的账单事实进入 quarantine；非账单事件只推进来源位置
//! `BillingProjectionSnapshot`
//!     ↓
//! 查询层可以把快照交给 `billing_api` 一类适配器转换成响应
//!
//! 仓内调用核对：当前可见的直接调用在 BQ-20 测试；crate 根会公开重导出这些 API，
//! 但仅凭导出不能推断已有产品路由在调用它。调用方负责先取得完整、连续且已提交的
//! 事件列表，并负责把返回快照交给下一层。
//!
//! 【设计原因】
//! 单看总金额无法还原某个 attempt 的最新成本版本，也不能核对账本修正目标；因此内部
//! 保留按 attempt、账本 entry、usage 和 correction ID 索引的事实，再从这些事实重算摘要。
//! 每一页先在克隆出的候选状态里处理，只有整页处理和快照校验成功后才替换当前状态，
//! 这样顶层失败不会留下“游标前进、金额未完整更新”的半成品。单条账单事实格式错误时，
//! 则记录隔离原因并继续读取后续事件，避免坏事实被悄悄丢弃或阻断整个历史。
//!
//! Domain 快照含有自摘要，用于发现字段与摘要不一致；自摘要不是带密钥的签名，也不能证明
//! 快照由可信调用者生成。调用方仍需在自己的信任边界内确认传入历史与快照来源。

use kiana_domain::{
    json_digest, AttemptId, BillingProjectionCursor, BillingProjectionSnapshot,
    BillingQuarantineRecord, BillingRollupTotals, CostAllocation, CostBreakdown, CostBreakdownKind,
    CostCorrection, CostLedgerEntry, EventCursor, EventId, EventStoreHealth, JournalPage,
    LedgerEntryId, RuntimeEvent, SchemaVersion, UsageId, BILLING_PROJECTION_NUMBER,
    BILLING_WINDOW_MILLIS, COST_ALLOCATION_EVENT, COST_CORRECTION_EVENT, COST_EVENT_ESTIMATED,
    COST_EVENT_MEASURED, COST_EVENT_UNKNOWN, COST_LEDGER_ENTRY_EVENT,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// 对外暴露的投影器契约名称；当前实现主要用于识别版本，没有写进 Domain 快照字段。
pub const BILLING_LEDGER_PROJECTOR_SCHEMA: &str = "kiana.billing-ledger-projector.v1";
/// 投影器自身的版本标识；快照实际保存的 projection version 由 Domain 常量管理。
pub const BILLING_LEDGER_PROJECTOR_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 供能力目录或静态检查读取的只读声明，不是运行时权限拦截器。
pub const BILLING_LEDGER_PROJECTOR_IS_READ_ONLY: bool = true;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
/// 投影过程中可供调用方识别的失败类别。
///
/// 【作用】区分来源 epoch/游标问题、重复事件、快照不变量失败以及账单数据无效。
/// 【调用者】`BillingLedgerProjector` 的构造、分页和重建 API 返回此类型；当前仓内直接
/// 调用者是测试，未来适配器需要把它映射到自己的错误响应或恢复策略。
/// 【影响】`apply_page()` 返回此错误时，候选页不会发布到原投影器；单个账单事实的普通
/// 解析/校验错误则通常会被转换为 quarantine 记录，而不是直接返回此 enum。
pub enum BillingLedgerProjectionError {
    /// 创建或使用分页器时，来源世代为 0 或与原绑定世代不一致。
    #[error("billing_projection_source_epoch_invalid")]
    SourceEpochInvalid,
    /// 输入游标与空页/来源范围不匹配，或游标算术无法表示。
    #[error("billing_projection_source_cursor_invalid")]
    SourceCursorInvalid,
    /// 页末游标没有覆盖预期的连续事件区间。
    #[error("billing_projection_source_cursor_gap")]
    SourceCursorGap,
    /// 输入页游标早于当前 projector 已发布游标。
    #[error("billing_projection_source_replay")]
    SourceReplay,
    /// 当前重放范围再次出现已见事件 ID；整页不会发布。
    #[error("billing_projection_source_event_duplicate")]
    SourceEventDuplicate,
    /// Domain 快照创建或校验失败；字符串原因由 Domain 契约给出。
    #[error("billing_projection_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
    /// 投影自身的不变量失败，例如隔离容量耗尽或金额无法安全相加。
    #[error("billing_projection_invalid:{0}")]
    Invalid(String),
}

/// 将一个运行时事件与它在完整事件历史中的逻辑位置配对。
///
/// `RuntimeEvent.sequence` 是请求内部的顺序号，不足以表示整个事件历史的位置；
/// `source_cursor` 才是调用方读到该事件时的全局来源游标。若把两者混用，跨请求的事件
/// 可能拥有相同 sequence，增量恢复就无法可靠识别重放或缺页。
///
/// 当前投影器的主要重建入口接收连续的 `RuntimeEvent` 切片，因此本类型是显式表达
/// “事件 + 全局位置”的公开契约；仓内搜索目前只找到类型定义和 crate 根重导出，没有找到
/// 生产代码或测试使用它。
#[derive(Clone, Debug, PartialEq)]
pub struct BillingSourceEvent {
    /// 事件在来源历史中的全局位置，从 1 开始且不能为 0。
    pub source_cursor: EventCursor,
    /// 事件载荷和请求内元数据；真正的来源位置单独保存在上方字段。
    pub event: RuntimeEvent,
}

impl BillingSourceEvent {
    /// 创建并检查一个“事件 + 来源位置”配对。
    ///
    /// 【输入】事件历史游标和运行时事件。
    /// 【输出】成功返回配对值；游标为 0 或事件 ID 为 nil 时返回固定原因字符串。
    /// 【副作用】无，只构造内存值。
    /// 【设计原因】这里先挡住无法参与连续重放的身份值，避免把无效来源包装成看似有效
    /// 的事件项。该 helper 不替调用者验证相邻事件是否连续。
    pub fn new(source_cursor: EventCursor, event: RuntimeEvent) -> Result<Self, String> {
        if source_cursor == 0 || event.event_id.as_uuid().is_nil() {
            return Err("billing_source_event_invalid".to_owned());
        }
        Ok(Self {
            source_cursor,
            event,
        })
    }
}

#[derive(Clone)]
/// 折叠后的最新用量成本事实，以及它来自哪一条来源事件。
///
/// 同一 `AttemptId` 允许从 Unknown 到 Estimated 再到 Measured 的有序版本更新；映射中
/// 只保留当前版本，重建汇总时因此不会把同一次调用的多个修订重复累计。
struct UsageFact {
    /// 产生当前修订的事件全局位置；当前 rollup 重算没有读取此字段。
    cursor: EventCursor,
    /// 同一 attempt 内必须连续递增的成本版本号。
    revision: u64,
    /// 已通过 Domain 校验的成本内容。
    fact: CostBreakdown,
    /// 从事件数据中提取的正数时间；缺失时仍计入总用量，但无法进入日/小时汇总。
    occurred_at_unix_ms: Option<u64>,
}

#[derive(Clone)]
/// 一条正式账本 entry 及其事件历史位置；与用量观察分开统计，避免把观察当作正式入账。
struct LedgerFact {
    /// entry 来源事件的全局位置；当前 rollup 重算没有读取此字段。
    cursor: EventCursor,
    /// 不可变账本事实本身。
    fact: CostLedgerEntry,
}

#[derive(Clone)]
/// 一条 usage 到成本维度的已校验分摊事实。
struct AllocationFact {
    /// 分摊事实来源事件的全局位置；当前 rollup 重算没有读取此字段。
    cursor: EventCursor,
    /// 分摊内容；汇总时单独计入 allocation 视图，不再累加进正式账本总额。
    fact: CostAllocation,
}

#[derive(Clone)]
/// 针对既有账本 entry 的成本修正事实。
struct CorrectionFact {
    /// 修正来源事件的全局位置；当前 rollup 重算没有读取此字段。
    cursor: EventCursor,
    /// 已通过目标 entry 身份绑定校验的修正内容。
    fact: CostCorrection,
}

/// 有状态的账单事件折叠器。
///
/// 【架构角色】这是 `kiana-query` 内部的临时重建状态，不是新的事实存储。字段保持私有，
/// 避免调用方绕过连续游标、重复 ID 和事实校验直接篡改累计状态。对外只提供只读快照；
/// 新进程应从完整已提交历史重建内部事实，而不能仅凭聚合总数继续折叠。
///
/// 【上下游】调用方传入 EventLog 历史页；本结构依赖 `kiana-domain` 的稳定账单契约和
/// reducer 校验，最终产生 Domain 定义的快照。它没有持有存储、授权或执行能力。
#[derive(Clone)]
pub struct BillingLedgerProjector {
    /// 重建所绑定的来源世代；世代变化代表来源历史身份变化，不能混着继续追加。
    source_epoch: u64,
    /// 当前已发布的汇总读模型和来源游标。
    snapshot: BillingProjectionSnapshot,
    /// 本轮重建见过的全部事件 ID，用于拒绝跨页重放；快照只保留有界 ID 尾部。
    seen_event_ids: BTreeSet<EventId>,
    /// 每个 attempt 的最新成本版本。
    usage: BTreeMap<AttemptId, UsageFact>,
    /// 已验证的正式账本 entry，按 entry ID 保证唯一。
    ledger: BTreeMap<LedgerEntryId, LedgerFact>,
    /// 已验证的分摊，按 usage ID 保证一个 usage 不重复归属。
    allocations: BTreeMap<UsageId, AllocationFact>,
    /// 已验证的修正，按 correction ID 保证唯一。
    corrections: BTreeMap<kiana_domain::CostCorrectionId, CorrectionFact>,
}

impl std::fmt::Debug for BillingLedgerProjector {
    /// 只输出定位重建进度所需的摘要，避免 Debug 日志展开完整账单数据。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BillingLedgerProjector")
            .field("source_epoch", &self.source_epoch)
            .field("source_cursor", &self.snapshot.source.source_cursor)
            .field("quarantine", &self.snapshot.quarantine.len())
            .finish_non_exhaustive()
    }
}

impl BillingLedgerProjector {
    /// 建立一个从游标 0 开始的空重建器。
    ///
    /// 【调用者】全量重放入口 `rebuild()`，或需要逐页重放的查询适配器。
    /// 【输入】`source_epoch` 标识本次事件历史所属的来源世代，必须大于 0。
    /// 【输出】成功时包含空快照和空事实索引。
    /// 【失败】0 会得到 `SourceEpochInvalid`；Domain 空快照若不满足自身契约，则映射为
    /// `SnapshotInvalid`。错误由上层调用者决定如何呈现；此处不重试、不写状态。
    /// 【设计原因】空快照是重放的起点，但不能跨 epoch 复用，否则相同游标可能指向另一
    /// 份事件历史。
    pub fn new(source_epoch: u64) -> Result<Self, BillingLedgerProjectionError> {
        if source_epoch == 0 {
            return Err(BillingLedgerProjectionError::SourceEpochInvalid);
        }
        let snapshot = BillingProjectionSnapshot::empty(source_epoch)
            .map_err(BillingLedgerProjectionError::SnapshotInvalid)?;
        Ok(Self {
            source_epoch,
            snapshot,
            seen_event_ids: BTreeSet::new(),
            usage: BTreeMap::new(),
            ledger: BTreeMap::new(),
            allocations: BTreeMap::new(),
            corrections: BTreeMap::new(),
        })
    }

    /// 检查一个序列化快照，但拒绝仅凭聚合结果恢复增量折叠。
    ///
    /// 【输入】外部保存或传递回来的投影快照。
    /// 【输出】快照本身合法仍会返回 `billing_projection_snapshot_requires_rebuild`。
    /// 【为什么】聚合总额没有包含每个 attempt 的前一修订、账本 entry 目标和已见 ID 集合；
    /// 如果从总额继续增量折叠，就无法正确判断新事件是合法后继还是重放/修正旧事实。
    /// 调用方必须回到已提交事件历史重建，再应用尾页。
    /// 【副作用】无；失败不会改变传入快照。
    pub fn from_snapshot(
        snapshot: BillingProjectionSnapshot,
    ) -> Result<Self, BillingLedgerProjectionError> {
        snapshot
            .validate()
            .map_err(BillingLedgerProjectionError::SnapshotInvalid)?;
        // 快照可以用于读取，但不足以恢复逐条事实索引。这里有意失败，避免调用方误以为
        // 已能安全地从当前总额继续追加事件。
        Err(BillingLedgerProjectionError::Invalid(
            "billing_projection_snapshot_requires_rebuild".to_owned(),
        ))
    }

    /// 返回当前已完整发布的快照，只读借用以避免调用方直接替换内部结果。
    pub fn snapshot(&self) -> &BillingProjectionSnapshot {
        &self.snapshot
    }

    /// 返回快照所覆盖的最后一个来源位置；它不是请求内 `RuntimeEvent.sequence`。
    pub fn source_cursor(&self) -> EventCursor {
        self.snapshot.source.source_cursor
    }

    /// 返回重建器绑定的来源世代，用于检查分页调用是否仍指向同一份历史。
    pub fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    /// 原子地折叠一页连续事件，并在整页成功后发布新快照。
    ///
    /// 【调用者】逐页读取已提交事件的查询适配器；仓内当前直接调用来自测试。
    /// 【输入】`page.events` 是上一游标之后的连续事件；`page.cursor` 应等于该页最后一项
    /// 的全局游标；`source_epoch` 必须与创建 projector 时一致。
    /// 【输出】成功时快照和来源游标一起前进。无法解码/验证的单条账单事件会进入 quarantine，
    /// 其余页事件仍继续处理。
    /// 【失败】epoch 不符、游标倒退/跳跃、事件 ID 重复、隔离容量耗尽或最终快照无效会返回
    /// 错误。错误向调用方返回；此页在克隆状态中执行，原状态保持不变，因此不会自动重试、
    /// 禁用来源账号或追加新事实。
    /// 【核心流程】先检查页边界，再克隆整个内部索引；按全局游标逐条折叠；重算全部摘要；
    /// 最后用一次赋值替换 `self`。
    /// 【为什么克隆】只在结束时整体发布，避免失败页留下部分金额和已前进游标。成本是页处理
    /// 会复制当前索引；这是以简单、可审计的原子边界换取的内存开销。
    pub fn apply_page(
        &mut self,
        page: &kiana_domain::JournalPage,
        source_epoch: u64,
    ) -> Result<(), BillingLedgerProjectionError> {
        if source_epoch == 0 || source_epoch != self.source_epoch {
            return Err(BillingLedgerProjectionError::SourceEpochInvalid);
        }
        let after = self.source_cursor();
        if page.cursor < after {
            return Err(BillingLedgerProjectionError::SourceReplay);
        }
        if page.events.is_empty() {
            if page.cursor != after {
                return Err(BillingLedgerProjectionError::SourceCursorInvalid);
            }
            return Ok(());
        }
        let first_cursor = after
            .checked_add(1)
            .ok_or(BillingLedgerProjectionError::SourceCursorInvalid)?;
        let expected_last = first_cursor
            .checked_add(page.events.len() as u64 - 1)
            .ok_or(BillingLedgerProjectionError::SourceCursorInvalid)?;
        if page.cursor != expected_last {
            return Err(BillingLedgerProjectionError::SourceCursorGap);
        }
        let mut candidate = self.clone();
        for (offset, event) in page.events.iter().cloned().enumerate() {
            let cursor = first_cursor + offset as u64;
            candidate.apply_event(cursor, event)?;
        }
        candidate.rebuild_snapshot(page.cursor)?;
        *self = candidate;
        Ok(())
    }

    /// 从游标 0 开始，将整段连续事件历史重建成一份快照。
    ///
    /// 【调用者】公开便捷函数 `project_billing_ledger()`；该函数本身当前由 BQ-20 测试调用，
    /// 仓内未找到产品路由直接调用。
    /// 【输入】来源世代、完整事件列表末尾的全局游标、按来源顺序排列的事件切片。
    /// 【输出】新建并完成重放的账单投影快照。
    /// 【失败】游标与事件数不匹配、重复事件、来源世代无效、隔离超限或投影不变量失败时
    /// 返回结构化投影错误；调用方负责决定展示或重新读取来源。
    /// 【副作用】只分配临时内存，不获取 EventLog，也不改动它。
    /// 【设计原因】将 I/O 与确定性折叠分离，调用者必须显式传入已提交事实，便于重放和审计。
    pub fn rebuild(
        source_epoch: u64,
        source_cursor: EventCursor,
        events: &[RuntimeEvent],
    ) -> Result<BillingProjectionSnapshot, BillingLedgerProjectionError> {
        if events.is_empty() {
            if source_cursor != 0 {
                return Err(BillingLedgerProjectionError::SourceCursorInvalid);
            }
            return Self::new(source_epoch).map(|projector| projector.snapshot.clone());
        }
        let mut projector = Self::new(source_epoch)?;
        let page = JournalPage {
            events: events.to_vec(),
            cursor: source_cursor,
            has_more: false,
        };
        projector.apply_page(&page, source_epoch)?;
        Ok(projector.snapshot.clone())
    }

    fn apply_event(
        &mut self,
        source_cursor: EventCursor,
        event: RuntimeEvent,
    ) -> Result<(), BillingLedgerProjectionError> {
        // 这个 ID 集合覆盖本轮重放见过的事件，而快照里的 source_event_ids 只是有界尾部。
        // 因此即使旧 ID 已从快照尾部裁掉，当前进程仍能识别跨页重放。
        if !self.seen_event_ids.insert(event.event_id) {
            return Err(BillingLedgerProjectionError::SourceEventDuplicate);
        }
        // replay_fence 把上一位置、当前全局游标和事件 ID 串成哈希链；它是重放顺序摘要，
        // 不是带密钥的签名，也不能单独证明事件来自可信存储。
        let fence = next_replay_fence(
            &self.snapshot.source.replay_fence,
            source_cursor,
            event.event_id,
        );
        self.snapshot.source.replay_fence = fence;
        self.snapshot.source.source_cursor = source_cursor;
        self.snapshot.source.source_event_ids.push(event.event_id);
        if self.snapshot.source.source_event_ids.len() > kiana_domain::MAX_BILLING_SOURCE_EVENT_IDS
        {
            let trim = self.snapshot.source.source_event_ids.len()
                - kiana_domain::MAX_BILLING_SOURCE_EVENT_IDS;
            self.snapshot.source.source_event_ids.drain(..trim);
        }

        // 只有四类已定义账单事实会进入金额视图。其他 RuntimeEvent 仍属于来源历史：它们
        // 推进游标和 replay fence，但不会被误解释为费用或预约消费。
        match event.kind.as_str() {
            COST_EVENT_ESTIMATED | COST_EVENT_MEASURED | COST_EVENT_UNKNOWN => {
                // 某一账单事件内容损坏时保留它的身份和摘要，再继续本页后续事实。
                if let Err(reason) = self.fold_usage(source_cursor, &event) {
                    self.quarantine(source_cursor, &event, reason)?;
                }
            }
            COST_LEDGER_ENTRY_EVENT => {
                if let Err(reason) = self.fold_ledger(source_cursor, &event) {
                    self.quarantine(source_cursor, &event, reason)?;
                }
            }
            COST_CORRECTION_EVENT => {
                if let Err(reason) = self.fold_correction(source_cursor, &event) {
                    self.quarantine(source_cursor, &event, reason)?;
                }
            }
            COST_ALLOCATION_EVENT => {
                if let Err(reason) = self.fold_allocation(source_cursor, &event) {
                    self.quarantine(source_cursor, &event, reason)?;
                }
            }
            // Reservations, approval lifecycle and all other RuntimeEvents are not billing facts
            // consumed by this projection. Ignoring them is deliberate and does not erase them.
            _ => {}
        }
        Ok(())
    }

    /// 解码并折叠一次 attempt 的成本观察或其后续修订。
    ///
    /// 【输入来源】事件 `data` 是序列化的 `CostBreakdown`；`aggregate_type/id` 和
    /// `stream_version` 来自事件外层信封。
    /// 【输出/状态变化】验证成功后，以 attempt ID 为键保存最新版本；旧版本被新版本替代，
    /// 因此摘要只统计一次 attempt 的当前成本状态。
    /// 【失败处理】解码失败、kind 与载荷不匹配、缺少 attempt、信封指向别的 attempt、版本
    /// 不连续、usage digest 改变或状态迁移不允许时返回原因字符串。`apply_event()` 会把该
    /// 账单事件放入隔离列表；若隔离列表已满，则整页失败且不会发布。
    /// 【为什么多重校验】载荷和信封由不同字段组成，只相信其中之一可能把一条成本数据归到
    /// 错误的调用上；revision 和 digest 则防止把不同输入的成本伪装成同一 attempt 的修订。
    fn fold_usage(&mut self, cursor: EventCursor, event: &RuntimeEvent) -> Result<(), String> {
        let fact: CostBreakdown = serde_json::from_value(event.data.clone())
            .map_err(|_| "usage_cost_decode_failed".to_owned())?;
        fact.validate()?;
        if fact.kind.event_kind() != event.kind {
            return Err("usage_cost_kind_mismatch".to_owned());
        }
        let attempt_id = fact
            .attempt_id
            .ok_or_else(|| "usage_cost_attempt_missing".to_owned())?;
        if event.aggregate_type.as_deref() != Some("billing_attempt")
            || event.aggregate_id.as_deref() != Some(attempt_id.to_string().as_str())
        {
            return Err("usage_cost_stream_mismatch".to_owned());
        }
        let revision = event
            .stream_version
            .filter(|revision| *revision > 0)
            .ok_or_else(|| "usage_cost_revision_missing".to_owned())?;
        if let Some(previous) = self.usage.get(&attempt_id) {
            // 所有修订必须保持同一 usage digest 且紧邻前一 revision。允许的状态只有
            // Unknown -> Estimated/Measured 和 Estimated -> Measured，不能倒退或重复确认。
            // 注意：这里用 saturating_add；在 u64::MAX 边界它不会溢出报错，见本批发现记录。
            if revision != previous.revision.saturating_add(1)
                || previous.fact.usage_digest != fact.usage_digest
            {
                return Err("usage_cost_revision_gap".to_owned());
            }
            let allowed = matches!(
                (&previous.fact.kind, &fact.kind),
                (
                    CostBreakdownKind::Unknown { .. },
                    CostBreakdownKind::Estimated { .. }
                ) | (
                    CostBreakdownKind::Unknown { .. },
                    CostBreakdownKind::Measured { .. }
                ) | (
                    CostBreakdownKind::Estimated { .. },
                    CostBreakdownKind::Measured { .. }
                )
            );
            if !allowed {
                return Err("usage_cost_transition_invalid".to_owned());
            }
        }
        // 成本事实允许缺少时间戳；此时全局用量仍能累计，但下面的日/窗口分桶会跳过它。
        let occurred_at_unix_ms = event_time(&event.data);
        self.usage.insert(
            attempt_id,
            UsageFact {
                cursor,
                revision,
                fact,
                occurred_at_unix_ms,
            },
        );
        Ok(())
    }

    /// 解码并折叠一条正式成本账本 entry。
    ///
    /// 【输入】事件载荷中的 `CostLedgerEntry`，以及信封中的账本聚合身份和版本号。
    /// 【输出】按 entry ID 保存一次原始账本事实，供总额、时间桶和修正目标核对使用。
    /// 【失败处理】Domain 校验失败、聚合类型/ID/版本和载荷不一致、或 entry ID 重复时，
    /// 返回原因并由 `apply_event()` 记录隔离；如果隔离空间耗尽则整页失败。这里不推测缺失
    /// 金额，也不把格式错误的条目当成 Unknown 成本。
    /// 【设计原因】正式账本与成本观察代表不同业务事实。分开存储和累计可避免把一次观察
    /// 再次当作已经入账的金额。
    fn fold_ledger(&mut self, cursor: EventCursor, event: &RuntimeEvent) -> Result<(), String> {
        let fact: CostLedgerEntry = serde_json::from_value(event.data.clone())
            .map_err(|_| "ledger_entry_decode_failed".to_owned())?;
        fact.validate()?;
        if event.aggregate_type.as_deref() != Some("cost_ledger")
            || event.aggregate_id.as_deref() != Some(fact.entry_id.to_string().as_str())
            || event.stream_version != Some(fact.revision)
        {
            return Err("ledger_entry_stream_mismatch".to_owned());
        }
        if self.ledger.contains_key(&fact.entry_id) {
            return Err("ledger_entry_duplicate".to_owned());
        }
        self.ledger
            .insert(fact.entry_id, LedgerFact { cursor, fact });
        Ok(())
    }

    /// 验证修正确实引用了本次重放中已经出现的原账本 entry。
    ///
    /// 修正载荷不仅要通过 Domain 自身校验，还要与已保存目标的 digest、run、原始来源游标
    /// 和 revision 全部相符；因此事件顺序上目标必须先于修正。修正 ID 重复或目标不匹配时
    /// 事件被隔离。修正只进入 correction 集合，后续在 rollup 中单独计入修正栏，不改写原 entry。
    ///
    /// 【边界】当前这里仅要求 envelope 的 stream version 非零；它没有另外比较同一账本
    /// aggregate 上连续修正事件的版本号。该行为是静态阅读发现，不能用本注释推断为已修复。
    fn fold_correction(&mut self, cursor: EventCursor, event: &RuntimeEvent) -> Result<(), String> {
        let fact: CostCorrection = serde_json::from_value(event.data.clone())
            .map_err(|_| "correction_decode_failed".to_owned())?;
        fact.validate()?;
        if event.aggregate_type.as_deref() != Some("cost_ledger")
            || event.aggregate_id.as_deref() != Some(fact.target_entry_id.to_string().as_str())
            || event.stream_version.is_none_or(|version| version == 0)
        {
            return Err("correction_stream_mismatch".to_owned());
        }
        let Some(target) = self.ledger.get(&fact.target_entry_id) else {
            return Err("correction_target_missing".to_owned());
        };
        if fact.target_entry_digest != target.fact.entry_digest
            || fact.target_run_id != target.fact.run_id
            || fact.target_source_cursor != target.fact.source_cursor
            || fact.target_revision != target.fact.revision
        {
            return Err("correction_target_mismatch".to_owned());
        }
        if self.corrections.contains_key(&fact.correction_id) {
            return Err("correction_duplicate".to_owned());
        }
        self.corrections
            .insert(fact.correction_id, CorrectionFact { cursor, fact });
        Ok(())
    }

    /// 验证并保存 usage 分摊事实。
    ///
    /// 分摊属于“这笔成本应归到哪些组织/项目/任务维度”的归因视图，不能替代成本观察或
    /// 正式账本。事件必须绑定 `cost_allocation` aggregate、分摊 ID 和其 revision；同一个
    /// usage ID 只允许一条分摊。失败会由事件分发层记入 quarantine，成功后仅影响独立的
    /// allocation totals，不会修改 ledger totals。
    fn fold_allocation(&mut self, cursor: EventCursor, event: &RuntimeEvent) -> Result<(), String> {
        let fact: CostAllocation = serde_json::from_value(event.data.clone())
            .map_err(|_| "allocation_decode_failed".to_owned())?;
        fact.validate()?;
        if event.aggregate_type.as_deref() != Some("cost_allocation")
            || event.aggregate_id.as_deref() != Some(fact.allocation_id.to_string().as_str())
            || event.stream_version != Some(fact.revision)
        {
            return Err("allocation_stream_mismatch".to_owned());
        }
        if self.allocations.contains_key(&fact.usage_id) {
            return Err("allocation_usage_duplicate".to_owned());
        }
        self.allocations
            .insert(fact.usage_id, AllocationFact { cursor, fact });
        Ok(())
    }

    /// 将无法作为有效账单事实使用的已提交事件保留为诊断记录。
    ///
    /// 【输入】来源游标、原事件和拒绝原因；记录还保存载荷摘要，便于定位内容而无需把整份
    /// 原始数据复制进读模型。
    /// 【输出】快照 quarantine 增加一条记录，不会删除或改写来源事件。
    /// 【失败】达到 Domain 规定的隔离容量上限后返回投影错误；该错误会使当前整页回滚。
    /// 【为什么保留】直接跳过坏事件会让查询结果看似完整，却无法解释总额为什么与来源不符。
    fn quarantine(
        &mut self,
        source_cursor: EventCursor,
        event: &RuntimeEvent,
        reason: String,
    ) -> Result<(), BillingLedgerProjectionError> {
        if self.snapshot.quarantine.len() >= kiana_domain::MAX_BILLING_QUARANTINE {
            return Err(BillingLedgerProjectionError::Invalid(
                "billing_quarantine_limit".to_owned(),
            ));
        }
        let record = BillingQuarantineRecord::new(
            source_cursor,
            event.event_id,
            event.kind.clone(),
            reason,
            json_digest(&event.data),
        )
        .map_err(BillingLedgerProjectionError::Invalid)?;
        self.snapshot.quarantine.push(record);
        Ok(())
    }

    fn rebuild_snapshot(
        &mut self,
        source_cursor: EventCursor,
    ) -> Result<(), BillingLedgerProjectionError> {
        // 每页都从当前已折叠事实重算摘要。这样 usage 修订替换旧版本时，金额不会因增量
        // “再加一次”而重复；代价是本函数遍历目前保留的全部账单事实。
        let mut usage_totals = BillingRollupTotals::default();
        let mut ledger_totals = BillingRollupTotals::default();
        let mut allocation_totals = BillingRollupTotals::default();
        let mut daily_rollups: BTreeMap<String, BillingRollupTotals> = BTreeMap::new();
        let mut window_rollups: BTreeMap<u64, BillingRollupTotals> = BTreeMap::new();

        // 用量观察总额只看每个 attempt 的最新成本事实。没有可用时间戳的用量仍计入总量，
        // 但无法准确归入某个日期或固定时长窗口。
        for usage in self.usage.values() {
            usage_totals.usage_count = usage_totals.usage_count.saturating_add(1);
            add_cost_kind(&mut usage_totals, &usage.fact.kind, false)?;
            if let Some(timestamp) = usage.occurred_at_unix_ms {
                add_bucket(
                    &mut daily_rollups,
                    &mut window_rollups,
                    timestamp,
                    |totals| {
                        totals.usage_count = totals.usage_count.saturating_add(1);
                        add_cost_kind(totals, &usage.fact.kind, false)
                    },
                )?;
            }
        }
        // 正式账本单独累计，并按账本事实创建时间进入日/窗口视图。
        for ledger in self.ledger.values() {
            ledger_totals.ledger_entry_count = ledger_totals.ledger_entry_count.saturating_add(1);
            add_ledger_kind(&mut ledger_totals, &ledger.fact, false)?;
            add_bucket(
                &mut daily_rollups,
                &mut window_rollups,
                ledger.fact.created_at_unix_ms,
                |totals| {
                    totals.ledger_entry_count = totals.ledger_entry_count.saturating_add(1);
                    add_ledger_kind(totals, &ledger.fact, false)
                },
            )?;
        }
        // 修正以独立 adjustment 数值体现，保留原始 entry 作为审计基线。
        for correction in self.corrections.values() {
            add_correction_kind(&mut ledger_totals, &correction.fact)?;
            add_bucket(
                &mut daily_rollups,
                &mut window_rollups,
                correction.fact.created_at_unix_ms,
                |totals| add_correction_kind(totals, &correction.fact),
            )?;
        }
        // 分摊金额只进入 allocation 汇总；若并入 ledger，就会对同一成本重复计数。
        // 当前实现也没有为 allocation 构造日期/窗口 rollup。
        for allocation in self.allocations.values() {
            allocation_totals.allocation_count =
                allocation_totals.allocation_count.saturating_add(1);
            add_allocation_kind(&mut allocation_totals, &allocation.fact.cost)?;
        }

        // 快照只携带 Domain 允许的有限来源 ID 尾部；全量重复检测仍由 projector 私有集合承担。
        let mut ids = self.snapshot.source.source_event_ids.clone();
        ids.dedup();
        let source = BillingProjectionCursor::new(
            self.source_epoch,
            source_cursor,
            BILLING_PROJECTION_NUMBER,
            self.snapshot.source.replay_fence.clone(),
            ids,
        )
        .map_err(BillingLedgerProjectionError::SnapshotInvalid)?;
        self.snapshot = BillingProjectionSnapshot::new(
            source,
            usage_totals,
            ledger_totals,
            allocation_totals,
            daily_rollups,
            window_rollups,
            self.snapshot.quarantine.clone(),
        )
        .map_err(BillingLedgerProjectionError::SnapshotInvalid)?;
        Ok(())
    }
}

/// 把一类成本观察计入总额中的估算、实测或未知栏。
///
/// 【调用者】重建总量和时间桶时处理 `CostBreakdownKind`。
/// 【输入】目标 totals、成本类别、是否属于修正栏。
/// 【输出】金额成功累加则返回 `Ok(())`；Unknown 只增加未知条数。
/// 【失败】估算缺少金额或 Money 币种/加法校验失败时返回投影错误，并由 `rebuild_snapshot()`
/// 终止候选快照生成。这里通过 Domain 的 checked-add 逻辑拒绝溢出或不兼容金额，不使用浮点数。
/// 【设计原因】Unknown 不是零金额：它表达“来源没有可靠金额”，所以不能伪造一个零元数值。
fn add_cost_kind(
    totals: &mut BillingRollupTotals,
    kind: &CostBreakdownKind,
    correction: bool,
) -> Result<(), BillingLedgerProjectionError> {
    match kind {
        CostBreakdownKind::Estimated { estimate } => {
            let amount = estimate.amount.as_ref().ok_or_else(|| {
                BillingLedgerProjectionError::Invalid("estimated_amount_missing".to_owned())
            })?;
            if correction {
                totals
                    .add_correction_estimated(amount)
                    .map_err(BillingLedgerProjectionError::Invalid)
            } else {
                totals
                    .add_estimated(amount)
                    .map_err(BillingLedgerProjectionError::Invalid)
            }
        }
        CostBreakdownKind::Measured { amount, .. } => {
            if correction {
                totals
                    .add_correction_measured(amount)
                    .map_err(BillingLedgerProjectionError::Invalid)
            } else {
                totals
                    .add_measured(amount)
                    .map_err(BillingLedgerProjectionError::Invalid)
            }
        }
        CostBreakdownKind::Unknown { .. } => {
            totals.unknown_count = totals.unknown_count.saturating_add(1);
            Ok(())
        }
    }
}

/// 将正式账本 entry 的估算和实测金额分别累加。
///
/// entry 可以同时包含两类成本，也可以两者都没有；最后一种情况只增加 unknown_count。
/// `correction` 决定金额进入原始金额栏还是 correction 栏。BQ-20 当前遍历正式 entry 时传
/// false；修正记录通过 `add_correction_kind()` 走独立路径。
fn add_ledger_kind(
    totals: &mut BillingRollupTotals,
    fact: &CostLedgerEntry,
    correction: bool,
) -> Result<(), BillingLedgerProjectionError> {
    if let Some(amount) = fact.estimated_cost.as_ref() {
        if correction {
            totals
                .add_correction_estimated(amount)
                .map_err(BillingLedgerProjectionError::Invalid)?;
        } else {
            totals
                .add_estimated(amount)
                .map_err(BillingLedgerProjectionError::Invalid)?;
        }
    }
    if let Some(amount) = fact.measured_cost.as_ref() {
        if correction {
            totals
                .add_correction_measured(amount)
                .map_err(BillingLedgerProjectionError::Invalid)?;
        } else {
            totals
                .add_measured(amount)
                .map_err(BillingLedgerProjectionError::Invalid)?;
        }
    }
    if fact.estimated_cost.is_none() && fact.measured_cost.is_none() {
        totals.unknown_count = totals.unknown_count.saturating_add(1);
    }
    Ok(())
}

/// 把一条修正中的估算/实测差额计入 correction 专用金额栏。
///
/// 输入已在折叠阶段绑定到原 entry；此处只做金额汇总，不重新判断修正是否获批或目标是否存在。
/// 任一金额校验或 checked-add 失败都会向上返回并使候选快照不发布。
fn add_correction_kind(
    totals: &mut BillingRollupTotals,
    fact: &CostCorrection,
) -> Result<(), BillingLedgerProjectionError> {
    if let Some(amount) = fact.delta_estimated.as_ref() {
        totals
            .add_correction_estimated(amount)
            .map_err(BillingLedgerProjectionError::Invalid)?;
    }
    if let Some(amount) = fact.delta_measured.as_ref() {
        totals
            .add_correction_measured(amount)
            .map_err(BillingLedgerProjectionError::Invalid)?;
    }
    Ok(())
}

/// 将归因视图中的 Estimated、Measured 或 Unknown 计入 allocation 汇总。
///
/// 这个汇总和 ledger totals 分开，因为一个 usage 可能对应多个归因维度；把分摊数再加到
/// 正式账本会把同一笔成本重复计入。金额相加继续复用 Domain 的币种校验与溢出检查。
fn add_allocation_kind(
    totals: &mut BillingRollupTotals,
    kind: &kiana_domain::AllocationCostKind,
) -> Result<(), BillingLedgerProjectionError> {
    match kind {
        kiana_domain::AllocationCostKind::Estimated { amount, .. } => totals
            .add_estimated(amount)
            .map_err(BillingLedgerProjectionError::Invalid),
        kiana_domain::AllocationCostKind::Measured { amount, .. } => totals
            .add_measured(amount)
            .map_err(BillingLedgerProjectionError::Invalid),
        kiana_domain::AllocationCostKind::Unknown { .. } => {
            totals.unknown_count = totals.unknown_count.saturating_add(1);
            Ok(())
        }
    }
}

/// 同时更新 UTC 自然日和固定长度时间窗的两份汇总。
///
/// 【输入】两个有序映射、Unix 毫秒时间戳、用于更新计数/金额的闭包。
/// 【输出】闭包对两份 totals 都成功后返回成功。
/// 【失败】时间戳为 0 或任一 totals 更新失败时返回错误。调用方在临时重建状态中执行，
/// 因而失败不会把半份桶结果发布到 projector。
/// 【为什么更新两次】查询既可能按日看趋势，也可能按固定窗口对齐监控；一个事实同时进入
/// 两个独立视图，但不会因此改变它在主 totals 中的计数。
fn add_bucket(
    daily: &mut BTreeMap<String, BillingRollupTotals>,
    windows: &mut BTreeMap<u64, BillingRollupTotals>,
    timestamp: u64,
    mut add: impl FnMut(&mut BillingRollupTotals) -> Result<(), BillingLedgerProjectionError>,
) -> Result<(), BillingLedgerProjectionError> {
    if timestamp == 0 {
        return Err(BillingLedgerProjectionError::Invalid(
            "billing_fact_timestamp_invalid".to_owned(),
        ));
    }
    // 日键按 UTC 生成，不依赖机器本地时区，确保不同主机重放得到相同分桶。
    let day = utc_day_key(timestamp);
    let daily_totals = daily.entry(day).or_default();
    add(daily_totals)?;
    // 整数除法向下取整到窗口起点；BILLING_WINDOW_MILLIS 是 60 分钟的毫秒数。
    let window = timestamp / BILLING_WINDOW_MILLIS * BILLING_WINDOW_MILLIS;
    let window_totals = windows.entry(window).or_default();
    add(window_totals)?;
    Ok(())
}

/// 按固定优先级从账单事件载荷中找出可用于时间汇总的毫秒时间戳。
///
/// 依次尝试发生时间、观测时间、创建时间；只接受 JSON 无符号整数且大于 0 的值。字段缺失、
/// 类型不符或为 0 时继续检查下一个字段；都不可用时返回 `None`，事实仍可进入非时间总量。
fn event_time(data: &Value) -> Option<u64> {
    [
        "occurred_at_unix_ms",
        "observed_at_unix_ms",
        "created_at_unix_ms",
    ]
    .into_iter()
    .find_map(|field| {
        data.get(field)
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
    })
}

/// 把 Unix epoch 毫秒转成稳定的 `YYYY-MM-DD` UTC 日期键。
///
/// 【设计原因】这里不引入日期时间库，避免 `kiana-domain` 为了一个确定性投影 helper 增加
/// 运行时依赖；算法只做整数运算，因此相同时间戳在不同本地时区会得到同一日期。
/// 86,400,000 是一天的毫秒数。后续 719,468、146,097、36,524、1,460 等常数属于
/// Gregorian civil date 转换算法：分别用于平移 epoch、按 400 年周期分段以及校正闰年。
/// 它们不是产品可配置参数，改动会改变历史日期分桶。
fn utc_day_key(timestamp: u64) -> String {
    // Howard Hinnant 的 civil_from_days 算法：先从毫秒换算完整天数，再分解为 400 年周期、
    // 年内日和月日。输入是从 Unix epoch 起算的无符号毫秒数。
    let days = (timestamp / 86_400_000) as i64;
    // 平移量把 Unix epoch 对齐到 Gregorian 算法使用的 March 起始年序列。
    let z = days + 719_468;
    // 每个 Gregorian 400 年周期固定有 146,097 天；负方向分支用于保持整除向下取整语义。
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    // `yoe` 是当前 400 年周期内的年份序号，除数用于扣除闰年修正。
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// 计算一个来源事件加入重放序列后的哈希链位置。
///
/// 新摘要同时包含旧摘要、全局游标和事件 ID；改变顺序或替换事件会改变后续 fence。
/// `json_digest` 是确定性摘要，不是秘密密钥签名，也不能替代对来源存储和访问者的认证。
fn next_replay_fence(previous: &str, source_cursor: EventCursor, event_id: EventId) -> String {
    json_digest(&serde_json::json!({
        "previous": previous,
        "source_cursor": source_cursor,
        "event_id": event_id,
    }))
}

/// 对外的全量账单投影便捷入口。
///
/// 【作用】把调用方准备的完整连续事件切片交给状态型 projector 重建。
/// 【调用者】模块根 `kiana-query` 会重导出此函数；当前仓内只在 BQ-20 行为测试中看到直接调用，
/// 没有找到生产路由调用。
/// 【输入】来源 epoch、列表末尾游标、按来源顺序排列的 RuntimeEvent 切片。
/// 【输出】校验过的 `BillingProjectionSnapshot`，供后续查询适配器读取。
/// 【失败】错误沿用 projector 的 epoch、游标、重复事件、隔离容量和 snapshot 错误；本函数
/// 不自行重试，也没有地方把它改写成用户可见文案。
/// 【副作用】无外部 I/O；输入切片只被借用，内部创建临时重建器。
/// 【为什么这样设计】依赖由调用方注入，Query 可以纯粹验证“给定这段历史会得到什么视图”，
/// 不会在查询代码里暗中取得事实读取或授权能力。
pub fn project_billing_ledger(
    source_epoch: u64,
    source_cursor: EventCursor,
    events: &[RuntimeEvent],
) -> Result<BillingProjectionSnapshot, BillingLedgerProjectionError> {
    BillingLedgerProjector::rebuild(source_epoch, source_cursor, events)
}

/// 判断健康元数据是否至少具备从头重建所需的来源游标。
///
/// 它只检查 `EventStoreHealth` 自身字段是否合法、且最后持久游标大于 0；不读取账单事件，
/// 不证明事件历史完整、不代表当前 projector 一定可重建，也不等于费用预留或审批状态。
/// 当前仓内未发现直接业务调用者。
pub fn billing_source_is_rebuildable(health: &EventStoreHealth) -> bool {
    health.validate().is_ok() && health.last_durable_cursor > 0
}

/// 仅供静态 source guard 识别此模块的只读边界标记。
///
/// 它是编译期常量，不会在运行时拦截行为；真正的只读事实来自本文件调用图和 guard 的源码
/// 检查范围，不能单独把这个布尔值当作安全证明。
pub const BILLING_PROJECTOR_NO_FACT_WRITES: bool = true;
