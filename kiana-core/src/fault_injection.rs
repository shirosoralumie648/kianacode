//! Replay-only fault matrix for safety classification.
//!
//! The simulator records the invariants expected at each fault boundary. It does not inject a
//! process kill, call a handler, contact a provider, or write a new EventLog fact.

use kiana_domain::{
    EventId, FaultCase, FaultCaseStatus, FaultInjectionPoint, FaultMatrix, RuntimeEvent,
};
use std::collections::HashSet;

use super::{ControlPlane, CoreError};

const POINTS: [FaultInjectionPoint; 8] = [
    FaultInjectionPoint::Prepare,
    FaultInjectionPoint::Commit,
    FaultInjectionPoint::Dispatch,
    FaultInjectionPoint::Result,
    FaultInjectionPoint::Flush,
    FaultInjectionPoint::Projector,
    FaultInjectionPoint::Export,
    FaultInjectionPoint::Shutdown,
];

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FaultInjectionError {
    #[error("fault_seed_invalid")]
    SeedInvalid,
    #[error("fault_source_empty")]
    SourceEmpty,
    #[error("fault_source_cursor_invalid")]
    SourceCursorInvalid,
    #[error("fault_source_event_duplicate")]
    SourceDuplicate,
    #[error("fault_matrix_invalid:{0}")]
    MatrixInvalid(String),
}

fn source_ids(events: &[RuntimeEvent]) -> Result<Vec<EventId>, FaultInjectionError> {
    if events.is_empty() {
        return Err(FaultInjectionError::SourceEmpty);
    }
    if events.len() > kiana_domain::MAX_SOURCE_EVENT_IDS {
        return Err(FaultInjectionError::SourceCursorInvalid);
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for event in events {
        if !seen.insert(event.event_id.to_string()) {
            return Err(FaultInjectionError::SourceDuplicate);
        }
        ids.push(event.event_id);
    }
    Ok(ids)
}

fn case_for(
    point: FaultInjectionPoint,
    seed: u64,
    source_cursor: u64,
    source_event_ids: &[EventId],
) -> Result<FaultCase, FaultInjectionError> {
    let (status, effect_started, effect_known, error_code) = match point {
        FaultInjectionPoint::Prepare => (
            FaultCaseStatus::Rejected,
            false,
            true,
            "fault_prepare_rejected",
        ),
        FaultInjectionPoint::Commit => (
            FaultCaseStatus::Unknown,
            false,
            false,
            "fault_commit_outcome_unknown",
        ),
        FaultInjectionPoint::Dispatch => (
            FaultCaseStatus::Unknown,
            false,
            false,
            "fault_dispatch_unconfirmed",
        ),
        FaultInjectionPoint::Result => (
            FaultCaseStatus::Unknown,
            true,
            false,
            "fault_result_unconfirmed",
        ),
        FaultInjectionPoint::Flush => (
            FaultCaseStatus::Unknown,
            true,
            false,
            "fault_flush_unconfirmed",
        ),
        FaultInjectionPoint::Projector => (
            FaultCaseStatus::Unknown,
            true,
            false,
            "fault_projector_unavailable",
        ),
        FaultInjectionPoint::Export => (
            FaultCaseStatus::Unknown,
            true,
            false,
            "fault_export_delivery_unknown",
        ),
        FaultInjectionPoint::Shutdown => (
            FaultCaseStatus::Unknown,
            true,
            false,
            "fault_shutdown_in_flight",
        ),
    };
    FaultCase::new(
        point,
        seed,
        status,
        effect_started,
        effect_known,
        true,
        false,
        false,
        source_cursor,
        source_event_ids.to_vec(),
        error_code,
    )
    .map_err(FaultInjectionError::MatrixInvalid)
}

/// Build a deterministic eight-point safety matrix for one source cursor.
pub fn fault_matrix(
    seed: u64,
    source_cursor: u64,
    source_event_ids: Vec<EventId>,
) -> Result<FaultMatrix, FaultInjectionError> {
    if seed == 0 {
        return Err(FaultInjectionError::SeedInvalid);
    }
    if source_cursor == 0 || source_event_ids.is_empty() {
        return Err(FaultInjectionError::SourceCursorInvalid);
    }
    let cases = POINTS
        .into_iter()
        .map(|point| case_for(point, seed, source_cursor, &source_event_ids))
        .collect::<Result<Vec<_>, _>>()?;
    FaultMatrix::new(seed, source_cursor, source_event_ids, cases)
        .map_err(FaultInjectionError::MatrixInvalid)
}

pub fn fault_matrix_from_events(
    seed: u64,
    events: &[RuntimeEvent],
) -> Result<FaultMatrix, FaultInjectionError> {
    let source_event_ids = source_ids(events)?;
    let source_cursor = u64::try_from(events.len())
        .ok()
        .filter(|cursor| *cursor > 0)
        .ok_or(FaultInjectionError::SourceCursorInvalid)?;
    fault_matrix(seed, source_cursor, source_event_ids)
}

pub fn replay_fault_matrix(
    seed: u64,
    source_cursor: u64,
    source_event_ids: Vec<EventId>,
) -> Result<FaultMatrix, FaultInjectionError> {
    fault_matrix(seed, source_cursor, source_event_ids)
}

impl ControlPlane {
    /// Produce a replay-only fault matrix from committed source IDs.
    pub fn fault_matrix(
        &self,
        seed: u64,
        events: &[RuntimeEvent],
    ) -> Result<FaultMatrix, CoreError> {
        fault_matrix_from_events(seed, events)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
