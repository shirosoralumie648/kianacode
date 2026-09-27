//! INT-29 connector restart/recovery and stale worker/lease fencing contract.
//!
//! Restart hydration is paused by default. Projection rebuild, Unknown reconciliation and a
//! fresh admission reference must all be visible before a connector can become eligible again;
//! this fact never resumes a worker or retries an external effect.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONNECTOR_RECOVERY_SCHEMA: &str = "kiana.connector-recovery.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRecoveryState {
    Paused,
    NeedsRecovery,
    Reconciled,
    ReadyForAdmission,
    Fenced,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRecoveryFact {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub account_id: String,
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub projection_generation: u64,
    pub source_generation: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub lease_epoch: u64,
    pub current_lease_epoch: u64,
    pub worker_epoch: u64,
    pub current_worker_epoch: u64,
    pub pending_unknown_count: u32,
    pub pending_invocation_count: u32,
    pub pending_reconciliation_count: u32,
    pub stale_worker_fenced: bool,
    pub old_lease_fenced: bool,
    pub default_paused: bool,
    pub source_digest: String,
    pub projection_digest: String,
    pub re_admission_refs: Vec<String>,
    pub state: ConnectorRecoveryState,
    pub recovery_digest: String,
}

impl ConnectorRecoveryFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CONNECTOR_RECOVERY_SCHEMA
            || !valid_text(&self.connector_id)
            || !valid_text(&self.binding_id)
            || !valid_text(&self.account_id)
            || self.source_cursor == 0
            || self.projection_cursor > self.source_cursor
            || self.projection_generation == 0
            || self.source_generation == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.lease_epoch == 0
            || self.current_lease_epoch == 0
            || self.worker_epoch == 0
            || self.current_worker_epoch == 0
            || !valid_digest(&self.source_digest)
            || !valid_digest(&self.projection_digest)
            || self.re_admission_refs.len() > 64
            || self
                .re_admission_refs
                .iter()
                .any(|value| !valid_text(value))
            || has_duplicates(&self.re_admission_refs)
            || !self.default_paused
            || !valid_digest(&self.recovery_digest)
            || self.recovery_digest != self.digest()
        {
            return Err("connector_recovery_fact_invalid");
        }
        if self.worker_epoch < self.current_worker_epoch && !self.stale_worker_fenced {
            return Err("connector_recovery_stale_worker_unfenced");
        }
        if self.lease_epoch < self.current_lease_epoch && !self.old_lease_fenced {
            return Err("connector_recovery_stale_lease_unfenced");
        }
        match self.state {
            ConnectorRecoveryState::Paused => {}
            ConnectorRecoveryState::NeedsRecovery => {
                if self.pending_unknown_count == 0 && self.pending_reconciliation_count == 0 {
                    return Err("connector_recovery_reconciliation_not_needed");
                }
            }
            ConnectorRecoveryState::Reconciled => {
                if self.pending_unknown_count != 0
                    || self.pending_reconciliation_count != 0
                    || self.re_admission_refs.is_empty()
                {
                    return Err("connector_recovery_reconciled_state_invalid");
                }
            }
            ConnectorRecoveryState::ReadyForAdmission => {
                if self.projection_cursor != self.source_cursor
                    || self.projection_generation != self.source_generation
                    || self.pending_unknown_count != 0
                    || self.pending_invocation_count != 0
                    || self.pending_reconciliation_count != 0
                    || !self.stale_worker_fenced
                    || !self.old_lease_fenced
                    || self.re_admission_refs.is_empty()
                {
                    return Err("connector_recovery_ready_gate_invalid");
                }
            }
            ConnectorRecoveryState::Fenced => {
                if !self.stale_worker_fenced || !self.old_lease_fenced {
                    return Err("connector_recovery_fence_required");
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
            "account_id": self.account_id,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "source_generation": self.source_generation,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "lease_epoch": self.lease_epoch,
            "current_lease_epoch": self.current_lease_epoch,
            "worker_epoch": self.worker_epoch,
            "current_worker_epoch": self.current_worker_epoch,
            "pending_unknown_count": self.pending_unknown_count,
            "pending_invocation_count": self.pending_invocation_count,
            "pending_reconciliation_count": self.pending_reconciliation_count,
            "stale_worker_fenced": self.stale_worker_fenced,
            "old_lease_fenced": self.old_lease_fenced,
            "default_paused": self.default_paused,
            "source_digest": self.source_digest,
            "projection_digest": self.projection_digest,
            "re_admission_refs": self.re_admission_refs,
            "state": self.state,
        }))
    }
}

pub fn validate_connector_recovery(fact: &ConnectorRecoveryFact) -> Result<(), &'static str> {
    fact.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.contains(['\0', '\r', '\n'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn has_duplicates(values: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value))
}
