//! BQ-23 read-only billing query wire contracts.
//!
//! Requests are bound to a server-owned project boundary and a BQ-20 projection cursor.  The
//! response carries freshness, Unknown/quarantine counts and a projection digest so CLI, Web and
//! Workbench can render the same snapshot.  These values do not reserve budget, consume approval
//! or mutate the ledger.

use crate::{json_digest, BillingProjectionSnapshot, BillingRollupTotals, Freshness, ProjectId};
use serde::{Deserialize, Serialize};

pub const BILLING_QUERY_SCHEMA: &str = "kiana.billing-query.v1";
pub const BILLING_QUERY_CURSOR_SCHEMA: &str = "kiana.billing-query-cursor.v1";
pub const BILLING_QUERY_RESPONSE_SCHEMA: &str = "kiana.billing-query-response.v1";
pub const BILLING_QUERY_MAX_LIMIT: u16 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingQueryKind {
    BudgetSummary,
    UsageBreakdown,
    CostExport,
    ReconciliationInbox,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingQueryCursor {
    pub schema: String,
    pub source_cursor: u64,
    pub after_cursor: u64,
    pub boundary_digest: String,
    pub projection_digest: String,
    pub cursor_digest: String,
}

impl BillingQueryCursor {
    pub fn new(
        source_cursor: u64,
        after_cursor: u64,
        boundary_digest: impl Into<String>,
        projection_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut cursor = Self {
            schema: BILLING_QUERY_CURSOR_SCHEMA.to_owned(),
            source_cursor,
            after_cursor,
            boundary_digest: boundary_digest.into(),
            projection_digest: projection_digest.into(),
            cursor_digest: String::new(),
        };
        cursor.cursor_digest = cursor.digest();
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_QUERY_CURSOR_SCHEMA
            || self.source_cursor == 0
            || self.after_cursor >= self.source_cursor
            || !valid_digest(&self.boundary_digest)
            || !valid_digest(&self.projection_digest)
            || !valid_digest(&self.cursor_digest)
            || self.cursor_digest != self.digest()
        {
            return Err("billing_query_cursor_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_cursor": self.source_cursor,
            "after_cursor": self.after_cursor,
            "boundary_digest": self.boundary_digest,
            "projection_digest": self.projection_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingQueryRequest {
    pub schema: String,
    pub kind: BillingQueryKind,
    pub project_id: ProjectId,
    pub data_boundary_digest: String,
    #[serde(default)]
    pub source_cursor: Option<u64>,
    #[serde(default)]
    pub after_cursor: u64,
    pub limit: u16,
    #[serde(default)]
    pub cursor: Option<BillingQueryCursor>,
    pub read_only: bool,
}

impl BillingQueryRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BILLING_QUERY_SCHEMA
            || self.project_id.as_uuid().is_nil()
            || !valid_digest(&self.data_boundary_digest)
            || self.source_cursor == Some(0)
            || self.limit == 0
            || self.limit > BILLING_QUERY_MAX_LIMIT
            || !self.read_only
            || !self
                .source_cursor
                .is_none_or(|source| self.after_cursor <= source)
            || self.source_cursor.is_none() && self.after_cursor != 0
        {
            return Err("billing_query_request_invalid");
        }
        if let Some(cursor) = &self.cursor {
            cursor
                .validate()
                .map_err(|_| "billing_query_cursor_invalid")?;
            if self.source_cursor != Some(cursor.source_cursor)
                || self.after_cursor != cursor.after_cursor
                || self.data_boundary_digest != cursor.boundary_digest
            {
                return Err("billing_query_cursor_binding_invalid");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingQueryResponse {
    pub schema: String,
    pub kind: BillingQueryKind,
    pub project_id: ProjectId,
    pub data_boundary_digest: String,
    pub source_cursor: u64,
    pub projection_digest: String,
    pub freshness: Freshness,
    pub after_cursor: u64,
    pub summary: BillingRollupTotals,
    pub row_count: u64,
    pub unknown_count: u64,
    pub quarantine_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<BillingQueryCursor>,
    pub response_digest: String,
}

impl BillingQueryResponse {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: BillingQueryKind,
        project_id: ProjectId,
        data_boundary_digest: impl Into<String>,
        source_cursor: u64,
        projection_digest: impl Into<String>,
        freshness: Freshness,
        after_cursor: u64,
        summary: BillingRollupTotals,
        row_count: u64,
        quarantine_count: u64,
        export_digest: Option<String>,
        next_cursor: Option<BillingQueryCursor>,
    ) -> Result<Self, String> {
        let mut response = Self {
            schema: BILLING_QUERY_RESPONSE_SCHEMA.to_owned(),
            kind,
            project_id,
            data_boundary_digest: data_boundary_digest.into(),
            source_cursor,
            projection_digest: projection_digest.into(),
            freshness,
            after_cursor,
            unknown_count: summary.unknown_count,
            quarantine_count,
            summary,
            row_count,
            export_digest,
            next_cursor,
            response_digest: String::new(),
        };
        response.response_digest = response.digest();
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_QUERY_RESPONSE_SCHEMA
            || self.project_id.as_uuid().is_nil()
            || !valid_digest(&self.data_boundary_digest)
            || self.source_cursor == 0
            || !valid_digest(&self.projection_digest)
            || self.after_cursor > self.source_cursor
            || self.unknown_count != self.summary.unknown_count
            || self.summary.validate().is_err()
            || !valid_digest(&self.response_digest)
            || self.response_digest != self.digest()
        {
            return Err("billing_query_response_invalid".to_owned());
        }
        if matches!(self.kind, BillingQueryKind::CostExport) {
            if self
                .export_digest
                .as_deref()
                .is_none_or(|digest| !valid_digest(digest))
            {
                return Err("billing_query_export_digest_required".to_owned());
            }
        } else if self.export_digest.is_some() {
            return Err("billing_query_export_digest_unexpected".to_owned());
        }
        if let Some(cursor) = &self.next_cursor {
            cursor.validate()?;
            if cursor.source_cursor != self.source_cursor
                || cursor.boundary_digest != self.data_boundary_digest
                || cursor.projection_digest != self.projection_digest
                || cursor.after_cursor <= self.after_cursor
            {
                return Err("billing_query_next_cursor_invalid".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "kind": self.kind,
            "project_id": self.project_id,
            "data_boundary_digest": self.data_boundary_digest,
            "source_cursor": self.source_cursor,
            "projection_digest": self.projection_digest,
            "freshness": self.freshness,
            "after_cursor": self.after_cursor,
            "summary": self.summary,
            "row_count": self.row_count,
            "unknown_count": self.unknown_count,
            "quarantine_count": self.quarantine_count,
            "export_digest": self.export_digest,
            "next_cursor": self.next_cursor,
        }))
    }
}

pub fn validate_billing_query_request(request: &BillingQueryRequest) -> Result<(), &'static str> {
    request.validate()
}

pub fn validate_billing_query_response(
    response: &BillingQueryResponse,
) -> Result<(), &'static str> {
    response
        .validate()
        .map_err(|_| "billing_query_response_invalid")
}

pub fn projection_digest(snapshot: &BillingProjectionSnapshot) -> String {
    snapshot.projection_digest.clone()
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
