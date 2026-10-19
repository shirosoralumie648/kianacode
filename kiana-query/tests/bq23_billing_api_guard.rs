//! BQ-23 billing API 的静态源码 marker/禁止词守卫。
//!
//! `include_str!` 检查 Query adapter 保留四类请求、cursor 错误、缺失来源游标的拒绝和 boundary
//! 字段，并限制单文件中不出现若干写入或授权扩展标记；它不运行实际 API 请求，也不证明上层路由
//! 已复用此函数。

#[test]
fn billing_query_is_read_only_and_does_not_widen_authority() {
    let source = include_str!("../src/billing_api.rs");
    // marker 表示源码仍保留四类查询和边界拒绝路径的名称。
    for marker in [
        "query_billing_projection",
        "BillingQueryKind::BudgetSummary",
        "BillingQueryKind::UsageBreakdown",
        "BillingQueryKind::CostExport",
        "BillingQueryKind::ReconciliationInbox",
        "CursorProjectionMismatch",
        "CursorOutOfRange",
        "billing_query_cursor_out_of_range",
        "QueryDataDisposition::Allowed",
    ] {
        assert!(source.contains(marker), "BQ-23 marker missing: {marker}");
    }
    // 缺失来源游标不能被渲染成空账单：拒绝类型、稳定码和调用点都要保留。
    for marker in [
        "ProjectionSourceMissing",
        "billing_query_projection_source_cursor_missing",
        "BillingQueryError::ProjectionSourceMissing",
    ] {
        assert!(
            source.contains(marker),
            "BQ-23 missing-source refusal marker missing: {marker}"
        );
    }
    // 负向 marker 仅搜索 billing_api.rs，不是整个 crate 的调用图保证。
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "ApprovalStore",
        "append(",
        "reserve(",
        "consume_approval",
        "release_reservation",
    ] {
        assert!(
            !source.contains(forbidden),
            "BQ-23 query widened: {forbidden}"
        );
    }
}
