//! BQ-23 只读账单 Query adapter 的合成 DTO fixture（先拒绝，后成功）。
//!
//! 快照、请求和 boundary 都由测试本地构造。拒绝用例排在成功用例之前：
//! 1. 没有来源游标的投影必须显式拒绝，而不是渲染成“零行成功”；
//! 2. CostExport 未获导出许可时必须拒绝；
//! 3. 分页 cursor 绑定另一份 projection 时必须拒绝；
//! 4. 分页越过当前快照进度时必须拒绝（不能渲染成空页）；
//! 5. 估算与实测必须是两个独立总额，查询 summary 不会把估算标记成实测。
//! 之后才检查允许读取的 summary 保留同一 source fence 与 freshness。
//! 这些 fixture 不读取账本 EventLog，不经过 CLI/Web/Workbench 路由，也不证明真实项目隔离。

use kiana_domain::{
    BillingProjectionCursor, BillingProjectionSnapshot, BillingQueryKind, BillingQueryRequest,
    ProjectId, BILLING_PROJECTION_NUMBER,
};
use kiana_query::{
    query_billing_projection, BillingQueryError, QueryDataBoundary, QueryDataDisposition,
};

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// 创建带固定 source cursor 和 projection digest 的空汇总快照。
///
/// Rollup 全部为默认零值仅用于检查响应封装；它不是“没有账单事件”的 EventLog 投影证明。
fn snapshot() -> BillingProjectionSnapshot {
    let source =
        BillingProjectionCursor::new(9, 20, BILLING_PROJECTION_NUMBER, D1, Vec::new()).unwrap();
    BillingProjectionSnapshot::new(
        source,
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Vec::new(),
    )
    .unwrap()
}

/// 创建绑定到同一测试 scope digest 的只读请求。
fn request(kind: BillingQueryKind, source_cursor: Option<u64>) -> BillingQueryRequest {
    BillingQueryRequest {
        schema: kiana_domain::BILLING_QUERY_SCHEMA.to_owned(),
        kind,
        project_id: ProjectId::new(),
        data_boundary_digest: D1.to_owned(),
        source_cursor,
        after_cursor: 0,
        limit: 10,
        cursor: None,
        read_only: true,
    }
}

/// 拒绝优先：cursor 0 的投影不是“空账单”，必须返回稳定拒绝码。
#[test]
fn projection_without_source_cursor_is_refused_not_rendered_empty() {
    let boundary = QueryDataBoundary::derive("/repo", D1, 4, false, true, true).unwrap();
    // `initial(epoch)` is the only cursor shape with source_cursor 0 that still validates.
    let source = BillingProjectionCursor::initial(9).unwrap();
    let empty = BillingProjectionSnapshot::new(
        source,
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        query_billing_projection(
            &request(BillingQueryKind::BudgetSummary, Some(1)),
            &empty,
            &boundary,
        )
        .unwrap_err(),
        BillingQueryError::ProjectionSourceMissing
    );
    assert_eq!(
        BillingQueryError::ProjectionSourceMissing.to_string(),
        "billing_query_projection_source_cursor_missing"
    );
    // 同一请求换成有来源游标的快照后才会成功，证明拒绝来自缺失的 source cursor。
    assert!(query_billing_projection(
        &request(BillingQueryKind::BudgetSummary, Some(20)),
        &snapshot(),
        &boundary,
    )
    .is_ok());
}

