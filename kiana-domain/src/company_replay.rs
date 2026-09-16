//! Deterministic Company event replay and the explicit legacy schema adapter.
//!
//! Replay validates stream metadata before applying the pure CompanyState transition. It never
//! re-runs a capability, Runner, provider or external effect.

use crate::{CompanyEvent, CompanyState, RuntimeEvent, COMPANY_EVENT_SCHEMA};
use serde_json::Value;
use std::collections::BTreeSet;

pub const LEGACY_COMPANY_EVENT_SCHEMA: &str = "kiana.company-event.v0";

#[derive(Clone, Debug, Default)]
pub struct CompanyReplayReducer {
    aggregate_id: String,
    project_root: String,
    owner_id: String,
    state: CompanyState,
    seen_idempotency: BTreeSet<String>,
    history: Vec<CompanyEvent>,
}

impl CompanyReplayReducer {
    pub fn new(
        aggregate_id: impl Into<String>,
        project_root: impl Into<String>,
        owner_id: impl Into<String>,
    ) -> Self {
        Self {
            aggregate_id: aggregate_id.into(),
            project_root: project_root.into(),
            owner_id: owner_id.into(),
            ..Self::default()
        }
    }

    pub fn apply(&mut self, runtime_event: &RuntimeEvent) -> Result<(), String> {
        if runtime_event.aggregate_type.as_deref() != Some("company")
            || runtime_event.aggregate_id.as_deref() != Some(self.aggregate_id.as_str())
        {
            return Err("company_replay_aggregate_mismatch".to_owned());
        }
        let stream_version = runtime_event
            .stream_version
            .ok_or_else(|| "company_replay_stream_version_missing".to_owned())?;
        let expected_version = self.state.revision.saturating_add(1);
        if stream_version != expected_version {
            return Err(if stream_version > expected_version {
                "company_replay_gap".to_owned()
            } else {
                "company_replay_version_regressed".to_owned()
            });
        }
        if runtime_event.sequence == 0 {
            return Err("company_replay_sequence_invalid".to_owned());
        }
        let value = migrate_company_event(runtime_event.data.clone())?;
        let record: CompanyEvent =
            serde_json::from_value(value).map_err(|_| "company_event_invalid".to_owned())?;
        if record.schema != COMPANY_EVENT_SCHEMA
            || record.request.schema != crate::COMPANY_COMMAND_SCHEMA
            || record.project_root != self.project_root
            || record.owner_id != self.owner_id
            || record.authority.actor_id != record.owner_id
            || record.request.expected_revision != self.state.revision
            || runtime_event.kind != format!("company.{}", record.request.command.event_name())
        {
            return Err("company_replay_conflict".to_owned());
        }
        let idempotency = record.request.idempotency_key.clone();
        let expected_idempotency = format!("company:{}:{}", self.aggregate_id, idempotency);
        if runtime_event.idempotency_key.as_deref() != Some(expected_idempotency.as_str()) {
            return Err("company_replay_idempotency_mismatch".to_owned());
        }
        if !self.seen_idempotency.insert(idempotency) {
            return Err("company_replay_duplicate_command".to_owned());
        }
        let state = self
            .state
            .transition(&record.request.command, &record.authority, &record.proof)
            .map_err(|error| format!("company_replay_state_transition:{error}"))?;
        if state.revision != stream_version {
            return Err("company_replay_revision_mismatch".to_owned());
        }
        self.state = state;
        self.history.push(record);
        Ok(())
    }

    pub fn state(&self) -> CompanyState {
        self.state.clone()
    }

    pub fn history(&self) -> &[CompanyEvent] {
        &self.history
    }

    pub fn into_parts(self) -> (CompanyState, Vec<CompanyEvent>) {
        (self.state, self.history)
    }
}

/// Convert the one explicitly supported legacy Company event schema to the current v1 shape.
/// Unknown major versions are rejected rather than guessed or silently upgraded.
pub fn migrate_company_event(mut value: Value) -> Result<Value, String> {
    let schema = value
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| "company_event_schema_missing".to_owned())?;
    match schema {
        COMPANY_EVENT_SCHEMA => Ok(value),
        LEGACY_COMPANY_EVENT_SCHEMA => {
            let object = value
                .as_object_mut()
                .ok_or_else(|| "company_event_invalid".to_owned())?;
            object.insert(
                "schema".to_owned(),
                Value::String(COMPANY_EVENT_SCHEMA.to_owned()),
            );
            Ok(value)
        }
        _ => Err("company_event_schema_unsupported".to_owned()),
    }
}
