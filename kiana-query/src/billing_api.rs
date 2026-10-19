//! 本文件为 CLI、Web、Workbench 等上层入口提供同一套只读账单摘要响应转换。
//!
//! 在系统中的位置：调用方把已构建并校验的 BQ-20 `BillingProjectionSnapshot`、BQ-23 请求和
//! `QueryDataBoundary` 交给 `query_billing_projection()`；本 adapter 检查三者合同与游标，再选取
//! usage 或 ledger 汇总并构造统一响应。当前仓内直接调用只见 BQ-23 fixture，未找到生产路由 caller。
//!
//! ```text
//! BillingQueryRequest + BillingProjectionSnapshot + QueryDataBoundary
//!   -> request / snapshot / boundary 依次验证
//!   -> boundary digest 与 index/export disposition 检查
//!   -> after/source/projection cursor 检查
//!   -> 选择 usage 或 ledger 汇总、freshness、row/quarantine 计数
//!   -> Domain BillingQueryResponse::new() 计算响应摘要并复验
//! ```
//!
//! 此模块只返回汇总 DTO，不读取账本、不筛选/分页明细行、不重新计算 projection，也不产生外部副作用。
//! `project_id` 会放入响应，但当前 snapshot DTO 没有 project ID，adapter 不会据此过滤或证明 snapshot
//! 已按项目隔离；调用方必须先提供与请求范围对应的可信 snapshot。边界 digest 是普通自摘要/绑定值，
//! 并非签名；真正入口必须确保 boundary 来源可信。

use crate::{QueryDataBoundary, QueryDataDisposition};
use kiana_domain::{
    BillingProjectionSnapshot, BillingQueryCursor, BillingQueryKind, BillingQueryRequest,
    BillingQueryResponse, Freshness,
};

pub const BILLING_QUERY_API_SCHEMA: &str = "kiana.billing-query-api.v1";

/// 账单 Query adapter 对请求拒绝、投影无效、边界拒绝和 cursor 冲突的错误分类。
///
/// 上层入口可将这些类型映射成协议错误；本函数不会自动重试，也不会把拒绝转换成空账单。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BillingQueryError {
    /// Domain 请求合同不成立，例如 limit 越界、project ID 无效或 read_only 不是 true。
    #[error("billing_query_request_invalid:{0}")]
    RequestInvalid(&'static str),
    /// Billing snapshot 或最终 response DTO 校验失败。
    #[error("billing_query_projection_invalid:{0}")]
    ProjectionInvalid(String),
    /// 边界 DTO 自身未通过合同验证。
    #[error("billing_query_boundary_invalid:{0}")]
    BoundaryInvalid(String),
    /// 请求的 scope digest 不匹配，或对应的 index/export disposition 被拒绝。
    #[error("billing_query_boundary_denied")]
    BoundaryDenied,
    /// 分页 cursor 声称属于另一份 projection digest。
    #[error("billing_query_cursor_projection_mismatch")]
    CursorProjectionMismatch,
    /// 请求的 after_cursor 超出当前 snapshot 的 source cursor。
    #[error("billing_query_cursor_out_of_range")]
    CursorOutOfRange,
    /// 投影没有可用的来源游标（cursor 0 / 未完成重建）。此时不能渲染成“空账单成功”，
    /// 因为 `BillingQueryResponse` 不允许 source_cursor 0，也不存在可绑定的 freshness。
    #[error("billing_query_projection_source_cursor_missing")]
    ProjectionSourceMissing,
}

