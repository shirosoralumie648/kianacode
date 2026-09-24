//! Read-only projector for BQ-14 append-only ledger facts.
//!
//! The projector rebuilds a deterministic view from committed `cost.ledger_entry` and
//! `cost.corrected` RuntimeEvents. It never writes EventLog facts, changes an original entry or
//! accepts a model transcript as a correction authority.

use kiana_domain::{
    CostCorrection, CostLedger, CostLedgerEntry, CostLedgerView, EventId, RunId, RuntimeEvent,
    SchemaVersion, COST_CORRECTION_EVENT, COST_LEDGER_ENTRY_EVENT,
};
use std::collections::BTreeSet;

pub const COST_LEDGER_PROJECTION_SCHEMA: &str = "kiana.cost-ledger-projection.v1";
pub const COST_LEDGER_PROJECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CostLedgerProjectionError {
    #[error("cost_ledger_projection_run_invalid")]
    RunInvalid,
    #[error("cost_ledger_projection_source_empty")]
    SourceEmpty,
    #[error("cost_ledger_projection_invalid:{0}")]
    Invalid(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CostLedgerProjection {
    pub schema: &'static str,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub ledger: CostLedger,
    pub views: Vec<CostLedgerView>,
}

impl CostLedgerProjection {
    pub fn validate(&self) -> Result<(), CostLedgerProjectionError> {
        if self.schema != COST_LEDGER_PROJECTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&COST_LEDGER_PROJECTION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.iter().any(|id| id.as_uuid().is_nil())
        {
            return Err(CostLedgerProjectionError::Invalid(
                "cost_ledger_projection_header_invalid".to_owned(),
            ));
        }
        if self
            .views
            .iter()
            .any(|view| view.original.run_id != self.run_id)
        {
            return Err(CostLedgerProjectionError::Invalid(
                "cost_ledger_projection_run_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn view_for(&self, entry_id: kiana_domain::LedgerEntryId) -> Option<&CostLedgerView> {
        self.views
            .iter()
            .find(|view| view.original.entry_id == entry_id)
    }
}

/// Rebuild one run's ledger projection from committed EventLog facts.
pub fn project_cost_ledger(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostLedgerProjection, CostLedgerProjectionError> {
    if run_id.as_uuid().is_nil() {
        return Err(CostLedgerProjectionError::RunInvalid);
    }
    let mut ledger = CostLedger::default();
    let mut source_event_ids = Vec::new();
    let mut seen_event_ids = BTreeSet::new();
    let mut source_cursor = 0;
    let mut previous_sequence = 0;
    let run_text = run_id.to_string();
    for event in events {
        if event.kind != COST_LEDGER_ENTRY_EVENT && event.kind != COST_CORRECTION_EVENT {
            continue;
        }
        if !seen_event_ids.insert(event.event_id) {
            return Err(CostLedgerProjectionError::Invalid(
                "cost_ledger_projection_duplicate_source_event".to_owned(),
            ));
        }
        if event.sequence == 0 || event.sequence <= previous_sequence {
            return Err(CostLedgerProjectionError::Invalid(
                "cost_ledger_projection_sequence_regression".to_owned(),
            ));
        }
        previous_sequence = event.sequence;
        source_cursor = source_cursor.max(event.sequence);
        source_event_ids.push(event.event_id);
        match event.kind.as_str() {
            COST_LEDGER_ENTRY_EVENT => {
                let entry: CostLedgerEntry =
                    serde_json::from_value(event.data.clone()).map_err(|_| {
                        CostLedgerProjectionError::Invalid("entry_decode_failed".to_owned())
                    })?;
                entry
                    .validate()
                    .map_err(CostLedgerProjectionError::Invalid)?;
                if entry.run_id != run_id
                    || event.data.get("run_id").and_then(serde_json::Value::as_str)
                        != Some(run_text.as_str())
                    || event.aggregate_type.as_deref() != Some("cost_ledger")
                    || event.aggregate_id.as_deref() != Some(entry.entry_id.to_string().as_str())
                    || event.stream_version != Some(entry.revision)
                {
                    return Err(CostLedgerProjectionError::Invalid(
                        "entry_identity_or_revision_mismatch".to_owned(),
                    ));
                }
                ledger
                    .append_entry(entry)
                    .map_err(CostLedgerProjectionError::Invalid)?;
            }
            COST_CORRECTION_EVENT => {
                let correction: CostCorrection = serde_json::from_value(event.data.clone())
                    .map_err(|_| {
                        CostLedgerProjectionError::Invalid("correction_decode_failed".to_owned())
                    })?;
                if correction.run_id != run_id
                    || event.data.get("run_id").and_then(serde_json::Value::as_str)
                        != Some(run_text.as_str())
                    || event.aggregate_type.as_deref() != Some("cost_ledger")
                    || event.aggregate_id.as_deref()
                        != Some(correction.target_entry_id.to_string().as_str())
                    || event.stream_version.is_none_or(|version| version == 0)
                {
                    return Err(CostLedgerProjectionError::Invalid(
                        "correction_identity_or_revision_mismatch".to_owned(),
                    ));
                }
                let command = correction_to_command(&correction);
                ledger
                    .append_correction(command)
                    .map_err(CostLedgerProjectionError::Invalid)?;
            }
            _ => unreachable!("filtered above"),
        }
    }
    if source_event_ids.is_empty() {
        return Err(CostLedgerProjectionError::SourceEmpty);
    }
    let views = ledger
        .entries()
        .map(|entry| ledger.view(entry.entry_id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(CostLedgerProjectionError::Invalid)?;
    let projection = CostLedgerProjection {
        schema: COST_LEDGER_PROJECTION_SCHEMA,
        version: COST_LEDGER_PROJECTION_VERSION,
        run_id,
        source_cursor,
        source_event_ids,
        ledger,
        views,
    };
    projection.validate()?;
    Ok(projection)
}

/// Correction facts are already approved and immutable. Reconstructing a command here only
/// gives the reducer its idempotency key and approval subject; no new approval is inferred.
fn correction_to_command(correction: &CostCorrection) -> kiana_domain::CostCorrectionCommand {
    let approval = kiana_domain::CostCorrectionApproval {
        schema: kiana_domain::COST_CORRECTION_APPROVAL_SCHEMA.to_owned(),
        version: kiana_domain::COST_LEDGER_VERSION,
        approval_id: correction.approval_id,
        command_id: correction.command_id,
        command_digest: correction.command_digest.clone(),
        target_entry_digest: correction.target_entry_digest.clone(),
        approver_id: correction.approver_id.clone(),
        approved_at_unix_ms: correction.approved_at_unix_ms,
        approval_digest: correction.approval_digest.clone(),
    };
    kiana_domain::CostCorrectionCommand {
        schema: kiana_domain::COST_CORRECTION_COMMAND_SCHEMA.to_owned(),
        version: kiana_domain::COST_LEDGER_VERSION,
        command_id: correction.command_id,
        correction_id: correction.correction_id,
        target_entry_id: correction.target_entry_id,
        target_entry_digest: correction.target_entry_digest.clone(),
        target_run_id: correction.target_run_id,
        target_attempt_id: correction.target_attempt_id,
        target_source_cursor: correction.target_source_cursor,
        target_revision: correction.target_revision,
        delta_estimated: correction.delta_estimated.clone(),
        delta_measured: correction.delta_measured.clone(),
        reason: correction.reason.clone(),
        evidence_refs: correction.evidence_refs.clone(),
        requested_by: correction.requested_by.clone(),
        idempotency_key: correction.idempotency_key.clone(),
        created_at_unix_ms: correction.created_at_unix_ms,
        approval: Some(approval),
        command_digest: correction.command_digest.clone(),
    }
}
