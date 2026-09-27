//! AUT-14 effect reservation and permit fence source contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const AUTOMATION_RESERVATION_SCHEMA: &str = "kiana.automation-effect-reservation.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationReservationState {
    Reserved,
    Committed,
    Consumed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationEffectReservation {
    pub schema: String,
    pub reservation_id: String,
    pub idempotency_key: String,
    pub execution_id: String,
    pub action_digest: String,
    pub capability: String,
    pub authority_epoch: u64,
    pub config_revision: String,
    pub policy_revision: String,
    pub budget_digest: String,
    pub path_scope_digest: String,
    pub approval_digest: Option<String>,
    pub state: AutomationReservationState,
    pub reservation_digest: String,
}

impl AutomationEffectReservation {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_RESERVATION_SCHEMA
            || !valid_text(&self.reservation_id)
            || !valid_text(&self.idempotency_key)
            || !valid_text(&self.execution_id)
            || !valid_digest(&self.action_digest)
            || !valid_text(&self.capability)
            || self.authority_epoch == 0
            || !valid_text(&self.config_revision)
            || !valid_text(&self.policy_revision)
            || !valid_digest(&self.budget_digest)
            || !valid_digest(&self.path_scope_digest)
            || self
                .approval_digest
                .as_deref()
                .is_some_and(|v| !valid_digest(v))
            || !valid_digest(&self.reservation_digest)
            || self.reservation_digest != self.digest()
        {
            return Err("automation_effect_reservation_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "reservation_id": self.reservation_id,
            "idempotency_key": self.idempotency_key,
            "execution_id": self.execution_id,
            "action_digest": self.action_digest,
            "capability": self.capability,
            "authority_epoch": self.authority_epoch,
            "config_revision": self.config_revision,
            "policy_revision": self.policy_revision,
            "budget_digest": self.budget_digest,
            "path_scope_digest": self.path_scope_digest,
            "approval_digest": self.approval_digest,
            "state": self.state,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationReservationLedger {
    pub authority_epoch: u64,
    pub config_revision: String,
    pub policy_revision: String,
    entries: BTreeMap<String, AutomationEffectReservation>,
}

impl AutomationReservationLedger {
    pub fn new(authority_epoch: u64, config_revision: String, policy_revision: String) -> Self {
        Self {
            authority_epoch,
            config_revision,
            policy_revision,
            entries: BTreeMap::new(),
        }
    }

    pub fn reserve(
        &mut self,
        mut reservation: AutomationEffectReservation,
    ) -> Result<AutomationEffectReservation, &'static str> {
        reservation.validate()?;
        if reservation.authority_epoch != self.authority_epoch
            || reservation.config_revision != self.config_revision
            || reservation.policy_revision != self.policy_revision
        {
            return Err("automation_effect_reservation_fence_drift");
        }
        if let Some(existing) = self.entries.get(&reservation.idempotency_key) {
            if existing.reservation_digest == reservation.reservation_digest {
                return Ok(existing.clone());
            }
            return Err("automation_effect_reservation_idempotency_conflict");
        }
        reservation.state = AutomationReservationState::Reserved;
        reservation.reservation_digest = reservation.digest();
        reservation.validate()?;
        self.entries
            .insert(reservation.idempotency_key.clone(), reservation.clone());
        Ok(reservation)
    }
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
