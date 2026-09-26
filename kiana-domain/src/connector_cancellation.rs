//! INT-23 connector cancellation, stop settlement and late-result fencing.
//!
//! A stop acknowledgement is evidence about one attempt. It does not rewrite dispatch history,
//! release a lease after an effect may have started, or allow a provider result to resurrect a
//! cancelled attempt.

use crate::{
    json_digest, ConnectorDispatchLifecycle, ConnectorDispatchStage, InvocationId,
    ProcessGroupState, StopMethod, StopReport,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONNECTOR_CANCELLATION_SCHEMA: &str = "kiana.connector-cancellation-settlement.v1";
pub const CONNECTOR_LEASE_SETTLEMENT_SCHEMA: &str = "kiana.connector-lease-settlement.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCancellationState {
    NotExecuted,
    StopConfirmed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorLeaseSettlement {
    Released,
    HeldForReconciliation,
    Consumed,
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

impl ConnectorCancellationSettlement {
    pub fn from_stop_report(
        lifecycle: &ConnectorDispatchLifecycle,
        permit_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        report: &StopReport,
        requested_at_unix_ms: u64,
        settled_at_unix_ms: u64,
    ) -> Result<Self, String> {
        lifecycle.validate()?;
        report.validate()?;
        if requested_at_unix_ms == 0
            || settled_at_unix_ms < requested_at_unix_ms
            || report.execution_id != lifecycle.invocation_id.to_string()
        {
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
        let (state, lease_settlement) = if report.confirmed {
            match lifecycle.stage {
                ConnectorDispatchStage::Prepared => (
                    ConnectorCancellationState::NotExecuted,
                    ConnectorLeaseSettlement::Released,
                ),
                ConnectorDispatchStage::Dispatching => (
                    ConnectorCancellationState::StopConfirmed,
                    ConnectorLeaseSettlement::HeldForReconciliation,
                ),
                _ => return Err("connector_cancellation_stage_invalid".to_owned()),
            }
        } else {
            (
                ConnectorCancellationState::Unknown,
                ConnectorLeaseSettlement::HeldForReconciliation,
            )
        };
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
        match (self.state, self.lease_settlement) {
            (ConnectorCancellationState::NotExecuted, ConnectorLeaseSettlement::Released)
            | (
                ConnectorCancellationState::StopConfirmed,
                ConnectorLeaseSettlement::HeldForReconciliation,
            )
            | (
                ConnectorCancellationState::Unknown,
                ConnectorLeaseSettlement::HeldForReconciliation,
            ) => {}
            _ => return Err("connector_cancellation_lease_settlement_invalid".to_owned()),
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
