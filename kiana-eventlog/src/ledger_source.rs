//! BQ-20 committed source-page adapter.
//!
//! This module only reads the existing `EventStorePort`. It does not create a second ledger or
//! append quarantine/projection facts. A caller persists the query projection separately and must
//! re-read from the last acknowledged cursor after restart.

// ============================================================================
// 中文说明
// ============================================================================
//
// 【本文件负责什么】
// 实现「账本数据源」的语义 —— 把 EventStorePort 中的事件**分页读取**，包装成
// BillingSourcePage 的形式，供账单计算器（billing projector）消费。
// 本文件**只读取**事件、**不创建**第二套账本、**不写**任何补偿事实（如隔离区、投影）。
//
// 【关键纪律：这只是一个**适配层**，不是独立的持久系统】
// 本文件做三件事：
//   1. 读一页事件（read_billing_page / read_ledger_page）；
//   2. 校验页的完整性（cursor 连续性、event count 匹配）；
//   3. 包装成 BillingSourcePage 对象，让投影器用起来方便。
// 调用方（账单投影器）必须：
//   * 自己存储投影结果（如「已计费到 cursor X」）；
//   * 重启后从存储中恢复「上一个已处理的 cursor」；
//   * 重新调 read_billing_page(store, epoch, last_cursor, limit) 继续读。
// 本文件不负责「断点续传」，只负责「校验页的合法性」。
//
// 【架构位置】
// kiana-eventlog 的 crate 级职责是「事件事实的存储适配器」。
// 本文件属于该 crate 中的「阅读适配器」（与 credential_rotation / artifact_store 等同列，
// 都是 EventStorePort 的衍生品）。依赖方向是单向的：
//
//     kiana-domain（值对象 + 校验规则）  <-  kiana-ports（trait 契约）  <-  本文件
//     本文件（适配器实现）  ->  投影器、外部系统（如计费、审计）
//
// 【什么是 source_epoch】
// source_epoch = 数据源的「逻辑纪元」（如「账单 week 号」或「发票生成周期号」）。
// 同一个事件可能被多个不同 epoch 的投影器读取（如周投影 + 月投影），
// 每个投影器维护自己的 epoch，所以不会相互干扰。
// source_epoch 作为参数传入，主要是为了让调用方能区分「这页数据属于哪个投影周期」。
//
// 【pages 的连续性保证】
// read_from(cursor, limit) 从 EventStorePort 返回的页面，保证：
//   * cursor 单调递增（不会倒退）；
//   * events 在逻辑上是连续的（不会因为压缩、归档而有间隙）。
// 本文件做的是「数学检查」：确认返回页面的 cursor 和 event count 一致
// （不存在「收到 N 个事件，但 cursor 说应该有 M 个」的奇形怪状）。
// 这就是 BillingSourcePage::new() 的所有工作。

use kiana_domain::{EventCursor, JournalPage, RuntimeEvent};
use kiana_ports::{EventStorePort, PortError};
use std::collections::BTreeSet;

/// 一页已提交的账本事件。
///
/// 【架构角色】这是本适配器的**唯一**数据结构。
/// 它把 EventStorePort::read_from() 返回的 JournalPage 包装一层，
/// 加上 source_epoch、after_cursor、first_cursor 等元数据，
/// 方便投影器理解「这页数据来自哪个逻辑纪元、从哪里开始、到哪里结束」。
///
/// 【字段含义】
///   * source_epoch: 投影的逻辑纪元（如「billing week 42」）。同一个 EventStorePort
///     可能被多个投影周期消费，每个维护自己的 epoch；
///   * after_cursor: 「这页数据之前」的 cursor（即上一页的最后一个事件的 cursor）；
///   * first_cursor: 「这页数据的第一个」事件的 cursor（= after_cursor + 1，前提是页面非空）；
///   * page: 实际的 JournalPage（包含 cursor 和 events 列表）。
///
/// 【cursor 和 events 的关系】
/// 假如 after_cursor = 100，page.events.len() = 3，那么：
///   * first_cursor = 101（第一个事件）
///   * page.cursor = 103（最后一个事件）
///   * 三个事件的 cursor 分别是 101, 102, 103。
/// 检查逻辑：expected_count = page.cursor - after_cursor == page.events.len()。
/// 如果不相等就是「事件间隙」，报错。
#[derive(Clone, Debug, PartialEq)]
pub struct BillingSourcePage {
    /// 数据源的逻辑纪元（投影器传入）。
    pub source_epoch: u64,
    /// 这页数据**之前**的 cursor（通常是上一页的 page.cursor）。
    pub after_cursor: EventCursor,
    /// 如果本页非空，first_cursor = after_cursor + 1；否则 = None。
    pub first_cursor: Option<EventCursor>,
    /// EventStorePort::read_from() 返回的原始页面。
    pub page: JournalPage,
}

