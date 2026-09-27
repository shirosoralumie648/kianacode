//! AUT-21 boot recovery, projection rebuild and reconciliation source contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const AUTOMATION_BOOT_RECOVERY_SCHEMA: &str = "kiana.automation-boot-recovery.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationBootRecoveryState {
    Rebuilding,
    ReconcileRequired,
    Ready,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationBootRecoveryFact {
    pub schema: String,
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub projection_generation: u64,
    pub authority_epoch: u64,
    pub pending_unknown_count: u32,
    pub stale_index: bool,
    pub state: AutomationBootRecoveryState,
    pub reconcile_refs: Vec<String>,
    pub recovery_digest: String,
}

impl AutomationBootRecoveryFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_BOOT_RECOVERY_SCHEMA
            || self.source_cursor == 0
            || self.projection_cursor > self.source_cursor
            || self.projection_generation == 0
            || self.authority_epoch == 0
            || self.reconcile_refs.iter().any(|v| v.trim().is_empty())
            || matches!(self.state, AutomationBootRecoveryState::Ready)
                && (self.pending_unknown_count > 0 || self.stale_index)
            || matches!(self.state, AutomationBootRecoveryState::ReconcileRequired)
                && self.pending_unknown_count == 0
                && !self.stale_index
            || !valid_digest(&self.recovery_digest)
            || self.recovery_digest != self.digest()
        {
            return Err("automation_boot_recovery_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "authority_epoch": self.authority_epoch,
            "pending_unknown_count": self.pending_unknown_count,
            "stale_index": self.stale_index,
            "state": self.state,
            "reconcile_refs": self.reconcile_refs,
        }))
    }
}

pub fn validate_automation_boot_recovery(
    fact: &AutomationBootRecoveryFact,
) -> Result<(), &'static str> {
    fact.validate()
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
