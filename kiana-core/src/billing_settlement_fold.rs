//! Read-only BQ-12 settlement/release/unknown projection.
//!
//! The projector consumes only committed `usage.*` RuntimeEvent facts. It never dispatches a
//! provider request, releases a permit, mutates a quota adapter or infers a successful result from
//! a transcript. Unknown and partial folds therefore remain visible to the caller for explicit
//! reconciliation.

use crate::{ControlPlane, CoreError};
use kiana_domain::{
    apply_settlement_fold_event, RuntimeEvent, SettlementFoldEvent, SettlementFoldEventKind,
    SettlementFoldRecord, SETTLEMENT_FOLD_EVENT_SCHEMA,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SettlementFoldProjectionError {
    #[error("settlement_fold_event_invalid:{0}")]
    Invalid(String),
    #[error("settlement_fold_transition_invalid:{0}")]
    Transition(String),
    #[error("settlement_fold_source_empty")]
    SourceEmpty,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettlementFoldProjection {
    pub source_cursor: u64,
    pub source_event_ids: Vec<kiana_domain::EventId>,
    pub records: Vec<SettlementFoldRecord>,
}

impl SettlementFoldProjection {
    pub fn attempt(&self, attempt_id: kiana_domain::AttemptId) -> Option<&SettlementFoldRecord> {
        self.records
            .iter()
            .find(|record| record.attempt_id == attempt_id)
    }
}

/// Rebuild settlement state from EventLog facts. A repeated exact event is rejected at the source
/// boundary rather than being allowed to double-release or double-charge a reservation.
pub fn project_settlement_folds(
    events: &[RuntimeEvent],
) -> Result<SettlementFoldProjection, SettlementFoldProjectionError> {
    let mut records: BTreeMap<kiana_domain::AttemptId, SettlementFoldRecord> = BTreeMap::new();
    let mut source_cursor = 0;
    let mut source_event_ids = Vec::new();
    let mut saw_fold = false;

    for event in events {
        let fold_kind = matches!(
            event.kind.as_str(),
            "usage.reserved"
                | "usage.observed"
                | "usage.settled"
                | "usage.released"
                | "usage.unknown"
        );
        if !fold_kind {
            continue;
        }
        saw_fold = true;
        if source_event_ids.contains(&event.event_id) {
            return Err(SettlementFoldProjectionError::Invalid(
                "settlement_duplicate_source_event".to_owned(),
            ));
        }
        if event.data.get("schema").and_then(serde_json::Value::as_str)
            != Some(SETTLEMENT_FOLD_EVENT_SCHEMA)
        {
            return Err(SettlementFoldProjectionError::Invalid(
                "settlement_fold_schema_required".to_owned(),
            ));
        }
        let fact: SettlementFoldEvent =
            serde_json::from_value(event.data.clone()).map_err(|_| {
                SettlementFoldProjectionError::Invalid("settlement_fold_decode_failed".to_owned())
            })?;
        if fact.kind.as_str() != event.kind {
            return Err(SettlementFoldProjectionError::Invalid(
                "settlement_fold_kind_mismatch".to_owned(),
            ));
        }
        if fact.kind == SettlementFoldEventKind::Unknown && !fact.reconciliation_required {
            return Err(SettlementFoldProjectionError::Transition(
                "settlement_unknown_requires_reconciliation".to_owned(),
            ));
        }
        fact.validate()
            .map_err(SettlementFoldProjectionError::Invalid)?;
        let current = records.remove(&fact.attempt_id);
        let next = apply_settlement_fold_event(current, &fact).map_err(|reason| {
            if reason.contains("transition")
                || reason.contains("duplicate")
                || reason.contains("revision")
                || reason.contains("release")
            {
                SettlementFoldProjectionError::Transition(reason)
            } else {
                SettlementFoldProjectionError::Invalid(reason)
            }
        })?;
        source_cursor = source_cursor.max(event.stream_version.unwrap_or(event.sequence));
        source_event_ids.push(event.event_id);
        records.insert(fact.attempt_id, next);
    }

    if !saw_fold {
        return Err(SettlementFoldProjectionError::SourceEmpty);
    }
    Ok(SettlementFoldProjection {
        source_cursor,
        source_event_ids,
        records: records.into_values().collect(),
    })
}

/// Singular alias used by adapters that project one billing stream at a time.
pub fn project_settlement_fold(
    events: &[RuntimeEvent],
) -> Result<SettlementFoldProjection, SettlementFoldProjectionError> {
    project_settlement_folds(events)
}

impl ControlPlane {
    /// Rebuild BQ-12 settlement state from the persisted EventLog only.
    pub async fn settlement_folds(
        &self,
        run_id: kiana_domain::RunId,
    ) -> Result<SettlementFoldProjection, CoreError> {
        let events = self.events_for_persisted_run(run_id).await?;
        project_settlement_folds(&events)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
