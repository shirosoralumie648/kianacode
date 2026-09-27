//! BQ-21 billing restart/recovery and unknown-attempt fencing source contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const BILLING_RECOVERY_SCHEMA: &str = "kiana.billing-recovery.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingRecoveryState {
    Paused,
    NeedsReconciliation,
    Recovered,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingRecoveryFact {
    pub schema: String,
    pub attempt_id: String,
    pub reservation_digest: String,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub lease_epoch: u64,
    pub state: BillingRecoveryState,
    pub usage_unknown: bool,
    pub continue_resets_usage: bool,
    pub settlement_count: u8,
    pub reconciliation_ref: Option<String>,
    pub recovery_digest: String,
}

impl BillingRecoveryFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BILLING_RECOVERY_SCHEMA
            || !valid_text(&self.attempt_id)
            || !valid_digest(&self.reservation_digest)
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.lease_epoch == 0
            || self.continue_resets_usage
            || self.settlement_count > 1
            || self
                .reconciliation_ref
                .as_deref()
                .is_some_and(|v| !valid_text(v))
            || matches!(self.state, BillingRecoveryState::Recovered)
                && (self.usage_unknown
                    || self.reconciliation_ref.is_none()
                    || self.settlement_count != 1)
            || matches!(self.state, BillingRecoveryState::NeedsReconciliation)
                && (!self.usage_unknown
                    || self.reconciliation_ref.is_none()
                    || self.settlement_count != 0)
            || matches!(self.state, BillingRecoveryState::Paused) && self.settlement_count != 0
            || !valid_digest(&self.recovery_digest)
            || self.recovery_digest != self.digest()
        {
            return Err("billing_recovery_fact_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "attempt_id": self.attempt_id,
            "reservation_digest": self.reservation_digest,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "lease_epoch": self.lease_epoch,
            "state": self.state,
            "usage_unknown": self.usage_unknown,
            "continue_resets_usage": self.continue_resets_usage,
            "settlement_count": self.settlement_count,
            "reconciliation_ref": self.reconciliation_ref,
        }))
    }
}

pub fn validate_billing_recovery(fact: &BillingRecoveryFact) -> Result<(), &'static str> {
    fact.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\n', '\r'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
