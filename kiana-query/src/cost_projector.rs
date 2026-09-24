//! Read-only receipt cost query projection.
//!
//! The query crate consumes committed RuntimeEvent values supplied by its caller and delegates
//! cost state transitions to the domain reducer.  It never reads provider prices, releases quota,
//! writes EventLog facts or promotes an estimate into a financial authority.

use kiana_domain::{
    project_receipt_cost_breakdown, ReceiptCostBreakdown, RunId, RuntimeEvent, SchemaVersion,
};

pub const COST_QUERY_PROJECTION_SCHEMA: &str = "kiana.cost-query-projection.v1";
pub const COST_QUERY_PROJECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CostQueryProjectionError {
    #[error("cost_query_run_invalid")]
    RunInvalid,
    #[error("cost_query_source_empty")]
    SourceEmpty,
    #[error("cost_query_projection_invalid:{0}")]
    Invalid(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CostReceiptProjection {
    pub schema: &'static str,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub source_cursor: u64,
    pub source_event_ids: Vec<kiana_domain::EventId>,
    pub breakdown: ReceiptCostBreakdown,
}

impl CostReceiptProjection {
    pub fn validate(&self) -> Result<(), CostQueryProjectionError> {
        if self.schema != COST_QUERY_PROJECTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&COST_QUERY_PROJECTION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.breakdown.run_id != self.run_id
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() != self.breakdown.source_event_ids.len()
            || self.source_event_ids != self.breakdown.source_event_ids
        {
            return Err(CostQueryProjectionError::Invalid(
                "cost_query_projection_header_invalid".to_owned(),
            ));
        }
        self.breakdown
            .validate()
            .map_err(CostQueryProjectionError::Invalid)
    }

    pub fn unknown_reasons(&self) -> &[kiana_domain::BillingUnknownReason] {
        &self.breakdown.unknown_reasons
    }
}

/// Rebuild a run cost view from committed BQ-13 events.  Empty input is distinguishable from an
/// unknown cost: a caller that has no cost facts should show an unavailable projection rather than
/// a fabricated zero amount.
pub fn project_cost_receipt(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostReceiptProjection, CostQueryProjectionError> {
    if run_id.as_uuid().is_nil() {
        return Err(CostQueryProjectionError::RunInvalid);
    }
    let breakdown = project_receipt_cost_breakdown(run_id, events)
        .map_err(CostQueryProjectionError::Invalid)?
        .ok_or(CostQueryProjectionError::SourceEmpty)?;
    let result = CostReceiptProjection {
        schema: COST_QUERY_PROJECTION_SCHEMA,
        version: COST_QUERY_PROJECTION_VERSION,
        run_id,
        source_cursor: breakdown.source_cursor,
        source_event_ids: breakdown.source_event_ids.clone(),
        breakdown,
    };
    result.validate()?;
    Ok(result)
}

/// Singular/read-model naming used by query adapters.
pub fn project_receipt_cost(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostReceiptProjection, CostQueryProjectionError> {
    project_cost_receipt(run_id, events)
}
