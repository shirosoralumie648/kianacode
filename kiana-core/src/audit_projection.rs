//! Checkpointed, replayable AuditRecord projection.
//!
//! Audit facts remain in EventLog. This module only folds committed events into a bounded
//! snapshot/checkpoint and validates the source cursor before exposing it to query callers.

use kiana_domain::{
    AuditProjectionCheckpoint, AuditProjectionSnapshot, AuditRecord, EventCursor, EventId,
    RuntimeEvent, MAX_AUDIT_PROJECTION_RECORDS, MAX_SOURCE_EVENT_IDS,
};
use std::collections::HashSet;

use super::{ControlPlane, CoreError};

pub const AUDIT_PROJECTION_VERSION: u64 = 1;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuditProjectionError {
    #[error("audit_projection_source_empty")]
    SourceEmpty,
    #[error("audit_projection_source_cursor_invalid")]
    SourceCursorInvalid,
    #[error("audit_projection_source_cursor_gap")]
    SourceCursorGap,
    #[error("audit_projection_source_cursor_overflow")]
    SourceCursorOverflow,
    #[error("audit_projection_source_event_duplicate")]
    SourceEventDuplicate,
    #[error("audit_projection_record_limit")]
    RecordLimit,
    #[error("audit_projection_reduce_failed:{0}")]
    ReduceFailed(String),
    #[error("audit_projection_checkpoint_invalid:{0}")]
    CheckpointInvalid(String),
    #[error("audit_projection_checkpoint_mismatch")]
    CheckpointMismatch,
}

fn event_cursor(
    event: &RuntimeEvent,
    first_cursor: EventCursor,
    index: usize,
) -> Result<EventCursor, AuditProjectionError> {
    let expected = first_cursor
        .checked_add(index as u64)
        .ok_or(AuditProjectionError::SourceCursorOverflow)?;
    if let Some(cursor) = event.data.get("source_cursor") {
        let observed = cursor
            .as_u64()
            .filter(|cursor| *cursor > 0)
            .ok_or(AuditProjectionError::SourceCursorInvalid)?;
        if observed != expected {
            return Err(AuditProjectionError::SourceCursorGap);
        }
    }
    Ok(expected)
}