impl BillingSourcePage {
    /// 构造并校验一页账本数据。
    ///
    /// 【参数】
    ///   * source_epoch: 投影器的逻辑纪元（必须 > 0）；
    ///   * after_cursor: 这页数据之前的 cursor；
    ///   * page: EventStorePort::read_from() 返回的原始页面。
    ///
    /// 【校验规则】
    ///   ①source_epoch > 0（否则报 Failed）；
    ///   ②page.cursor >= after_cursor（cursor 不能倒退）；
    ///   ③event count 必须 == (page.cursor - after_cursor)（连续性检查）；
    ///   ④页内每个事件的全局 ID 必须非 nil 且页内唯一。
    ///
    /// 【错误场景】
    ///   * billing_source_epoch_invalid -> source_epoch == 0；
    ///   * billing_source_cursor_regressed -> page.cursor < after_cursor（倒退）；
    ///   * billing_source_cursor_not_contiguous -> event count != cursor delta（间隙）；
    ///   * billing_source_event_id_invalid -> 页内事件 ID 为 nil；
    ///   * billing_source_event_duplicate -> 同一页内出现重复事件 ID。
    ///
    /// 【设计理由】
    /// 这是「防守性编程」：确保投影器拿到的数据一定是合法的、连续的、没有损失的。
    /// 任何异常（epoch 无效、cursor 倒退、事件数不匹配、事件身份无效）都会被及时发现，
    /// 而不是让投影器在处理错误数据时反复踩坑。
    pub fn new(
        source_epoch: u64,
        after_cursor: EventCursor,
        page: JournalPage,
    ) -> Result<Self, PortError> {
        if source_epoch == 0 {
            return Err(PortError::Failed("billing_source_epoch_invalid".to_owned()));
        }
        if page.cursor < after_cursor {
            return Err(PortError::Conflict(
                "billing_source_cursor_regressed".to_owned(),
            ));
        }
        let expected_count = page.cursor - after_cursor;
        if expected_count != page.events.len() as u64 {
            return Err(PortError::Conflict(
                "billing_source_cursor_not_contiguous".to_owned(),
            ));
        }
        let mut seen_event_ids = BTreeSet::new();
        for event in &page.events {
            if event.event_id.as_uuid().is_nil() {
                return Err(PortError::Failed(
                    "billing_source_event_id_invalid".to_owned(),
                ));
            }
            if !seen_event_ids.insert(event.event_id) {
                return Err(PortError::Conflict(
                    "billing_source_event_duplicate".to_owned(),
                ));
            }
        }
        let first_cursor =
            if expected_count > 0 {
                Some(after_cursor.checked_add(1).ok_or_else(|| {
                    PortError::Failed("billing_source_cursor_overflow".to_owned())
                })?)
            } else {
                None
            };
        Ok(Self {
            source_epoch,
            after_cursor,
            first_cursor,
            page,
        })
    }

