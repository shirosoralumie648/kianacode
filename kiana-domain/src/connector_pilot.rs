//! INT-31 default-off read-only external connector pilot gate.
//!
//! This is an admission/evidence contract only. It never opens a network connection or invokes a
//! connector. A pilot is eligible only with an isolated account, explicit operator approval,
//! endpoint/credential/receipt/revocation/cleanup evidence and a read-only operation.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const CONNECTOR_PILOT_SCHEMA: &str = "kiana.connector-pilot.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorPilotMode {
    DefaultOff,
    OptedIn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorPilotEvidenceSource {
    Fake,
    LiveNetwork,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorPilotStatus {
    Blocked,
    ReadyForOperator,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorPilotGate {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub isolated_account_id: String,
    pub operation: String,
    pub endpoint_digest: String,
    pub credential_revision_digest: String,
    pub scope_digest: String,
    pub data_epoch: u64,
    pub revocation_epoch: u64,
    pub mode: ConnectorPilotMode,
    pub source: ConnectorPilotEvidenceSource,
    pub operator_approval_ref: Option<String>,
    pub provider_receipt_digest: Option<String>,
    pub cleanup_plan_digest: Option<String>,
    pub limitations: Vec<String>,
    pub status: ConnectorPilotStatus,
    pub gate_digest: String,
}

impl ConnectorPilotGate {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CONNECTOR_PILOT_SCHEMA
            || !valid_text(&self.connector_id)
            || !valid_text(&self.binding_id)
            || !valid_text(&self.isolated_account_id)
            || !valid_text(&self.operation)
            || !matches!(self.operation.as_str(), "health" | "list" | "whoami")
            || !valid_digest(&self.endpoint_digest)
            || !valid_digest(&self.credential_revision_digest)
            || !valid_digest(&self.scope_digest)
            || self.data_epoch == 0
            || self.revocation_epoch == 0
            || self.limitations.iter().any(|value| !valid_text(value))
            || self.limitations.is_empty()
            || !valid_digest(&self.gate_digest)
            || self.gate_digest != self.digest()
        {
            return Err("connector_pilot_gate_invalid");
        }
        if self.isolated_account_id == self.connector_id {
            return Err("connector_pilot_isolated_account_required");
        }
        match self.status {
            ConnectorPilotStatus::Blocked => {
                if self.mode == ConnectorPilotMode::OptedIn
                    && self.source == ConnectorPilotEvidenceSource::LiveNetwork
                    && self.operator_approval_ref.is_some()
                {
                    return Err("connector_pilot_blocked_gate_inconsistent");
                }
            }
            ConnectorPilotStatus::ReadyForOperator => {
                if self.mode != ConnectorPilotMode::OptedIn
                    || self.source != ConnectorPilotEvidenceSource::LiveNetwork
                {
                    return Err("connector_pilot_live_opt_in_required");
                }
                let Some(approval) = self.operator_approval_ref.as_deref() else {
                    return Err("connector_pilot_approval_required");
                };
                if !approval.starts_with("approval:") || approval.len() > 256 {
                    return Err("connector_pilot_approval_invalid");
                }
                if self.provider_receipt_digest.is_none() {
                    return Err("connector_pilot_provider_receipt_required");
                }
                if self.cleanup_plan_digest.is_none() {
                    return Err("connector_pilot_cleanup_required");
                }
                if self
                    .provider_receipt_digest
                    .as_deref()
                    .is_none_or(|value| !valid_digest(value))
                    || self
                        .cleanup_plan_digest
                        .as_deref()
                        .is_none_or(|value| !valid_digest(value))
                {
                    return Err("connector_pilot_evidence_digest_invalid");
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
            "endpoint_digest": self.endpoint_digest,
            "credential_revision_digest": self.credential_revision_digest,
            "scope_digest": self.scope_digest,
            "data_epoch": self.data_epoch,
            "revocation_epoch": self.revocation_epoch,
            "mode": self.mode,
            "source": self.source,
            "operator_approval_ref": self.operator_approval_ref,
            "provider_receipt_digest": self.provider_receipt_digest,
            "cleanup_plan_digest": self.cleanup_plan_digest,
            "limitations": self.limitations,
            "status": self.status,
        }))
    }
}

pub fn validate_connector_pilot(gate: &ConnectorPilotGate) -> Result<(), &'static str> {
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