fn source_event_ids(events: &[RuntimeEvent]) -> Result<Vec<EventId>, AuditProjectionError> {
    if events.len() > MAX_SOURCE_EVENT_IDS {
        return Err(AuditProjectionError::RecordLimit);
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(events.len());
    for event in events {
        if !seen.insert(event.event_id.to_string()) {
            return Err(AuditProjectionError::SourceEventDuplicate);
        }
        ids.push(event.event_id);
    }
    Ok(ids)
}

fn validate_source(
    events: &[RuntimeEvent],
    first_cursor: EventCursor,
) -> Result<EventCursor, AuditProjectionError> {
    if events.is_empty() || first_cursor == 0 {
        return Err(if events.is_empty() {
            AuditProjectionError::SourceEmpty
        } else {
            AuditProjectionError::SourceCursorInvalid
        });
    }
    source_event_ids(events)?;
    let mut last = first_cursor;
    for (index, event) in events.iter().enumerate() {
        last = event_cursor(event, first_cursor, index)?;
    }
    Ok(last)
}

/// Rebuild the full audit projection from an ordered committed EventLog slice.
pub fn rebuild_audit_projection(
    events: &[RuntimeEvent],
    first_cursor: EventCursor,
) -> Result<AuditProjectionSnapshot, AuditProjectionError> {
    let last_cursor = validate_source(events, first_cursor)?;
    let source_ids = source_event_ids(events)?;
    let records = kiana_domain::reduce_audit_records(events, first_cursor)
        .map_err(AuditProjectionError::ReduceFailed)?;
    if records.len() > MAX_AUDIT_PROJECTION_RECORDS {
        return Err(AuditProjectionError::RecordLimit);
    }
    AuditProjectionSnapshot::new(
        AUDIT_PROJECTION_VERSION,
        last_cursor,
        source_ids,
        records,
        Vec::new(),
    )
    .map_err(AuditProjectionError::CheckpointInvalid)
}

/// Stateful projection facade for a commit observer or a restart replay.
#[derive(Clone, Debug)]
pub struct AuditProjection {
    snapshot: AuditProjectionSnapshot,
}

impl AuditProjection {
    pub fn rebuild(
        events: &[RuntimeEvent],
        first_cursor: EventCursor,
    ) -> Result<Self, AuditProjectionError> {
        Ok(Self {
            snapshot: rebuild_audit_projection(events, first_cursor)?,
        })
    }

    pub fn from_snapshot(snapshot: AuditProjectionSnapshot) -> Result<Self, AuditProjectionError> {
        snapshot
            .validate()
            .map_err(AuditProjectionError::CheckpointInvalid)?;
        Ok(Self { snapshot })
    }

    pub fn apply_page(
        &mut self,
        events: &[RuntimeEvent],
        first_cursor: EventCursor,
    ) -> Result<(), AuditProjectionError> {
        let expected = self
            .snapshot
            .source_cursor
            .checked_add(1)
            .ok_or(AuditProjectionError::SourceCursorOverflow)?;
        if first_cursor != expected {
            return Err(if first_cursor < expected {
                AuditProjectionError::SourceCursorInvalid
            } else {
                AuditProjectionError::SourceCursorGap
            });
        }
        let last_cursor = validate_source(events, first_cursor)?;
        let page_ids = source_event_ids(events)?;
        let mut all_ids = self.snapshot.source_event_ids.clone();
        let mut existing_ids = all_ids
            .iter()
            .map(ToString::to_string)
            .collect::<HashSet<_>>();
        for event_id in page_ids {
            if !existing_ids.insert(event_id.to_string()) {
                return Err(AuditProjectionError::SourceEventDuplicate);
            }
            all_ids.push(event_id);
            if all_ids.len() > MAX_SOURCE_EVENT_IDS {
                return Err(AuditProjectionError::RecordLimit);
            }
        }
        let appended = kiana_domain::reduce_audit_records(events, first_cursor)
            .map_err(AuditProjectionError::ReduceFailed)?;
        let mut records = self.snapshot.records.clone();
        records.extend(appended);
        if records.len() > MAX_AUDIT_PROJECTION_RECORDS {
            return Err(AuditProjectionError::RecordLimit);
        }
        self.snapshot = AuditProjectionSnapshot::new(
            self.snapshot.projection_version,
            last_cursor,
            all_ids,
            records,
            self.snapshot.limitations.clone(),
        )
        .map_err(AuditProjectionError::CheckpointInvalid)?;
        Ok(())
    }

    pub fn snapshot(&self) -> &AuditProjectionSnapshot {
        &self.snapshot
    }

    pub fn checkpoint(&self) -> &AuditProjectionCheckpoint {
        &self.snapshot.checkpoint
    }

    pub fn records(&self) -> &[AuditRecord] {
        &self.snapshot.records
    }

    pub fn restore(
        checkpoint: AuditProjectionCheckpoint,
        records: Vec<AuditRecord>,
    ) -> Result<Self, AuditProjectionError> {
        checkpoint
            .validate()
            .map_err(AuditProjectionError::CheckpointInvalid)?;
        let snapshot = AuditProjectionSnapshot::new(
            checkpoint.projection_version,
            checkpoint.source_cursor,
            checkpoint.source_event_ids.clone(),
            records,
            Vec::new(),
        )
        .map_err(AuditProjectionError::CheckpointInvalid)?;
        if snapshot.checkpoint != checkpoint {
            return Err(AuditProjectionError::CheckpointMismatch);
        }
        Ok(Self { snapshot })
    }
}

impl ControlPlane {
    /// Rebuild audit records from all committed facts; a run stream is never used as a global
    /// audit source when the adapter cannot provide read-all semantics.
    pub async fn audit_projection(&self) -> Result<AuditProjectionSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("audit_projection_read_all_unsupported".to_owned())
        })?;
        let first_cursor = events
            .first()
            .and_then(|event| event.data.get("source_cursor"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1);
        rebuild_audit_projection(&events, first_cursor)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
