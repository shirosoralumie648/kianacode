//! Read-only BQ-11 model-attempt lifecycle projection.
//!
//! This module folds only lifecycle facts that have already crossed the EventLog boundary.  It
//! never calls a provider, consumes a permit, settles a reservation or infers success from a
//! transcript.  Legacy `model.prepared` payloads that do not carry the BQ-11 schema are ignored
//! so older committed runs remain readable; new lifecycle kinds fail closed when malformed.

use crate::{ControlPlane, CoreError};
use kiana_domain::{
    apply_model_attempt_event, ModelAttemptEventKind, ModelAttemptLifecycleEvent,
    ModelAttemptLifecycleRecord, ModelAttemptState, RequestId, RuntimeEvent,
    MODEL_ATTEMPT_EVENT_SCHEMA,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ModelAttemptLifecycleProjectionError {
    #[error("model_attempt_lifecycle_event_invalid:{0}")]
    Invalid(String),
    #[error("model_attempt_lifecycle_transition_invalid:{0}")]
    Transition(String),
    #[error("model_attempt_lifecycle_source_empty")]
    SourceEmpty,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelAttemptLifecycleProjection {
    pub source_cursor: u64,
    pub source_event_ids: Vec<kiana_domain::EventId>,
    pub attempts: Vec<ModelAttemptLifecycleRecord>,
}

impl ModelAttemptLifecycleProjection {
    pub fn attempt(
        &self,
        attempt_id: kiana_domain::AttemptId,
    ) -> Option<&ModelAttemptLifecycleRecord> {
        self.attempts
            .iter()
            .find(|record| record.attempt_id == attempt_id)
    }
}

/// Rebuild BQ-11 records from committed RuntimeEvent facts.  A dispatch fact is accepted only
/// when its serialized event says that the prepared append was flushed and includes the exact
/// permit carried by the prepared fact.
pub fn project_model_attempt_lifecycle(
    events: &[RuntimeEvent],
) -> Result<ModelAttemptLifecycleProjection, ModelAttemptLifecycleProjectionError> {
    let mut records: BTreeMap<kiana_domain::AttemptId, ModelAttemptLifecycleRecord> =
        BTreeMap::new();
    let mut source_cursor = 0;
    let mut source_event_ids = Vec::new();
    let mut saw_lifecycle = false;

    for event in events {
        let lifecycle_kind = matches!(
            event.kind.as_str(),
            "model.prepared"
                | "model.dispatching"
                | "model.observed"
                | "model.settled"
                | "model.unknown"
        );
        if !lifecycle_kind {
            continue;
        }
        if source_event_ids.contains(&event.event_id) {
            return Err(ModelAttemptLifecycleProjectionError::Invalid(
                "model_attempt_duplicate_source_event".to_owned(),
            ));
        }

        // The old model.prepared event has a different payload and remains readable by the
        // legacy model budget path.  It is not silently treated as a BQ-11 fact.
        if event.data.get("schema").and_then(serde_json::Value::as_str)
            != Some(MODEL_ATTEMPT_EVENT_SCHEMA)
        {
            if event.kind == "model.prepared" {
                continue;
            }
            return Err(ModelAttemptLifecycleProjectionError::Invalid(
                "model_attempt_lifecycle_schema_required".to_owned(),
            ));
        }
        saw_lifecycle = true;
        let fact: ModelAttemptLifecycleEvent =
            serde_json::from_value(event.data.clone()).map_err(|_| {
                ModelAttemptLifecycleProjectionError::Invalid(
                    "model_attempt_lifecycle_decode_failed".to_owned(),
                )
            })?;
        if fact.kind.as_str() != event.kind {
            return Err(ModelAttemptLifecycleProjectionError::Invalid(
                "model_attempt_lifecycle_kind_mismatch".to_owned(),
            ));
        }
        fact.validate()
            .map_err(ModelAttemptLifecycleProjectionError::Invalid)?;
        if fact.kind == ModelAttemptEventKind::Prepared && records.contains_key(&fact.attempt_id) {
            return Err(ModelAttemptLifecycleProjectionError::Transition(
                "model_attempt_duplicate_prepared".to_owned(),
            ));
        }
        let current = records.remove(&fact.attempt_id);
        let next =
            apply_model_attempt_event(current, &fact, Some(event.event_id)).map_err(|reason| {
                if reason.contains("transition")
                    || reason.contains("revision")
                    || reason.contains("requires_prepared")
                {
                    ModelAttemptLifecycleProjectionError::Transition(reason)
                } else {
                    ModelAttemptLifecycleProjectionError::Invalid(reason)
                }
            })?;
        source_cursor = source_cursor.max(event.stream_version.unwrap_or(event.sequence));
        if !source_event_ids.contains(&event.event_id) {
            source_event_ids.push(event.event_id);
        }
        records.insert(fact.attempt_id, next);
    }

    if !saw_lifecycle {
        return Err(ModelAttemptLifecycleProjectionError::SourceEmpty);
    }
    let attempts = records.into_values().collect::<Vec<_>>();
    Ok(ModelAttemptLifecycleProjection {
        source_cursor,
        source_event_ids,
        attempts,
    })
}

/// Shared core-side dispatch fence.  It is intentionally read-only and must run immediately
/// before the ControlPlane's existing provider adapter call.
pub fn validate_model_attempt_dispatch(
    record: &ModelAttemptLifecycleRecord,
    permit_id: Option<RequestId>,
    prepared_flushed: bool,
) -> Result<(), ModelAttemptLifecycleProjectionError> {
    record
        .validate()
        .map_err(ModelAttemptLifecycleProjectionError::Invalid)?;
    if record.state != ModelAttemptState::Prepared {
        return Err(ModelAttemptLifecycleProjectionError::Transition(
            "model_attempt_dispatch_state_invalid".to_owned(),
        ));
    }
    if !prepared_flushed || !record.prepared_flushed {
        return Err(ModelAttemptLifecycleProjectionError::Transition(
            "model_attempt_prepared_not_flushed".to_owned(),
        ));
    }
    let Some(permit_id) = permit_id else {
        return Err(ModelAttemptLifecycleProjectionError::Transition(
            "model_attempt_permit_required".to_owned(),
        ));
    };
    if record.permit_id != Some(permit_id) {
        return Err(ModelAttemptLifecycleProjectionError::Transition(
            "model_attempt_permit_mismatch".to_owned(),
        ));
    }
    Ok(())
}

impl ControlPlane {
    /// Rebuild model-attempt lifecycle state from the persisted EventLog only.
    pub async fn model_attempt_lifecycle(
        &self,
        run_id: kiana_domain::RunId,
    ) -> Result<ModelAttemptLifecycleProjection, CoreError> {
        let events = self.events_for_persisted_run(run_id).await?;
        project_model_attempt_lifecycle(&events)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
