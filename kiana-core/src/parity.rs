//! Shared projection envelope for CLI/Web/Workbench/Desktop parity.
//!
//! Entry points receive this snapshot from the ControlPlane. They do not read EventLog files,
//! derive health independently, or trigger reconciliation actions.

use kiana_domain::{
    json_digest, EntryPointKind, EntryPointParitySnapshot, EventId, EventStoreCapabilities,
    ExecutionStatus, HealthProbeKind, RequestContext, RunId, RuntimeEvent,
};
use serde_json::json;
use std::collections::HashSet;

use super::{
    project_health_snapshot, project_run_state, rebuild_audit_projection, ControlPlane, CoreError,
    RunOutcome,
};
use crate::receipts::{filter_run_events, receipt_owner_mismatch};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ParityProjectionError {
    #[error("entrypoint_parity_source_empty")]
    SourceEmpty,
    #[error("entrypoint_parity_source_duplicate")]
    SourceDuplicate,
    #[error("entrypoint_parity_projection_failed:{0}")]
    ProjectionFailed(String),
    #[error("entrypoint_parity_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

fn source_ids(events: &[RuntimeEvent]) -> Result<Vec<EventId>, ParityProjectionError> {
    if events.is_empty() {
        return Err(ParityProjectionError::SourceEmpty);
    }
    if events.len() > kiana_domain::MAX_SOURCE_EVENT_IDS {
        return Err(ParityProjectionError::ProjectionFailed(
            "source_event_limit".to_owned(),
        ));
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for event in events {
        if !seen.insert(event.event_id.to_string()) {
            return Err(ParityProjectionError::SourceDuplicate);
        }
        if ids.len() < kiana_domain::MAX_SOURCE_EVENT_IDS {
            ids.push(event.event_id);
        }
    }
    Ok(ids)
}

fn source_cursor(events: &[RuntimeEvent]) -> Result<(u64, u64), ParityProjectionError> {
    let first = events
        .first()
        .and_then(|event| event.data.get("source_cursor"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    if first == 0 {
        return Err(ParityProjectionError::ProjectionFailed(
            "source_cursor_invalid".to_owned(),
        ));
    }
    let mut last = first;
    for (index, event) in events.iter().enumerate() {
        let expected = first.checked_add(index as u64).ok_or_else(|| {
            ParityProjectionError::ProjectionFailed("source_cursor_overflow".to_owned())
        })?;
        if let Some(observed) = event
            .data
            .get("source_cursor")
            .and_then(serde_json::Value::as_u64)
        {
            if observed != expected || observed == 0 {
                return Err(ParityProjectionError::ProjectionFailed(
                    "source_cursor_gap".to_owned(),
                ));
            }
        }
        last = expected;
    }
    Ok((first, last))
}

fn run_status(run_id: RunId, events: &[RuntimeEvent]) -> (ExecutionStatus, Option<String>) {
    match project_run_state(run_id, events) {
        Ok(state) => {
            let status = match state.outcome {
                Some(RunOutcome::Completed) => ExecutionStatus::Completed,
                Some(RunOutcome::Failed) => ExecutionStatus::Failed,
                Some(RunOutcome::Cancelled) => ExecutionStatus::Cancelled,
                Some(RunOutcome::ResultUnknown) => ExecutionStatus::ResultUnknown,
                None => ExecutionStatus::Running,
            };
            let receipt_digest = json_digest(&json!({
                "run_id": state.run_id,
                "phase": format!("{:?}", state.phase),
                "outcome": state.outcome.map(|outcome| format!("{:?}", outcome)),
                "error": state.error,
            }));
            (status, Some(receipt_digest))
        }
        Err(_) => (ExecutionStatus::ResultUnknown, None),
    }
}

/// Derive one entry-point-neutral snapshot from a committed event view.
pub fn project_entrypoint_parity(
    entrypoint: EntryPointKind,
    run_id: Option<RunId>,
    events: &[RuntimeEvent],
) -> Result<EntryPointParitySnapshot, ParityProjectionError> {
    project_entrypoint_parity_with_capabilities(
        entrypoint,
        run_id,
        events,
        &EventStoreCapabilities::default(),
    )
}

fn project_entrypoint_parity_with_capabilities(
    entrypoint: EntryPointKind,
    run_id: Option<RunId>,
    events: &[RuntimeEvent],
    capabilities: &EventStoreCapabilities,
) -> Result<EntryPointParitySnapshot, ParityProjectionError> {
    let source_event_ids = source_ids(events)?;
    let (first_cursor, source_cursor) = source_cursor(events)?;
    let (mut status, receipt_digest) = run_id
        .map(|run_id| run_status(run_id, events))
        .unwrap_or((ExecutionStatus::Running, None));
    let mut limitations = Vec::new();
    let audit_digest = match rebuild_audit_projection(events, first_cursor) {
        Ok(snapshot) => Some(snapshot.digest()),
        Err(error) => {
            limitations.push(format!("audit_projection_unavailable:{error}"));
            status = if status == ExecutionStatus::Completed {
                ExecutionStatus::ResultUnknown
            } else {
                status
            };
            None
        }
    };
    let health_digest =
        match project_health_snapshot(events, capabilities, HealthProbeKind::Liveness, 1) {
            Ok(snapshot) => Some(snapshot.digest()),
            Err(error) => {
                limitations.push(format!("health_projection_unavailable:{error}"));
                None
            }
        };
    if receipt_digest.is_none() && run_id.is_some() {
        limitations.push("receipt_projection_unavailable".to_owned());
    }
    EntryPointParitySnapshot::new(
        entrypoint,
        source_cursor,
        source_event_ids,
        1,
        status,
        receipt_digest,
        audit_digest,
        health_digest,
        limitations,
    )
    .map_err(ParityProjectionError::SnapshotInvalid)
}

impl ControlPlane {
    /// Serve one owner-scoped parity snapshot to every entrypoint.
    ///
    /// The request is read-only, but ownership is still checked from the authenticated
    /// [`RequestContext`]. No adapter may substitute a run stream or caller-provided owner data
    /// for the committed EventLog view.
    pub async fn entrypoint_parity(
        &self,
        context: &RequestContext,
        entrypoint: EntryPointKind,
        requested_run: Option<RunId>,
    ) -> Result<kiana_domain::CoreResponse, CoreError> {
        let run_id = match self.resolve_run_id(context, requested_run).await? {
            Ok(run_id) => run_id,
            Err(reason) => {
                return Ok(kiana_domain::CoreResponse::blocked(
                    context.request_id,
                    reason,
                ))
            }
        };
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("entrypoint_parity_read_all_unsupported".to_owned())
        })?;
        let run_events = filter_run_events(&events, run_id);
        if run_events.is_empty() {
            return Ok(kiana_domain::CoreResponse::blocked(
                context.request_id,
                "run_not_found",
            ));
        }
        if run_events
            .iter()
            .find(|event| event.kind == "run.authorized")
            .is_none()
            || receipt_owner_mismatch(&run_events, context)
        {
            return Ok(kiana_domain::CoreResponse::blocked(
                context.request_id,
                "run_owner_mismatch",
            ));
        }
        let mut snapshot = project_entrypoint_parity_with_capabilities(
            entrypoint,
            Some(run_id),
            &run_events,
            &self.events.capabilities(),
        )
        .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        if self.run_data_revoked(run_id).await? {
            snapshot.status = ExecutionStatus::ResultUnknown;
            if snapshot.limitations.len() < kiana_domain::MAX_PARITY_LIMITATIONS {
                snapshot.limitations.push("data_revoked".to_owned());
            }
            snapshot.snapshot_digest = snapshot.digest();
            snapshot
                .validate()
                .map_err(|error| kiana_ports::PortError::Failed(error.to_owned()))?;
        }
        Ok(kiana_domain::CoreResponse::completed(
            context.request_id,
            serde_json::to_value(snapshot).map_err(|error| {
                kiana_ports::PortError::Failed(format!("entrypoint_parity_serialize:{error}"))
            })?,
        ))
    }
}
