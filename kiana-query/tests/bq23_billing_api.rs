use kiana_domain::{
    BillingProjectionCursor, BillingProjectionSnapshot, BillingQueryKind, BillingQueryRequest,
    ProjectId, BILLING_PROJECTION_NUMBER,
};
use kiana_query::{
    query_billing_projection, BillingQueryError, QueryDataBoundary, QueryDataDisposition,
};

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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
