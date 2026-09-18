//! Generic, read-only replay/checkpoint helper for EventLog-backed projections.
//!
//! The helper owns no EventStore or authorization capability. A caller supplies a pure fold
//! function; failures leave the source journal untouched and callers can rebuild from zero.

use kiana_domain::{
    EventCursor, EventId, ProjectionCheckpoint, RuntimeEvent, MAX_SOURCE_EVENT_IDS,
};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionDriverStatus {
    Ready,
    Paused,
    RebuildRequired,
}

/// Pure projector driver state. It only folds caller-supplied committed events; persistence of
/// the checkpoint is a separate ProjectionStorePort concern.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionDriver {
    projector: String,
    checkpoint: Option<ProjectionCheckpoint>,
    status: ProjectionDriverStatus,
    retry_count: u32,
    last_error: Option<String>,
}

impl ProjectionDriver {
    pub fn new(projector: impl Into<String>) -> Result<Self, String> {
        let projector = projector.into();
        if projector.trim().is_empty() || projector.len() > 256 {
            return Err("projection_driver_projector_invalid".to_owned());
        }
        Ok(Self {
            projector,
            checkpoint: None,
            status: ProjectionDriverStatus::Ready,
            retry_count: 0,
            last_error: None,
        })
    }

    pub fn apply<F>(
        &mut self,
        initial_state: Value,
        events: &[RuntimeEvent],
        source_cursor: EventCursor,
        fold: F,
    ) -> Result<(), String>
    where
        F: FnMut(Value, &RuntimeEvent) -> Result<Value, String>,
    {
        if self.status == ProjectionDriverStatus::Paused {
            return Err("projection_driver_paused".to_owned());
        }
        let result = match self.checkpoint.as_ref() {
            Some(checkpoint) => ReplayProjection::from_checkpoint(
                &self.projector,
                checkpoint,
                events,
                source_cursor,
                fold,
            ),
            None => ReplayProjection::from_zero(
                &self.projector,
                initial_state,
                events,
                source_cursor,
                fold,
            ),
        };
        match result {
            Ok(projection) => {
                self.checkpoint = Some(projection.checkpoint()?);
                self.status = ProjectionDriverStatus::Ready;
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                self.status = ProjectionDriverStatus::Paused;
                self.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub fn retry(&mut self) -> Result<(), String> {
        if self.status != ProjectionDriverStatus::Paused {
            return Err("projection_driver_not_paused".to_owned());
        }
        self.retry_count = self
            .retry_count
            .checked_add(1)
            .ok_or_else(|| "projection_driver_retry_overflow".to_owned())?;
        self.status = ProjectionDriverStatus::Ready;
        Ok(())
    }

    pub fn rebuild(&mut self) {
        self.checkpoint = None;
        self.status = ProjectionDriverStatus::RebuildRequired;
        self.last_error = None;
    }

    pub fn checkpoint(&self) -> Option<&ProjectionCheckpoint> {
        self.checkpoint.as_ref()
    }

    pub fn status(&self) -> ProjectionDriverStatus {
        self.status
    }

    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayProjection {
    projector: String,
    source_cursor: EventCursor,
    source_event_ids: Vec<EventId>,
    state: Value,
}

impl ReplayProjection {
    pub fn from_zero<F>(
        projector: &str,
        initial_state: Value,
        events: &[RuntimeEvent],
        source_cursor: EventCursor,
        mut fold: F,
    ) -> Result<Self, String>
    where
        F: FnMut(Value, &RuntimeEvent) -> Result<Value, String>,
    {
        let (state, source_event_ids) = apply_events(initial_state, Vec::new(), events, &mut fold)?;
        Self::build(projector, source_cursor, source_event_ids, state)
    }

    pub fn from_checkpoint<F>(
        projector: &str,
        checkpoint: &ProjectionCheckpoint,
        tail: &[RuntimeEvent],
        source_cursor: EventCursor,
        mut fold: F,
    ) -> Result<Self, String>
    where
        F: FnMut(Value, &RuntimeEvent) -> Result<Value, String>,
    {
        checkpoint.validate()?;
        if checkpoint.projector != projector {
            return Err("projection_checkpoint_projector_mismatch".to_owned());
        }
        if tail.is_empty() {
            if source_cursor != checkpoint.source_cursor {
                return Err("projection_checkpoint_cursor_gap".to_owned());
            }
        } else if source_cursor <= checkpoint.source_cursor {
            return Err("projection_checkpoint_cursor_not_advanced".to_owned());
        }
        let (state, source_event_ids) = apply_events(
            checkpoint.state.clone(),
            checkpoint.source_event_ids.clone(),
            tail,
            &mut fold,
        )?;
        Self::build(projector, source_cursor, source_event_ids, state)
    }

    pub fn checkpoint(&self) -> Result<ProjectionCheckpoint, String> {
        ProjectionCheckpoint::new(
            &self.projector,
            self.source_cursor,
            self.source_event_ids.clone(),
            self.state.clone(),
        )
    }

    pub fn projector(&self) -> &str {
        &self.projector
    }

    pub fn source_cursor(&self) -> EventCursor {
        self.source_cursor
    }

    pub fn source_event_ids(&self) -> &[EventId] {
        &self.source_event_ids
    }

    pub fn state(&self) -> &Value {
        &self.state
    }

    /// A projection can be compared or serialized without granting any execution capability.
    pub fn state_digest(&self) -> String {
        kiana_domain::json_digest(&self.state)
    }

    fn build(
        projector: &str,
        source_cursor: EventCursor,
        source_event_ids: Vec<EventId>,
        state: Value,
    ) -> Result<Self, String> {
        let checkpoint = ProjectionCheckpoint::new(
            projector,
            source_cursor,
            source_event_ids.clone(),
            state.clone(),
        )?;
        Ok(Self {
            projector: checkpoint.projector,
            source_cursor: checkpoint.source_cursor,
            source_event_ids: checkpoint.source_event_ids,
            state: checkpoint.state,
        })
    }
}

fn apply_events<F>(
    mut state: Value,
    source_event_ids: Vec<EventId>,
    events: &[RuntimeEvent],
    fold: &mut F,
) -> Result<(Value, Vec<EventId>), String>
where
    F: FnMut(Value, &RuntimeEvent) -> Result<Value, String>,
{
    let mut seen = source_event_ids.into_iter().collect::<BTreeSet<_>>();
    if seen.len() > MAX_SOURCE_EVENT_IDS {
        return Err("projection_checkpoint_source_event_limit".to_owned());
    }
    for event in events {
        if event.event_id.as_uuid().is_nil() {
            return Err("projection_event_id_invalid".to_owned());
        }
        if !seen.insert(event.event_id) {
            continue;
        }
        if seen.len() > MAX_SOURCE_EVENT_IDS {
            return Err("projection_checkpoint_source_event_limit".to_owned());
        }
        state = fold(state, event).map_err(|reason| format!("projection_fold_failed:{reason}"))?;
    }
    Ok((state, seen.into_iter().collect()))
}
