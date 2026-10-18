//! INT-23 connector cancellation, stop settlement and late-result fencing.
//!
//! A stop acknowledgement is evidence about one attempt. It does not rewrite dispatch history,
//! release a lease after an effect may have started, or allow a provider result to resurrect a
//! cancelled attempt.
//!
//! 2026-10-17 completion slice (CI-only): the settlement is reconstructible from server-owned stop
//! evidence. `expected_settlement` is the single derivation shared by `from_stop_report` and
//! `validate_for_stop_report`, so an unconfirmed stop can never be recorded as `NotExecuted` or
//! `StopConfirmed`, a started effect can never release its lease, and the typed `ConnectorLateResult`
//! envelope is fenced for every terminal settlement with invocation/attempt/time binding.
//!
//! The completion slice adds the durable [`ConnectorCancellationRecord`]: a cancellation has no
//! constructor without a [`StopReport`], `reconstruct_outcome` re-derives the terminal
//! `not_executed` / `stop_confirmed` / `unknown` state from the lifecycle and that report instead of
//! trusting the stored settlement, and a late provider result can only be recorded as
//! [`ConnectorLateResultDisposition::Fenced`] evidence — it can never move the terminal state, the
//! lease settlement or the settlement digest.

use crate::{
    json_digest, ConnectorDispatchLifecycle, ConnectorDispatchStage, InvocationId,
    ProcessGroupState, StopMethod, StopReport,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONNECTOR_CANCELLATION_SCHEMA: &str = "kiana.connector-cancellation-settlement.v1";
pub const CONNECTOR_LEASE_SETTLEMENT_SCHEMA: &str = "kiana.connector-lease-settlement.v1";
pub const CONNECTOR_LATE_RESULT_SCHEMA: &str = "kiana.connector-late-result.v1";
pub const CONNECTOR_CANCELLATION_RECORD_SCHEMA: &str = "kiana.connector-cancellation-record.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCancellationState {
    NotExecuted,
    StopConfirmed,
    Unknown,
}