/// 从已准备好的 billing projection 和数据边界构造统一的只读账单响应。
///
/// 【调用者】设计上供 CLI/Web/Workbench adapter 共用；当前仓内只有 BQ-23 fixture 直接调用，
/// `kiana-query/src/lib.rs` 公开 re-export，但未找到生产入口使用。
/// 【输入】`request` 声明 summary 类型、project ID、scope digest、source/after cursor 与 limit；
/// `snapshot` 提供已校验的 usage/ledger 汇总、quarantine 和 projection digest；`boundary` 提供 scope
/// 和 Index/Export disposition。上游负责取得可信 boundary 和正确范围的 snapshot。
/// 【输出】`BillingQueryResponse` 携带 kind、project ID、boundary/source/projection digest、freshness、
/// 选定汇总、行数/Unknown/quarantine 计数、可选 export digest 与 next cursor。
/// 【核心流程】依次校验 request、snapshot、boundary；检查 boundary digest 与读权限；验证 after cursor
/// 及 cursor projection digest；按 kind 选 summary/row count，按 source cursor 计算 freshness/next cursor；
/// 最后交给 Domain response constructor 计算并验证 response digest。
/// 【失败与影响】请求/快照/boundary 无效、scope 不匹配、Index/Export 被拒、after cursor 超界、
/// cursor digest 不属于当前 projection，或 Domain cursor/response 构造失败时返回具体错误；无重试，
/// 无写入，无 reservation/approval 状态变化。
/// 【容易误改】`source_cursor` 表示 projection 数据进度；`after_cursor + limit` 用来构造下一页 fence，
/// 但本响应只有 rollup summary，不在这里切片或返回账单明细行。`request.project_id` 被回显，却没有
/// 与 snapshot 内项目 ID 比较，因为 snapshot 没有该字段；不能把此 adapter 当成项目级过滤器。
pub fn query_billing_projection(
    request: &BillingQueryRequest,
    snapshot: &BillingProjectionSnapshot,
    boundary: &QueryDataBoundary,
) -> Result<BillingQueryResponse, BillingQueryError> {
    // 先验证调用方请求，再验证要消费的快照和边界；后续字段映射以这些合同已通过为前提。
    request
        .validate()
        .map_err(BillingQueryError::RequestInvalid)?;
    snapshot
        .validate()
        .map_err(BillingQueryError::ProjectionInvalid)?;
    boundary
        .validate()
        .map_err(BillingQueryError::BoundaryInvalid)?;
    // 请求把同一个 scope digest 带来后，还需同时满足 index 读取许可；CostExport 额外检查 export 开关。
    if request.data_boundary_digest != boundary.scope_digest
        || boundary.index != QueryDataDisposition::Allowed
        || (request.kind == BillingQueryKind::CostExport
            && boundary.export != QueryDataDisposition::Allowed)
    {
        return Err(BillingQueryError::BoundaryDenied);
    }

    let source_cursor = snapshot.source.source_cursor;
    // 没有来源游标的投影不能渲染成“零行成功”：Domain response 拒绝 source_cursor 0，
    // 调用方也无法区分“账本没有事实”和“投影从未推进/读取失败”。必须显式拒绝。
    if source_cursor == 0 || snapshot.source.source_epoch == 0 {
        return Err(BillingQueryError::ProjectionSourceMissing);
    }
    // after_cursor 指向的进度不能超过当前快照；等于末尾是合法的空后续页。
    if request.after_cursor > source_cursor {
        return Err(BillingQueryError::CursorOutOfRange);
    }
    // Domain request.validate() 已绑定 cursor 与 request 的 boundary/source/after 字段；此处再将
    // cursor 所属 projection 与本次 snapshot 对齐，避免拿旧视图的游标继续翻新视图。
    if let Some(cursor) = &request.cursor {
        if cursor.projection_digest != snapshot.projection_digest {
            return Err(BillingQueryError::CursorProjectionMismatch);
        }
    }

    // 请求未给 source cursor 时 freshness 保持 Unknown；不能默认宣称读到最新版本。
    let freshness = match request.source_cursor {
        Some(requested) if source_cursor >= requested => Freshness::Current,
        Some(_) => Freshness::Stale,
        None => Freshness::Unknown,
    };
    // 四种 kind 只选择预先汇总好的 rollup，不在这里展开或过滤原始明细。
    let summary = match request.kind {
        BillingQueryKind::BudgetSummary | BillingQueryKind::CostExport => snapshot.ledger.clone(),
        BillingQueryKind::UsageBreakdown | BillingQueryKind::ReconciliationInbox => {
            snapshot.usage.clone()
        }
    };
    // row_count 是该 view 的逻辑记录数：BudgetSummary 固定一行，reconciliation 把 unknown usage 与 quarantine 合计。
    let row_count = match request.kind {
        BillingQueryKind::BudgetSummary => 1,
        BillingQueryKind::UsageBreakdown => snapshot.usage.usage_count,
        BillingQueryKind::CostExport => snapshot.ledger.ledger_entry_count,
        BillingQueryKind::ReconciliationInbox => {
            snapshot.usage.unknown_count + snapshot.quarantine.len() as u64
        }
    };
    // checked_add 防止 after+limit 溢出；只有仍有后续 source 区间时才签发绑定当前 boundary/projection 的 cursor。
    let next_cursor = request
        .after_cursor
        .checked_add(u64::from(request.limit))
        .filter(|next| *next < source_cursor)
        .map(|after_cursor| {
            BillingQueryCursor::new(
                source_cursor,
                after_cursor,
                request.data_boundary_digest.clone(),
                snapshot.projection_digest.clone(),
            )
        })
        .transpose()
        .map_err(BillingQueryError::ProjectionInvalid)?;
    BillingQueryResponse::new(
        request.kind,
        request.project_id,
        request.data_boundary_digest.clone(),
        source_cursor,
        snapshot.projection_digest.clone(),
        freshness,
        request.after_cursor,
        summary,
        row_count,
        snapshot.quarantine.len() as u64,
        (request.kind == BillingQueryKind::CostExport).then(|| snapshot.projection_digest.clone()),
        next_cursor,
    )
    .map_err(BillingQueryError::ProjectionInvalid)
}
