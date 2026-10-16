//! BQ-23 只读账单 Query adapter 的合成 DTO fixture。
//!
//! 快照、请求和 boundary 都由测试本地构造，检查一个允许读取的 summary、显式导出拒绝及旧 projection
//! cursor 拒绝；不读取账本 EventLog，不经过 CLI/Web/Workbench 路由，也不证明真实项目隔离。

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

/// 响应沿用快照 source fence，并按请求 cursor 推导 freshness。
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

/// CostExport 需要单独允许；分页 cursor 绑定另一个 projection 时必须拒绝。
#[test]
fn export_requires_explicit_boundary_and_stale_cursor_is_rejected() {
    let snapshot = snapshot();
    let denied_boundary = QueryDataBoundary::derive("/repo", D1, 4, false, true, false).unwrap();
    assert_eq!(
        query_billing_projection(
            &request(BillingQueryKind::CostExport, Some(20)),
            &snapshot,
            &denied_boundary,
        )
        .unwrap_err(),
        BillingQueryError::BoundaryDenied
    );

    let boundary = QueryDataBoundary::derive("/repo", D1, 4, false, true, true).unwrap();
    let mut request = request(BillingQueryKind::UsageBreakdown, Some(20));
    request.cursor = Some(kiana_domain::BillingQueryCursor::new(20, 2, D1, D2).unwrap());
    request.after_cursor = 2;
    assert_eq!(
        query_billing_projection(&request, &snapshot, &boundary).unwrap_err(),
        BillingQueryError::CursorProjectionMismatch
    );
    assert_eq!(boundary.index, QueryDataDisposition::Allowed);
}
