//! Core adapter for scope-bound Company read models.
//!
//! The source Company aggregate and EventLog remain authoritative; this adapter only delegates
//! deterministic projection and never mutates state or advances a project from client text.

use kiana_domain::{project_read_model, CompanyReadModelSnapshot, CompanyState};
use std::collections::BTreeSet;

pub(crate) fn project_company_read_model(
    state: &CompanyState,
    project_id: &str,
    authorized_project_ids: &BTreeSet<String>,
    source_cursor: u64,
    projection_cursor: Option<u64>,
    revision: u64,
    authority_epoch: u64,
) -> Result<CompanyReadModelSnapshot, &'static str> {
    project_read_model(
        state,
        project_id,
        authorized_project_ids,
        source_cursor,
        projection_cursor,
        revision,
        authority_epoch,
    )
}
