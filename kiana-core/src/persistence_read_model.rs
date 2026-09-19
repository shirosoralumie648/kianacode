//! PD-10 read model assembled only from committed EventLog facts.

use crate::receipts::{receipt_from_events, try_filter_run_events};
use crate::{
    project_invocations, project_run_state, InvocationProjection, RunOutcome, RunPhase, RunState,
};
use kiana_domain::{EventId, RequestContext, RunId, RuntimeEvent};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;

pub const PERSISTENCE_READ_MODEL_SCHEMA: &str = "kiana.persistence-read-model.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectedRunState {
    pub phase: String,
    pub outcome: Option<String>,
    pub error: Option<String>,
}

impl From<RunState> for ProjectedRunState {
    fn from(state: RunState) -> Self {
        let phase = match state.phase {
            RunPhase::Authorized => "authorized",
            RunPhase::Queued => "queued",
            RunPhase::Cancelling => "cancelling",
            RunPhase::Running => "running",
            RunPhase::AwaitingApproval => "awaiting_approval",
            RunPhase::Terminal => "terminal",
        };
        let outcome = state.outcome.map(|outcome| match outcome {
            RunOutcome::Completed => "completed",
            RunOutcome::Failed => "failed",
            RunOutcome::Cancelled => "cancelled",
            RunOutcome::ResultUnknown => "result_unknown",
        });
        Self {
            phase: phase.to_owned(),
            outcome: outcome.map(str::to_owned),
            error: state.error,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PersistenceReadModel {
    pub schema: String,
    pub run_id: RunId,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub run: ProjectedRunState,
    pub invocations: Vec<InvocationProjection>,
    pub receipt: Value,
}

impl PersistenceReadModel {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PERSISTENCE_READ_MODEL_SCHEMA
            || self.run_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.iter().any(|id| id.as_uuid().is_nil())
        {
            return Err("persistence_read_model_header_invalid".to_owned());
        }
        let unique = self
            .source_event_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if unique.len() != self.source_event_ids.len() {
            return Err("persistence_read_model_event_duplicate".to_owned());
        }
        if self.receipt.get("run_id") != Some(&json!(self.run_id)) {
            return Err("persistence_read_model_receipt_run_mismatch".to_owned());
        }
        if self.receipt.get("source_cursor") != Some(&json!(self.source_cursor)) {
            return Err("persistence_read_model_receipt_cursor_mismatch".to_owned());
        }
        Ok(())
    }
}

pub fn project_persistence_read_model(
    context: &RequestContext,
    run_id: RunId,
    events: &[RuntimeEvent],
    source_cursor: u64,
    expected_data_epoch: Option<u64>,
) -> Result<PersistenceReadModel, String> {
    if source_cursor == 0 || run_id.as_uuid().is_nil() {
        return Err("persistence_read_model_source_invalid".to_owned());
    }
    let run_events = try_filter_run_events(events, run_id)?;
    if run_events.is_empty() {
        return Err("persistence_read_model_source_empty".to_owned());
    }
    if expected_data_epoch.is_some_and(|epoch| {
        run_events
            .iter()
            .any(|event| event.data_epoch != Some(epoch))
    }) {
        return Err("persistence_read_model_data_epoch_mismatch".to_owned());
    }
    let mut source_event_ids = Vec::with_capacity(run_events.len());
    let mut seen = HashSet::new();
    for event in &run_events {
        if !seen.insert(event.event_id) {
            return Err("persistence_read_model_event_duplicate".to_owned());
        }
        source_event_ids.push(event.event_id);
    }
    let run = project_run_state(run_id, &run_events)
        .map_err(|error| format!("persistence_read_model_run:{error}"))?
        .into();
    let invocations = project_invocations(run_id, &run_events)
        .map_err(|error| format!("persistence_read_model_invocation:{error}"))?;
    let mut receipt = receipt_from_events(context, run_id, "read-only", Value::Null, &run_events);
    if let Some(object) = receipt.as_object_mut() {
        object.insert("source_cursor".to_owned(), json!(source_cursor));
        object.insert("source_event_ids".to_owned(), json!(source_event_ids));
    } else {
        return Err("persistence_read_model_receipt_invalid".to_owned());
    }
    let model = PersistenceReadModel {
        schema: PERSISTENCE_READ_MODEL_SCHEMA.to_owned(),
        run_id,
        source_cursor,
        source_event_ids,
        run,
        invocations,
        receipt,
    };
    model.validate()?;
    Ok(model)
}