impl ConnectorCancellationState {
    /// The canonical lease settlement of every cancellation outcome.
    ///
    /// Only a stop confirmed *before* the effect started proves the lease was never used and may be
    /// released. A confirmed stop of a started effect and an unconfirmed stop both keep the lease
    /// held for reconciliation. `Consumed` is reserved for a committed dispatch result and is never
    /// a cancellation outcome.
    pub const fn lease_settlement(self) -> ConnectorLeaseSettlement {
        match self {
            Self::NotExecuted => ConnectorLeaseSettlement::Released,
            Self::StopConfirmed | Self::Unknown => ConnectorLeaseSettlement::HeldForReconciliation,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorLeaseSettlement {
    Released,
    HeldForReconciliation,
    Consumed,
}

impl ConnectorLeaseSettlement {
    /// Only `Released` gives the lease back.
    pub const fn releases_lease(self) -> bool {
        matches!(self, Self::Released)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCancellationSettlement {
    pub schema: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub command_digest: String,
    pub permit_digest: String,
    pub lease_digest: String,
    pub stop_report_digest: String,
    pub state: ConnectorCancellationState,
    pub lease_settlement: ConnectorLeaseSettlement,
    pub late_result_fenced: bool,
    pub requested_at_unix_ms: u64,
    pub settled_at_unix_ms: u64,
    pub settlement_digest: String,
}

/// A provider result that surfaced after the attempt was settled.
///
/// The envelope is bound to the settled attempt and is rejected for every terminal settlement, so a
/// late result can be recorded as evidence without ever resurrecting the attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorLateResult {
    pub schema: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub result_digest: String,
    pub observed_at_unix_ms: u64,
}

impl ConnectorLateResult {
    pub fn new(
        invocation_id: InvocationId,
        attempt: u32,
        result_digest: impl Into<String>,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let result = Self {
            schema: CONNECTOR_LATE_RESULT_SCHEMA.to_owned(),
            invocation_id,
            attempt,
            result_digest: result_digest.into(),
            observed_at_unix_ms,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_LATE_RESULT_SCHEMA
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || !valid_digest(&self.result_digest)
            || self.observed_at_unix_ms == 0
        {
            return Err("connector_late_result_invalid".to_owned());
        }
        Ok(())
    }
}

/// The only possible disposition of a late provider result: fenced evidence.
///
/// A terminal settlement has exactly one answer to "may this result move the attempt?".  Making
/// any other answer unrepresentable is stronger than rejecting it at runtime, so the enum carries
/// a single variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorLateResultDisposition {
    Fenced,
}

/// The durable cancellation record of one connector attempt.
///
/// A cancellation cannot be recorded without the stop evidence that produced it: the record carries
/// the [`StopReport`], and every validation re-derives the settlement from the lifecycle and that
/// report.  A started effect keeps its lease held unless a confirmed stop was recorded before any
/// effect could start, and every late provider result is retained only as fenced evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCancellationRecord {
    pub schema: String,
    pub settlement: ConnectorCancellationSettlement,
    pub stop_report: StopReport,
    pub fenced_late_results: Vec<ConnectorLateResult>,
    pub record_digest: String,
}

impl ConnectorCancellationRecord {
    /// Build the record from the server-owned stop evidence of one attempt.
    ///
    /// There is no constructor that takes a lifecycle alone or a caller-declared cancellation
    /// state, so "cancelled without a stop" cannot be recorded: an unconfirmed report settles as
    /// `Unknown` with the lease held, and a started effect can never release its lease.
    pub fn from_stop_report(
        lifecycle: &ConnectorDispatchLifecycle,
        permit_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        report: &StopReport,
        requested_at_unix_ms: u64,
        settled_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let settlement = ConnectorCancellationSettlement::from_stop_report(
            lifecycle,
            permit_digest,
            lease_digest,
            report,
            requested_at_unix_ms,
            settled_at_unix_ms,
        )?;
        let mut record = Self {
            schema: CONNECTOR_CANCELLATION_RECORD_SCHEMA.to_owned(),
            settlement,
            stop_report: report.clone(),
            fenced_late_results: Vec::new(),
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    /// Reconstruct the terminal outcome of the settled attempt from server-owned evidence.
    ///
    /// This is the single reconstruction entry point: the lifecycle and the stored stop report are
    /// re-read, the outcome is re-derived from them, and the stored settlement must be exactly that
    /// outcome, so the returned state and lease settlement are reconstructed rather than remembered.
    /// A record that drifted from its attempt, claimed a stop that the report does not confirm, or
    /// released the lease of a started effect is refused here.
    pub fn reconstruct_outcome(
        &self,
        lifecycle: &ConnectorDispatchLifecycle,
    ) -> Result<(ConnectorCancellationState, ConnectorLeaseSettlement), String> {
        self.validate_for_lifecycle(lifecycle)?;
        let outcome = expected_settlement(lifecycle, &self.stop_report)?;
        if self.settlement.state != outcome.0 || self.settlement.lease_settlement != outcome.1 {
            return Err("connector_cancellation_settlement_mismatch".to_owned());
        }
        Ok(outcome)
    }

    /// Validate the record against the lifecycle it claims to settle.
    pub fn validate_for_lifecycle(
        &self,
        lifecycle: &ConnectorDispatchLifecycle,
    ) -> Result<(), String> {
        self.validate()?;
        self.settlement.validate_for_lifecycle(lifecycle)?;
        if self.stop_report.execution_id != lifecycle.invocation_id.to_string() {
            return Err("connector_cancellation_stop_report_mismatch".to_owned());
        }
        Ok(())
    }

    /// Record a late provider result as fenced evidence.
    ///
    /// The result must be addressed to this settled attempt and must have arrived after the
    /// settlement; anything else is a structured refusal.  The only disposition is `Fenced`, so
    /// recording keeps evidence and never moves the terminal state, the lease settlement or the
    /// settlement digest.
    pub fn record_late_result(
        &mut self,
        result: ConnectorLateResult,
    ) -> Result<ConnectorLateResultDisposition, String> {
        self.validate()?;
        let disposition = self.settlement.late_result_disposition(&result)?;
        self.fenced_late_results.push(result);
        self.record_digest = self.digest();
        self.validate()?;
        Ok(disposition)
    }

    /// A late result that is not addressed to this settled attempt or arrived before it was
    /// settled cannot even be kept as fenced evidence.
    fn validate_fenced_late_results(
        &self,
        invocation_id: InvocationId,
        attempt: u32,
        settled_at_unix_ms: u64,
    ) -> Result<(), String> {
        for result in &self.fenced_late_results {
            result.validate()?;
            if result.invocation_id != invocation_id {
                return Err("connector_cancellation_late_result_invocation_mismatch".to_owned());
            }
            if result.attempt != attempt {
                return Err("connector_cancellation_late_result_attempt_mismatch".to_owned());
            }
            if result.observed_at_unix_ms < settled_at_unix_ms {
                return Err("connector_cancellation_late_result_not_late".to_owned());
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_CANCELLATION_RECORD_SCHEMA || !valid_digest(&self.record_digest)
        {
            return Err("connector_cancellation_record_invalid".to_owned());
        }
        self.settlement.validate()?;
        self.stop_report.validate()?;
        if self.settlement.stop_report_digest != self.stop_report.report_digest
            || self.settlement.invocation_id.to_string() != self.stop_report.execution_id
        {
            return Err("connector_cancellation_stop_report_mismatch".to_owned());
        }
        self.validate_fenced_late_results(
            self.settlement.invocation_id,
            self.settlement.attempt,
            self.settlement.settled_at_unix_ms,
        )?;
        if self.record_digest != self.digest() {
            return Err("connector_cancellation_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "settlement": &self.settlement,
            "stop_report": &self.stop_report,
            "fenced_late_results": &self.fenced_late_results,
        }))
    }
}

impl ConnectorCancellationSettlement {
    pub fn from_stop_report(
        lifecycle: &ConnectorDispatchLifecycle,
        permit_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        report: &StopReport,
        requested_at_unix_ms: u64,
        settled_at_unix_ms: u64,
    ) -> Result<Self, String> {
        if requested_at_unix_ms == 0 || settled_at_unix_ms < requested_at_unix_ms {
            return Err("connector_cancellation_identity_or_time_invalid".to_owned());
        }
        let (state, lease_settlement) = expected_settlement(lifecycle, report)?;
        let mut settlement = Self {
            schema: CONNECTOR_CANCELLATION_SCHEMA.to_owned(),
            invocation_id: lifecycle.invocation_id,
            attempt: lifecycle.attempt,
            command_digest: lifecycle.command_digest.clone(),
            permit_digest: permit_digest.into(),
            lease_digest: lease_digest.into(),
            stop_report_digest: report.report_digest.clone(),
            state,
            lease_settlement,
            late_result_fenced: true,
            requested_at_unix_ms,
            settled_at_unix_ms,
            settlement_digest: String::new(),
        };
        settlement.settlement_digest = settlement.digest();
        settlement.validate()?;
        Ok(settlement)
    }

    /// Re-derive the settlement identity against the lifecycle it claims to settle.
    ///
    /// This refuses a settlement that drifted from the attempt (invocation/attempt/command digest),
    /// a settlement of an attempt that already produced an effect, and a release of a lease whose
    /// effect may have started. It does not require the stop report, so it is the cheaper check used
    /// before the report is re-read.
    pub fn validate_for_lifecycle(
        &self,
        lifecycle: &ConnectorDispatchLifecycle,
    ) -> Result<(), String> {
        self.validate()?;
        lifecycle.validate()?;
        if self.invocation_id != lifecycle.invocation_id
            || self.attempt != lifecycle.attempt
            || self.command_digest != lifecycle.command_digest
        {
            return Err("connector_cancellation_lifecycle_mismatch".to_owned());
        }
        match lifecycle.stage {
            ConnectorDispatchStage::Prepared => {
                if self.state == ConnectorCancellationState::StopConfirmed {
                    return Err("connector_cancellation_started_effect_required".to_owned());
                }
            }
            ConnectorDispatchStage::Dispatching => {
                if self.lease_settlement.releases_lease() {
                    return Err(
                        "connector_cancellation_started_effect_cannot_release_lease".to_owned()
                    );
                }
                if self.state == ConnectorCancellationState::NotExecuted {
                    return Err("connector_cancellation_settlement_mismatch".to_owned());
                }
            }
            ConnectorDispatchStage::Observed
            | ConnectorDispatchStage::ResultCommitted
            | ConnectorDispatchStage::Unknown => {
                return Err("connector_cancellation_effect_already_observed".to_owned());
            }
        }
        Ok(())
    }

    /// Re-derive the exact outcome from the stop report that backs this settlement.
    ///
    /// An unconfirmed report can only settle as `Unknown` with the lease held, a confirmed stop
    /// before dispatch as `NotExecuted` released, and a confirmed stop of a started effect as
    /// `StopConfirmed` held. A hand-written settlement (or a deserialized replay) that claims any
    /// other combination is refused even when its digests were recomputed.
    pub fn validate_for_stop_report(
        &self,
        lifecycle: &ConnectorDispatchLifecycle,
        report: &StopReport,
    ) -> Result<(), String> {
        self.validate_for_lifecycle(lifecycle)?;
        report.validate()?;
        if self.stop_report_digest != report.report_digest {
            return Err("connector_cancellation_stop_report_mismatch".to_owned());
        }
        let (state, lease_settlement) = expected_settlement(lifecycle, report)?;
        if self.state != state || self.lease_settlement != lease_settlement {
            return Err("connector_cancellation_settlement_mismatch".to_owned());
        }
        Ok(())
    }

    /// Refuse a late provider result addressed to this attempt.
    ///
    /// `Err` is the fenced answer: the result may be recorded as evidence, but it can never move the
    /// terminal settlement. Identity and arrival bind the envelope to this attempt before the fence
    /// verdict is returned.
    pub fn fence_late_result(&self, result: &ConnectorLateResult) -> Result<(), String> {
        self.late_result_disposition(result)?;
        Err("connector_cancellation_late_result_fenced".to_owned())
    }

    /// The disposition of a late provider result addressed to this attempt.
    ///
    /// A result that is not addressed to this settled attempt (invocation, attempt, arrival time)
    /// is refused with a structured reason; every result that is addressed to it is `Fenced`.
    /// There is no accepted answer, so accepting a recorded late result can never resurrect the
    /// attempt or release its lease.
    pub fn late_result_disposition(
        &self,
        result: &ConnectorLateResult,
    ) -> Result<ConnectorLateResultDisposition, String> {
        self.validate()?;
        result.validate()?;
        if result.invocation_id != self.invocation_id {
            return Err("connector_cancellation_late_result_invocation_mismatch".to_owned());
        }
        if result.attempt != self.attempt {
            return Err("connector_cancellation_late_result_attempt_mismatch".to_owned());
        }
        if result.observed_at_unix_ms < self.settled_at_unix_ms {
            return Err("connector_cancellation_late_result_not_late".to_owned());
        }
        if !self.late_result_fenced {
            return Err("connector_cancellation_settlement_invalid".to_owned());
        }
        Ok(ConnectorLateResultDisposition::Fenced)
    }

    pub fn reject_late_result(&self, result_digest: &str) -> Result<(), String> {
        self.validate()?;
        if !valid_digest(result_digest) {
            return Err("connector_cancellation_result_digest_invalid".to_owned());
        }
        if self.late_result_fenced {
            return Err("connector_cancellation_late_result_fenced".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_CANCELLATION_SCHEMA
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || !valid_digest(&self.command_digest)
            || !valid_digest(&self.permit_digest)
            || !valid_digest(&self.lease_digest)
            || !valid_digest(&self.stop_report_digest)
            || !valid_digest(&self.settlement_digest)
            || !self.late_result_fenced
            || self.requested_at_unix_ms == 0
            || self.settled_at_unix_ms < self.requested_at_unix_ms
            || self.settlement_digest != self.digest()
        {
            return Err("connector_cancellation_settlement_invalid".to_owned());
        }
        if self.lease_settlement != self.state.lease_settlement() {
            return Err("connector_cancellation_lease_settlement_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "command_digest": self.command_digest,
            "permit_digest": self.permit_digest,
            "lease_digest": self.lease_digest,
            "stop_report_digest": self.stop_report_digest,
            "state": self.state,
            "lease_settlement": self.lease_settlement,
            "late_result_fenced": self.late_result_fenced,
            "requested_at_unix_ms": self.requested_at_unix_ms,
            "settled_at_unix_ms": self.settled_at_unix_ms,
        }))
    }
}

/// The single server-owned derivation of a cancellation outcome.
fn expected_settlement(
    lifecycle: &ConnectorDispatchLifecycle,
    report: &StopReport,
) -> Result<(ConnectorCancellationState, ConnectorLeaseSettlement), String> {
    lifecycle.validate()?;
    report.validate()?;
    if report.execution_id != lifecycle.invocation_id.to_string() {
        return Err("connector_cancellation_identity_or_time_invalid".to_owned());
    }
    if matches!(
        lifecycle.stage,
        ConnectorDispatchStage::Observed
            | ConnectorDispatchStage::ResultCommitted
            | ConnectorDispatchStage::Unknown
    ) {
        return Err("connector_cancellation_effect_already_observed".to_owned());
    }
    if !report.confirmed {
        return Ok((
            ConnectorCancellationState::Unknown,
            ConnectorLeaseSettlement::HeldForReconciliation,
        ));
    }
    match lifecycle.stage {
        ConnectorDispatchStage::Prepared => Ok((
            ConnectorCancellationState::NotExecuted,
            ConnectorLeaseSettlement::Released,
        )),
        ConnectorDispatchStage::Dispatching => {
            // A started effect can only be called stopped when a stop signal was actually
            // recorded.  A "confirmed" report without any stop attempt is incomplete evidence:
            // claiming a stop that never happened is refused instead of releasing the lease.
            if !(report.term_sent || report.kill_sent) {
                return Err("connector_cancellation_stop_attempt_missing".to_owned());
            }
            Ok((
                ConnectorCancellationState::StopConfirmed,
                ConnectorLeaseSettlement::HeldForReconciliation,
            ))
        }
        ConnectorDispatchStage::Observed
        | ConnectorDispatchStage::ResultCommitted
        | ConnectorDispatchStage::Unknown => {
            Err("connector_cancellation_effect_already_observed".to_owned())
        }
    }
}

/// A bounded constructor used by source fixtures to make the stop evidence explicit. Real
/// adapters use `StopReport::new`; these constants document the no-process stop shape.
pub fn connector_not_executed_stop_report(
    invocation_id: InvocationId,
) -> Result<StopReport, String> {
    StopReport::new(
        invocation_id.to_string(),
        None,
        None,
        StopMethod::None,
        false,
        false,
        true,
        ProcessGroupState::Empty,
        true,
        0,
    )
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
