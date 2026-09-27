//! INT-32 per-operation controlled write connector pilot gate.
//!
//! This contract describes prerequisites for a separately approved write pilot. It never sends a
//! request or claims a business outcome. Approval, permit, idempotency, receipt, cancellation,
//! compensation and reconciliation evidence remain independently bound.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const CONNECTOR_WRITE_PILOT_SCHEMA: &str = "kiana.connector-write-pilot.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorWritePilotStatus {
    Blocked,
    ReadyForExplicitRun,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorWritePilotGate {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub isolated_account_id: String,
    pub operation: String,
    pub approval_ref: Option<String>,
    pub permit_digest: Option<String>,
    pub idempotency_policy_digest: String,
    pub final_payload_digest: String,
    pub provider_receipt_digest: Option<String>,
    pub cancellation_fence_digest: Option<String>,
    pub compensation_plan_digest: Option<String>,
    pub reconciliation_case_digest: Option<String>,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub request_count: u32,
    pub limitations: Vec<String>,
    pub status: ConnectorWritePilotStatus,
    pub gate_digest: String,
}

impl ConnectorWritePilotGate {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CONNECTOR_WRITE_PILOT_SCHEMA
            || !valid_text(&self.connector_id)
            || !valid_text(&self.binding_id)
            || !valid_text(&self.isolated_account_id)
            || !valid_text(&self.operation)
            || self.operation == "health"
            || !valid_digest(&self.idempotency_policy_digest)
            || !valid_digest(&self.final_payload_digest)
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.request_count > 1
            || self.limitations.is_empty()
            || self.limitations.iter().any(|value| !valid_text(value))
            || !valid_digest(&self.gate_digest)
            || self.gate_digest != self.digest()
        {
            return Err("connector_write_pilot_gate_invalid");
        }
        if self.isolated_account_id == self.connector_id {
            return Err("connector_write_pilot_isolated_account_required");
        }
        match self.status {
            ConnectorWritePilotStatus::Blocked => {}
            ConnectorWritePilotStatus::ReadyForExplicitRun => {
                let Some(approval) = self.approval_ref.as_deref() else {
                    return Err("connector_write_pilot_approval_required");
                };
                if !approval.starts_with("approval:") || approval.len() > 256 {
                    return Err("connector_write_pilot_approval_invalid");
                }
                for (value, error) in [
                    (
                        self.permit_digest.as_deref(),
                        "connector_write_pilot_permit_required",
                    ),
                    (
                        self.provider_receipt_digest.as_deref(),
                        "connector_write_pilot_receipt_required",
                    ),
                    (
                        self.cancellation_fence_digest.as_deref(),
                        "connector_write_pilot_cancellation_required",
                    ),
                    (
                        self.compensation_plan_digest.as_deref(),
                        "connector_write_pilot_compensation_required",
                    ),
                    (
                        self.reconciliation_case_digest.as_deref(),
                        "connector_write_pilot_reconciliation_required",
                    ),
                ] {
                    let Some(value) = value else {
                        return Err(error);
                    };
                    if !valid_digest(value) {
                        return Err("connector_write_pilot_evidence_digest_invalid");
                    }
                }
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "binding_id": self.binding_id,
            "isolated_account_id": self.isolated_account_id,
            "operation": self.operation,
            "approval_ref": self.approval_ref,
            "permit_digest": self.permit_digest,
            "idempotency_policy_digest": self.idempotency_policy_digest,
            "final_payload_digest": self.final_payload_digest,
            "provider_receipt_digest": self.provider_receipt_digest,
            "cancellation_fence_digest": self.cancellation_fence_digest,
            "compensation_plan_digest": self.compensation_plan_digest,
            "reconciliation_case_digest": self.reconciliation_case_digest,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "request_count": self.request_count,
            "limitations": self.limitations,
            "status": self.status,
        }))
    }
}

pub fn validate_connector_write_pilot(gate: &ConnectorWritePilotGate) -> Result<(), &'static str> {
    gate.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\r', '\n'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
