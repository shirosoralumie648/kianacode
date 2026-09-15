//! Checkpointed, replayable AuditRecord projection.
//!
//! Audit facts remain in EventLog. This module only folds committed events into a bounded
//! snapshot/checkpoint and validates the source cursor before exposing it to query callers.

use kiana_domain::{
    AuditActionKind, AuditDecision, AuditProjectionCheckpoint, AuditProjectionSnapshot,
    AuditRecord, CoreResponse, EventCursor, EventId, RequestContext, RuntimeEvent,
    MAX_AUDIT_PROJECTION_RECORDS, MAX_SOURCE_EVENT_IDS,
};
use serde_json::Value;
use std::collections::HashSet;

use super::{ControlPlane, CoreError};

pub const AUDIT_PROJECTION_VERSION: u64 = 1;
const MAX_AUDIT_QUERY_LIMIT: usize = 1_000;

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
    #[error("audit_query_limit_invalid")]
    QueryLimitInvalid,
    #[error("audit_query_cursor_invalid")]
    QueryCursorInvalid,
    #[error("audit_query_cursor_stale")]
    QueryCursorStale,
    #[error("audit_query_filter_invalid")]
    QueryFilterInvalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditQueryInput {
    pub source_cursor: Option<EventCursor>,
    pub after_cursor: EventCursor,
    pub limit: usize,
    pub action_kind: Option<AuditActionKind>,
    pub decision: Option<AuditDecision>,
    pub target_kind: Option<String>,
}

impl AuditQueryInput {
    pub fn validate(&self) -> Result<(), AuditProjectionError> {
        if self.limit == 0 || self.limit > MAX_AUDIT_QUERY_LIMIT {
            return Err(AuditProjectionError::QueryLimitInvalid);
        }
        if self.source_cursor == Some(0)
            || self.after_cursor > self.source_cursor.unwrap_or(u64::MAX)
        {
            return Err(AuditProjectionError::QueryCursorInvalid);
        }
        if let Some(target_kind) = &self.target_kind {
            if target_kind.trim().is_empty() || target_kind.len() > 128 {
                return Err(AuditProjectionError::QueryFilterInvalid);
            }
        }
        Ok(())
    }
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

    /// Query server-derived audit records inside the authenticated session/project scope.
    ///
    /// The filter is applied to a rebuilt projection; it never exposes raw RuntimeEvent payloads
    /// and it never trusts caller-provided owner/scope strings. A stale source cursor is rejected
    /// instead of returning a silently old page.
    pub async fn query_audit(
        &self,
        context: &RequestContext,
        query: AuditQueryInput,
    ) -> Result<CoreResponse, CoreError> {
        query
            .validate()
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("audit_query_read_all_unsupported".to_owned())
        })?;
        let first_cursor = events
            .first()
            .and_then(|event| event.data.get("source_cursor"))
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let projection = rebuild_audit_projection(&events, first_cursor)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        if query
            .source_cursor
            .is_some_and(|cursor| cursor != projection.source_cursor)
        {
            return Err(
                kiana_ports::PortError::Conflict("audit_query_cursor_stale".to_owned()).into(),
            );
        }

        let actor = context
            .actor_id
            .as_deref()
            .filter(|actor| !actor.trim().is_empty())
            .ok_or_else(|| {
                kiana_ports::PortError::Failed("audit_query_unauthenticated".to_owned())
            })?;
        let owned_runs = events
            .iter()
            .filter(|event| event.kind == "run.authorized")
            .filter(|event| {
                event.data.get("actor_id").and_then(Value::as_str) == Some(actor)
                    && event.data.get("session_id").and_then(Value::as_str)
                        == Some(context.session_id.as_str())
                    && event
                        .data
                        .get("project_root")
                        .and_then(Value::as_str)
                        .is_some_and(|project| {
                            Self::canonical_project_root(project)
                                == Self::canonical_project_root(&context.project_root)
                        })
            })
            .filter_map(|event| event.data.get("run_id").and_then(Value::as_str))
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>();
        let owned_requests = events
            .iter()
            .filter(|event| {
                event
                    .data
                    .get("run_id")
                    .and_then(Value::as_str)
                    .is_some_and(|run| owned_runs.contains(run))
            })
            .map(|event| event.request_id.to_string())
            .collect::<std::collections::HashSet<_>>();
        let owned_approvals = events
            .iter()
            .filter(|event| {
                event
                    .data
                    .get("run_id")
                    .and_then(Value::as_str)
                    .is_some_and(|run| owned_runs.contains(run))
            })
            .filter_map(|event| event.data.get("approval_id").and_then(Value::as_str))
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>();
        let owned_event_ids = events
            .iter()
            .filter(|event| {
                event
                    .data
                    .get("run_id")
                    .and_then(Value::as_str)
                    .is_some_and(|run| owned_runs.contains(run))
                    || owned_requests.contains(&event.request_id.to_string())
                    || event
                        .data
                        .get("approval_id")
                        .and_then(Value::as_str)
                        .is_some_and(|approval| owned_approvals.contains(approval))
                    || (event.aggregate_type.as_deref() == Some("run")
                        && event
                            .aggregate_id
                            .as_deref()
                            .is_some_and(|run| owned_runs.contains(run)))
            })
            .map(|event| event.event_id.to_string())
            .collect::<std::collections::HashSet<_>>();

        let mut records = projection
            .records
            .into_iter()
            .filter(|record| {
                record
                    .source_event_ids
                    .iter()
                    .any(|event_id| owned_event_ids.contains(&event_id.to_string()))
            })
            .filter(|record| record.source_cursor > query.after_cursor)
            .filter(|record| {
                query
                    .action_kind
                    .is_none_or(|kind| record.action_kind == kind)
            })
            .filter(|record| {
                query
                    .decision
                    .is_none_or(|decision| record.decision == decision)
            })
            .filter(|record| {
                query
                    .target_kind
                    .as_deref()
                    .is_none_or(|target| record.target_kind == target)
            })
            .collect::<Vec<_>>();
        let next_cursor = if records.len() > query.limit {
            let cursor = records[query.limit - 1].source_cursor;
            records.truncate(query.limit);
            Some(cursor)
        } else {
            None
        };
        Ok(CoreResponse::completed(
            context.request_id,
            serde_json::json!({
                "schema": "kiana.audit-query.v1",
                "records": records,
                "next_cursor": next_cursor,
                "source_cursor": projection.source_cursor,
                "projection_version": projection.projection_version,
                "limitations": projection.limitations,
            }),
        ))
    }
}
