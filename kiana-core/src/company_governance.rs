//! Read-only CompanyOS cross-entry governance projection.
//!
//! This module never advances a Company aggregate. It only checks that runtime, review,
//! acceptance, delivery and closing references form a complete chain before exposing a
//! comparable snapshot to every entrypoint.

use kiana_domain::{
    AcceptanceStatus, CompanyGovernanceSnapshot, CompanyState, DeliveryStatus, EventId,
    ExecutionStatus, GovernanceStatus, ProjectStatus,
};
use std::collections::BTreeMap;

use super::{ControlPlane, CoreError};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CompanyGovernanceProjectionError {
    #[error("company_governance_project_not_found")]
    ProjectNotFound,
    #[error("company_governance_source_empty")]
    SourceEmpty,
    #[error("company_governance_source_duplicate")]
    SourceDuplicate,
    #[error("company_governance_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

fn status_for_run(status: ExecutionStatus) -> ExecutionStatus {
    match status {
        ExecutionStatus::Accepted
        | ExecutionStatus::Queued
        | ExecutionStatus::Cancelling
        | ExecutionStatus::Denied
        | ExecutionStatus::Blocked => status,
        other => other,
    }
}

/// Project one Company aggregate without changing it or treating runtime completion as Outcome.
pub fn project_company_governance(
    state: &CompanyState,
    project_id: &str,
    source_event_ids: Vec<EventId>,
) -> Result<CompanyGovernanceSnapshot, CompanyGovernanceProjectionError> {
    let project = state
        .projects
        .get(project_id)
        .ok_or(CompanyGovernanceProjectionError::ProjectNotFound)?;
    if source_event_ids.is_empty() {
        return Err(CompanyGovernanceProjectionError::SourceEmpty);
    }
    let mut runtime_statuses = BTreeMap::new();
    for (packet_id, run) in state
        .runs
        .iter()
        .filter(|(_, run)| run.project_id == project_id)
    {
        runtime_statuses.insert(packet_id.clone(), status_for_run(run.status));
    }
    let acceptance = project
        .acceptance_id
        .as_deref()
        .and_then(|id| state.acceptances.get(id));
    let acceptance_status = acceptance.map(|acceptance| acceptance.status);
    let review = acceptance.and_then(|acceptance| {
        state
            .reviews
            .values()
            .find(|review| review.acceptance_id == acceptance.acceptance_id)
    });
    let delivery = acceptance.and_then(|acceptance| {
        state.deliveries.values().find(|delivery| {
            delivery.project_id == project_id && delivery.acceptance_id == acceptance.acceptance_id
        })
    });
    let closing = state
        .closing_receipts
        .values()
        .find(|receipt| receipt.project_id == project_id);
    let outcome_ids = state
        .outcomes
        .values()
        .filter(|outcome| outcome.project_id == project_id)
        .map(|outcome| outcome.outcome_id.clone())
        .collect::<Vec<_>>();

    let mut limitations = Vec::new();
    if runtime_statuses.is_empty() {
        limitations.push("runtime_evidence_missing".to_owned());
    }
    if runtime_statuses
        .values()
        .any(|status| *status == ExecutionStatus::ResultUnknown)
    {
        limitations.push("runtime_result_unknown_requires_reconcile".to_owned());
    }
    if runtime_statuses
        .values()
        .any(|status| *status == ExecutionStatus::Completed)
        && acceptance.is_none()
    {
        limitations.push("runtime_completed_not_business_outcome".to_owned());
    }

    let mut status = if project.status == ProjectStatus::Closed || closing.is_some() {
        GovernanceStatus::Closed
    } else if matches!(
        acceptance_status,
        Some(AcceptanceStatus::Accepted | AcceptanceStatus::Waived)
    ) {
        GovernanceStatus::Accepted
    } else {
        GovernanceStatus::InProgress
    };

    if status == GovernanceStatus::Closed {
        let chain_complete = project.status == ProjectStatus::Closed
            && matches!(
                acceptance_status,
                Some(AcceptanceStatus::Accepted | AcceptanceStatus::Waived)
            )
            && review.is_some()
            && delivery.is_some_and(|delivery| delivery.status == DeliveryStatus::Confirmed)
            && closing.is_some()
            && runtime_statuses
                .values()
                .all(|run| *run == ExecutionStatus::Completed);
        if !chain_complete {
            status = GovernanceStatus::Unknown;
            limitations.push("closing_chain_incomplete".to_owned());
        }
    }
    if let (Some(acceptance), Some(review)) = (acceptance, review) {
        if acceptance.author_session_id == review.reviewer_session_id {
            status = GovernanceStatus::Unknown;
            limitations.push("review_author_session_overlap".to_owned());
        }
    }
    if let Some(closing) = closing {
        if closing.author_session_id == closing.reviewer_session_id
            || closing.author_session_id == closing.closer_session_id
            || closing.reviewer_session_id == closing.closer_session_id
        {
            status = GovernanceStatus::Unknown;
            limitations.push("closing_role_separation_failed".to_owned());
        }
    }
    if status == GovernanceStatus::Unknown && limitations.is_empty() {
        limitations.push("company_governance_unknown".to_owned());
    }
    CompanyGovernanceSnapshot::new(
        project_id,
        project.status,
        runtime_statuses,
        acceptance_status,
        review.map(|review| review.review_id.clone()),
        delivery.map(|delivery| delivery.status),
        closing.map(|closing| closing.receipt_id.clone()),
        outcome_ids,
        source_event_ids,
        status,
        limitations,
    )
    .map_err(CompanyGovernanceProjectionError::SnapshotInvalid)
}

impl ControlPlane {
    /// Read a project governance chain from the authenticated Company aggregate and EventLog.
    pub async fn company_governance(
        &self,
        context: &kiana_domain::RequestContext,
        project_id: &str,
    ) -> Result<kiana_domain::CoreResponse, CoreError> {
        if let Err(reason) = super::company::company_context_for_read(context) {
            return Ok(kiana_domain::CoreResponse::blocked(
                context.request_id,
                reason,
            ));
        }
        let (state, _) = self.load_company(context).await?;
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable(
                "company_governance_read_all_unsupported".to_owned(),
            )
        })?;
        let source_event_ids = events
            .iter()
            .filter(|event| super::company::company_event_owned(event, context, &events))
            .map(|event| event.event_id)
            .collect::<Vec<_>>();
        if source_event_ids.len() > kiana_domain::MAX_GOVERNANCE_REFS {
            return Err(kiana_ports::PortError::Failed(
                "company_governance_source_limit".to_owned(),
            )
            .into());
        }
        let snapshot = project_company_governance(&state, project_id, source_event_ids)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        Ok(kiana_domain::CoreResponse::completed(
            context.request_id,
            serde_json::to_value(snapshot).map_err(|error| {
                kiana_ports::PortError::Failed(format!("company_governance_serialize:{error}"))
            })?,
        ))
    }
}
