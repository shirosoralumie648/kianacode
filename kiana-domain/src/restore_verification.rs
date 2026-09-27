//! PD-23 restore verification and replacement-fence source contract.
//!
//! A verified backup manifest is still inert until a new instance proves that its facts,
//! projectors, ACL/epoch and external-effect reconciliation agree. This value contract keeps the
//! old instance fenced and the restored process paused; it never activates a root, revives an
//! approval/lease/trigger or claims an unknown external effect succeeded.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RESTORE_VERIFICATION_SCHEMA: &str = "kiana.restore-verification.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreVerificationState {
    Blocked,
    NeedsReconciliation,
    Verified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreVerificationFact {
    pub schema: String,
    pub snapshot_manifest_digest: String,
    pub owner_scope: String,
    pub store_id: String,
    pub previous_instance_id: Option<String>,
    pub restored_instance_id: String,
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub projection_digest: String,
    pub data_epoch: u64,
    pub previous_data_epoch: Option<u64>,
    pub authority_epoch: u64,
    pub previous_authority_epoch: Option<u64>,
    pub artifact_refs: Vec<String>,
    pub pending_approval_count: u32,
    pub active_lease_count: u32,
    pub trigger_count: u32,
    pub unknown_effect_count: u32,
    pub old_instance_fenced: bool,
    pub old_authority_fenced: bool,
    pub external_effects_reconciled: bool,
    pub default_paused: bool,
    pub reconciliation_refs: Vec<String>,
    pub state: RestoreVerificationState,
    pub reason: String,
    pub verification_digest: String,
}

impl RestoreVerificationFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != RESTORE_VERIFICATION_SCHEMA
            || !valid_digest(&self.snapshot_manifest_digest)
            || !valid_text(&self.owner_scope)
            || !valid_text(&self.store_id)
            || self
                .previous_instance_id
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || !valid_text(&self.restored_instance_id)
            || self
                .previous_instance_id
                .as_deref()
                .is_some_and(|value| value == self.restored_instance_id)
            || self.source_cursor == 0
            || self.projection_cursor > self.source_cursor
            || !valid_digest(&self.projection_digest)
            || self.data_epoch == 0
            || self
                .previous_data_epoch
                .is_some_and(|value| value >= self.data_epoch)
            || self.authority_epoch == 0
            || self
                .previous_authority_epoch
                .is_some_and(|value| value >= self.authority_epoch)
            || self.artifact_refs.is_empty()
            || self.artifact_refs.len() > 256
            || self.artifact_refs.iter().any(|value| !valid_text(value))
            || has_duplicates(&self.artifact_refs)
            || !self.default_paused
            || self
                .reconciliation_refs
                .iter()
                .any(|value| !valid_text(value))
            || self.reconciliation_refs.len() > 64
            || has_duplicates(&self.reconciliation_refs)
            || !valid_text(&self.reason)
            || !valid_digest(&self.verification_digest)
            || self.verification_digest != self.digest()
        {
            return Err("restore_verification_fact_invalid");
        }
        if self.previous_instance_id.is_some()
            && (!self.old_instance_fenced || !self.old_authority_fenced)
        {
            return Err("restore_old_instance_not_fenced");
        }
        if self.external_effects_reconciled && self.unknown_effect_count != 0 {
            return Err("restore_unknown_effect_reconciled_mismatch");
        }
        match self.state {
            RestoreVerificationState::Blocked => {
                if self.reason == "ok" {
                    return Err("restore_blocked_reason_invalid");
                }
            }
            RestoreVerificationState::NeedsReconciliation => {
                if self.pending_approval_count == 0
                    && self.active_lease_count == 0
                    && self.trigger_count == 0
                    && self.unknown_effect_count == 0
                {
                    return Err("restore_reconciliation_not_needed");
                }
                if self.reconciliation_refs.is_empty() || self.external_effects_reconciled {
                    return Err("restore_reconciliation_evidence_invalid");
                }
            }
            RestoreVerificationState::Verified => {
                if self.previous_instance_id.is_none()
                    || self.previous_data_epoch.is_none()
                    || self.previous_authority_epoch.is_none()
                    || self.projection_cursor != self.source_cursor
                    || self.pending_approval_count != 0
                    || self.active_lease_count != 0
                    || self.trigger_count != 0
                    || self.unknown_effect_count != 0
                    || !self.old_instance_fenced
                    || !self.old_authority_fenced
                    || !self.external_effects_reconciled
                    || !self.reconciliation_refs.is_empty()
                    || self.reason != "ok"
                {
                    return Err("restore_verification_not_ready");
                }
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "snapshot_manifest_digest": self.snapshot_manifest_digest,
            "owner_scope": self.owner_scope,
            "store_id": self.store_id,
            "previous_instance_id": self.previous_instance_id,
            "restored_instance_id": self.restored_instance_id,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_digest": self.projection_digest,
            "data_epoch": self.data_epoch,
            "previous_data_epoch": self.previous_data_epoch,
            "authority_epoch": self.authority_epoch,
            "previous_authority_epoch": self.previous_authority_epoch,
            "artifact_refs": self.artifact_refs,
            "pending_approval_count": self.pending_approval_count,
            "active_lease_count": self.active_lease_count,
            "trigger_count": self.trigger_count,
            "unknown_effect_count": self.unknown_effect_count,
            "old_instance_fenced": self.old_instance_fenced,
            "old_authority_fenced": self.old_authority_fenced,
            "external_effects_reconciled": self.external_effects_reconciled,
            "default_paused": self.default_paused,
            "reconciliation_refs": self.reconciliation_refs,
            "state": self.state,
            "reason": self.reason,
        }))
    }
}

pub fn validate_restore_verification(fact: &RestoreVerificationFact) -> Result<(), &'static str> {
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

fn has_duplicates(values: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value))
}
