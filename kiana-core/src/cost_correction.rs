//! ControlPlane admission helpers for BQ-14 cost corrections.
//!
//! The helpers below validate a typed command against the committed target and build an
//! append-only RuntimeEvent payload. They do not mutate a ledger, call a provider, or dispatch a
//! capability. The caller must append the returned event through the existing EventStore CAS
//! transaction and then project a Receipt/query view from committed facts.

use kiana_domain::{
    ApprovalDecision, ApprovalDecisionRecord, CostCorrection, CostCorrectionCommand,
    CostLedgerEntry, RuntimeEvent, COST_CORRECTION_EVENT, COST_LEDGER_ENTRY_EVENT,
};
use serde_json::Value;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CostCorrectionAdmissionError {
    #[error("cost_correction_target_invalid:{0}")]
    TargetInvalid(String),
    #[error("cost_correction_command_invalid:{0}")]
    CommandInvalid(String),
    #[error("cost_correction_event_invalid:{0}")]
    EventInvalid(String),
}

/// Server-side command admission. The model/runner never receives this helper and there is no
/// boolean "approved" shortcut: a command must carry an exact CostCorrectionApproval subject.
pub struct CostCorrectionAdmission;

impl CostCorrectionAdmission {
    pub fn authorize(
        target: &CostLedgerEntry,
        command: &CostCorrectionCommand,
    ) -> Result<CostCorrection, CostCorrectionAdmissionError> {
        target
            .validate()
            .map_err(CostCorrectionAdmissionError::TargetInvalid)?;
        command
            .validate_draft_against(target)
            .map_err(CostCorrectionAdmissionError::CommandInvalid)?;
        command
            .validate_approved()
            .map_err(CostCorrectionAdmissionError::CommandInvalid)?;
        CostCorrection::from_command(command, target)
            .map_err(CostCorrectionAdmissionError::CommandInvalid)
    }

    /// Bind the correction subject to the existing ApprovalStore projection. A standalone
    /// `approved=true` value or model response is never accepted as authority.
    pub fn authorize_with_approval_record(
        target: &CostLedgerEntry,
        command: &CostCorrectionCommand,
        decision: &ApprovalDecisionRecord,
    ) -> Result<CostCorrection, CostCorrectionAdmissionError> {
        let requested = command.approval.as_ref().ok_or_else(|| {
            CostCorrectionAdmissionError::CommandInvalid(
                "cost_correction_approval_required".to_owned(),
            )
        })?;
        if decision.approval_id != requested.approval_id
            || decision.decision != Some(ApprovalDecision::Approve)
            || decision.decision_command_id != Some(command.command_id)
            || decision.challenge.request_hash.as_str() != command.command_digest.as_str()
            || decision.decided_by.as_deref() != Some(requested.approver_id.as_str())
        {
            return Err(CostCorrectionAdmissionError::CommandInvalid(
                "cost_correction_approval_record_mismatch".to_owned(),
            ));
        }
        Self::authorize(target, command)
    }

    /// Build an immutable original ledger fact event. The event is not appended here; callers
    /// retain the existing DaemonHost → ControlPlane → EventStore path.
    pub fn ledger_entry_event(
        request_id: kiana_domain::RequestId,
        sequence: u64,
        entry: &CostLedgerEntry,
    ) -> Result<RuntimeEvent, CostCorrectionAdmissionError> {
        entry
            .validate()
            .map_err(CostCorrectionAdmissionError::TargetInvalid)?;
        let data = serde_json::to_value(entry)
            .map_err(|_| CostCorrectionAdmissionError::EventInvalid("encode_failed".to_owned()))?;
        RuntimeEvent::new(request_id, sequence, COST_LEDGER_ENTRY_EVENT, data)
            .map(|event| {
                event.with_stream_metadata(
                    "cost_ledger",
                    entry.entry_id.to_string(),
                    entry.revision,
                )
            })
            .map_err(|error| CostCorrectionAdmissionError::EventInvalid(error.to_string()))
    }

    /// Build a correction event after approval admission. No update/delete operation exists;
    /// every correction is a new event on the ledger stream.
    pub fn correction_event(
        request_id: kiana_domain::RequestId,
        sequence: u64,
        correction: &CostCorrection,
        stream_version: u64,
    ) -> Result<RuntimeEvent, CostCorrectionAdmissionError> {
        correction
            .validate()
            .map_err(CostCorrectionAdmissionError::CommandInvalid)?;
        if stream_version == 0 {
            return Err(CostCorrectionAdmissionError::EventInvalid(
                "stream_version_invalid".to_owned(),
            ));
        }
        let data = serde_json::to_value(correction)
            .map_err(|_| CostCorrectionAdmissionError::EventInvalid("encode_failed".to_owned()))?;
        RuntimeEvent::new(request_id, sequence, COST_CORRECTION_EVENT, data)
            .map(|event| {
                event.with_stream_metadata(
                    "cost_ledger",
                    correction.target_entry_id.to_string(),
                    stream_version,
                )
            })
            .map_err(|error| CostCorrectionAdmissionError::EventInvalid(error.to_string()))
    }

    /// The payload is deliberately typed and contains no transcript/model text. This helper is
    /// useful to source guards and adapters that need to inspect the command without executing it.
    pub fn command_payload(
        command: &CostCorrectionCommand,
    ) -> Result<Value, CostCorrectionAdmissionError> {
        command
            .validate_approved()
            .map_err(CostCorrectionAdmissionError::CommandInvalid)?;
        serde_json::to_value(command)
            .map_err(|_| CostCorrectionAdmissionError::EventInvalid("encode_failed".to_owned()))
    }
}

impl super::ControlPlane {
    /// Keep correction admission on the existing ControlPlane surface. This method is pure and
    /// intentionally does not append or dispatch; a higher-level command transaction must do so.
    pub fn admit_cost_correction(
        &self,
        target: &CostLedgerEntry,
        command: &CostCorrectionCommand,
    ) -> Result<CostCorrection, CostCorrectionAdmissionError> {
        CostCorrectionAdmission::authorize(target, command)
    }

    pub fn admit_cost_correction_with_approval(
        &self,
        target: &CostLedgerEntry,
        command: &CostCorrectionCommand,
        decision: &kiana_domain::ApprovalDecisionRecord,
    ) -> Result<CostCorrection, CostCorrectionAdmissionError> {
        CostCorrectionAdmission::authorize_with_approval_record(target, command, decision)
    }
}