    /// 获取本页最后一个事件的 cursor（用于下一页的 after_cursor）。
    ///
    /// 【用途】
    /// 投影器处理完本页后，调这个方法来获取「截止 cursor」，
    /// 然后存储起来（如存在数据库），作为下次启动的起点。
    ///
    /// 【示例】
    /// ```ignore
    /// loop {
    ///     let page = read_billing_page(store, epoch, last_cursor, limit).await?;
    ///     process_events(&page.events());
    ///     last_cursor = page.cursor();  // 下次的 after_cursor
    ///     store_checkpoint(epoch, last_cursor)?;
    /// }
    /// ```
    pub fn cursor(&self) -> EventCursor {
        self.page.cursor
    }

    /// 获取本页的事件列表。
    ///
    /// 【用途】
    /// 投影器迭代 events()，对每个事件进行计费/审计/其它投影逻辑。
    ///
    /// 【性质】
    ///   * 顺序：按追加顺序，单调递增的 cursor；
    ///   * 连续性：不会有「事件 1 的 cursor=10，事件 2 的 cursor=12」这种间隙；
    ///   * 原子性：本页面由 EventStorePort 原子返回，页面中的事件一定是「同一个转换」的结果。
    pub fn events(&self) -> &[RuntimeEvent] {
        &self.page.events
    }
}

/// 从事件存储读取一页账单数据。
///
/// 【参数】
///   * store: EventStorePort 实现（如 MemoryEventLog / JsonlEventLog / SQLiteEventLog）；
///   * source_epoch: 投影器的逻辑纪元；
///   * after_cursor: 从哪个 cursor 开始读（通常是上一页的 page.cursor）；
///   * limit: 本页最多读几个事件。
///
/// 【返回值】
///   * Ok(BillingSourcePage) -> 读取成功，页面已校验，可放心消费；
///   * Err(PortError) -> 存储层错误，或校验失败。
///
/// 【流程】
///   ①调 store.read_from(after_cursor, limit)；
///   ②调 BillingSourcePage::new(source_epoch, after_cursor, page) 进行校验；
///   ③返回包装后的页面。
///
/// 【用途】
/// 账单投影器、审计系统、任何需要「逐页扫事件」的系统都用这个函数。
/// 是 EventStorePort -> 投影器 之间的标准适配层。
pub async fn read_billing_page(
    store: &dyn EventStorePort,
    source_epoch: u64,
    after_cursor: EventCursor,
    limit: usize,
) -> Result<BillingSourcePage, PortError> {
    let page = store.read_from(after_cursor, limit).await?;
    BillingSourcePage::new(source_epoch, after_cursor, page)
}

/// 别名：针对用「账本（ledger）」术语的系统。
///
/// 【用途】
/// 有些地方喜欢说「从 ledger 读一页」而不是「从 billing source 读一页」。
/// 本函数是同义词，完全等价于 read_billing_page()。
/// 如果你的代码用了 ledger terminology，import 这个函数会让意图更清晰。
pub async fn read_ledger_page(
    store: &dyn EventStorePort,
    source_epoch: u64,
    after_cursor: EventCursor,
    limit: usize,
) -> Result<BillingSourcePage, PortError> {
    read_billing_page(store, source_epoch, after_cursor, limit).await
}

/// 常量：声明本模块只做读取，不写任何新事实到账本。
///
/// 【语义】
/// BILLING_SOURCE_IS_READ_ONLY = true，意思是：
///   * read_billing_page() 不会修改 EventStorePort 的状态；
///   * 不会创建新的账本、不会追加补偿事实、不会删除历史；
///   * 投影器必须**自己**存储投影结果，本模块不负责持久化。
///
/// 【为什么要这个常量】
/// 这是一个「面向编译器和审查者的承诺」：任何试图依赖「本模块会写账本」的代码
/// 都会因为这个 false 而一目了然地看到自己的假设是错的。
/// 有些静态分析工具会检查这个常量，防止意外的状态修改。
pub const BILLING_SOURCE_IS_READ_ONLY: bool = true;