/// CostExport 需要单独允许；分页 cursor 绑定另一个 projection 时必须拒绝。
#[test]
fn export_requires_explicit_boundary_and_stale_cursor_is_rejected() {
    let snapshot = snapshot();
    let denied_boundary = QueryDataBoundary::derive("/repo", D1, 4, false, true, false).unwrap();
    let denied = query_billing_projection(
        &request(BillingQueryKind::CostExport, Some(20)),
        &snapshot,
        &denied_boundary,
    )
    .unwrap_err();
    assert_eq!(denied, BillingQueryError::BoundaryDenied);
    assert_eq!(denied.to_string(), "billing_query_boundary_denied");

    let boundary = QueryDataBoundary::derive("/repo", D1, 4, false, true, true).unwrap();
    let mut request = request(BillingQueryKind::UsageBreakdown, Some(20));
    request.cursor = Some(kiana_domain::BillingQueryCursor::new(20, 2, D1, D2).unwrap());
    request.after_cursor = 2;
    let stale = query_billing_projection(&request, &snapshot, &boundary).unwrap_err();
    assert_eq!(stale, BillingQueryError::CursorProjectionMismatch);
    assert_eq!(
        stale.to_string(),
        "billing_query_cursor_projection_mismatch"
    );
    assert_eq!(boundary.index, QueryDataDisposition::Allowed);
}

/// 拒绝优先：分页不能跳过当前 source cursor，且估算金额永远不会被当成实测金额返回。
#[test]
fn pagination_cannot_skip_the_cursor_and_estimate_stays_estimated() {
    let boundary = QueryDataBoundary::derive("/repo", D1, 4, false, true, true).unwrap();

    // 请求游标合法（after <= 声明的 source），但它已越过快照自身进度：不能返回空页假装
    // 翻页成功，必须显式报告 out-of-range。
    let behind_source =
        BillingProjectionCursor::new(9, 3, BILLING_PROJECTION_NUMBER, D1, Vec::new()).unwrap();
    let behind = BillingProjectionSnapshot::new(
        behind_source,
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Vec::new(),
    )
    .unwrap();
    let mut skip = request(BillingQueryKind::UsageBreakdown, Some(5));
    skip.after_cursor = 5;
    let skipped = query_billing_projection(&skip, &behind, &boundary).unwrap_err();
    assert_eq!(skipped, BillingQueryError::CursorOutOfRange);
    assert_eq!(skipped.to_string(), "billing_query_cursor_out_of_range");

    // 估算与实测保持两个独立总额；查看 summary 不会把估算搬进 measured。
    let mut totals = kiana_domain::BillingRollupTotals::default();
    totals
        .add_estimated(&kiana_domain::Money::new("USD", 700).unwrap())
        .unwrap();
    totals
        .add_measured(&kiana_domain::Money::new("USD", 900).unwrap())
        .unwrap();
    totals.unknown_count = 2;
    totals.usage_count = 4;
    let source =
        BillingProjectionCursor::new(9, 20, BILLING_PROJECTION_NUMBER, D1, Vec::new()).unwrap();
    let mixed = BillingProjectionSnapshot::new(
        source,
        totals,
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Vec::new(),
    )
    .unwrap();
    let response = query_billing_projection(
        &request(BillingQueryKind::UsageBreakdown, Some(20)),
        &mixed,
        &boundary,
    )
    .unwrap();
    assert_eq!(
        response
            .summary
            .estimated
            .as_ref()
            .map(|money| money.micros),
        Some(700)
    );
    assert_eq!(
        response.summary.measured.as_ref().map(|money| money.micros),
        Some(900)
    );
    assert_eq!(response.unknown_count, 2);
    assert_eq!(response.row_count, 4);
}

/// 成功路径在全部拒绝之后：响应沿用快照 source fence，并按请求 cursor 推导 freshness。
#[test]
fn query_returns_same_projection_fence_and_freshness_for_read_views() {
    let boundary = QueryDataBoundary::derive("/repo", D1, 4, false, true, true).unwrap();
    let response = query_billing_projection(
        &request(BillingQueryKind::BudgetSummary, Some(20)),
        &snapshot(),
        &boundary,
    )
    .unwrap();
    assert_eq!(response.source_cursor, 20);
    assert_eq!(response.freshness, kiana_domain::Freshness::Current);
    assert_eq!(response.quarantine_count, 0);
}
