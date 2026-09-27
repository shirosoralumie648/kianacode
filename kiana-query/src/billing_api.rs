//! BQ-23 read-only billing query adapter.
//!
//! This adapter folds no new facts. It validates the caller's data boundary and page cursor, then
//! derives one of four bounded summaries from the committed BQ-20 projection. Reservation,
//! approval and EventLog writer interfaces are deliberately absent from this module.

use crate::{QueryDataBoundary, QueryDataDisposition};
use kiana_domain::{
    BillingProjectionSnapshot, BillingQueryCursor, BillingQueryKind, BillingQueryRequest,
    BillingQueryResponse, Freshness,
};

pub const BILLING_QUERY_API_SCHEMA: &str = "kiana.billing-query-api.v1";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BillingQueryError {
    #[error("billing_query_request_invalid:{0}")]
    RequestInvalid(&'static str),
    #[error("billing_query_projection_invalid:{0}")]
    ProjectionInvalid(String),
    #[error("billing_query_boundary_invalid:{0}")]
    BoundaryInvalid(String),
    #[error("billing_query_boundary_denied")]
    BoundaryDenied,
    #[error("billing_query_cursor_projection_mismatch")]
    CursorProjectionMismatch,
    #[error("billing_query_cursor_out_of_range")]
    CursorOutOfRange,
}

/// Build the same read-only billing response for CLI, Web and Workbench callers.
pub fn query_billing_projection(
    request: &BillingQueryRequest,
    snapshot: &BillingProjectionSnapshot,
    boundary: &QueryDataBoundary,
) -> Result<BillingQueryResponse, BillingQueryError> {
    request
        .validate()
        .map_err(BillingQueryError::RequestInvalid)?;
    snapshot
        .validate()
        .map_err(BillingQueryError::ProjectionInvalid)?;
    boundary
        .validate()
        .map_err(BillingQueryError::BoundaryInvalid)?;
    if request.data_boundary_digest != boundary.scope_digest
        || boundary.index != QueryDataDisposition::Allowed
        || (request.kind == BillingQueryKind::CostExport
            && boundary.export != QueryDataDisposition::Allowed)
    {
        return Err(BillingQueryError::BoundaryDenied);
    }

    let source_cursor = snapshot.source.source_cursor;
    if request.after_cursor > source_cursor {
        return Err(BillingQueryError::CursorOutOfRange);
    }
    if let Some(cursor) = &request.cursor {
        if cursor.projection_digest != snapshot.projection_digest {
            return Err(BillingQueryError::CursorProjectionMismatch);
        }
    }

    let freshness = match request.source_cursor {
        Some(requested) if source_cursor >= requested => Freshness::Current,
        Some(_) => Freshness::Stale,
        None => Freshness::Unknown,
    };
    let summary = match request.kind {
        BillingQueryKind::BudgetSummary | BillingQueryKind::CostExport => snapshot.ledger.clone(),
        BillingQueryKind::UsageBreakdown | BillingQueryKind::ReconciliationInbox => {
            snapshot.usage.clone()
        }
    };
    let row_count = match request.kind {
        BillingQueryKind::BudgetSummary => 1,
        BillingQueryKind::UsageBreakdown => snapshot.usage.usage_count,
        BillingQueryKind::CostExport => snapshot.ledger.ledger_entry_count,
        BillingQueryKind::ReconciliationInbox => {
            snapshot.usage.unknown_count + snapshot.quarantine.len() as u64
        }
    };
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
