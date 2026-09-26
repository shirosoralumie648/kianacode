//! Bounded packet rework and successor provenance.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const REWORK_PROVENANCE_SCHEMA: &str = "kiana.rework-provenance.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReworkAttemptState {
    NeverStarted,
    Failed,
    Rejected,
    Stopped,
    Unknown,
    Succeeded,
}

impl ReworkAttemptState {
    fn reclaimable(self) -> bool {
        matches!(self, Self::Failed | Self::Rejected | Self::Stopped)
    }
    fn terminal(self) -> bool {
        matches!(self, Self::Unknown | Self::Succeeded)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReworkProvenance {
    pub schema: String,
    pub predecessor_packet_id: String,
    pub successor_packet_id: String,
    pub rejection_ref: String,
    pub rejection_digest: String,
    pub baseline_digest: String,
    pub successor_packet_digest: String,
    pub reason: String,
    pub max_attempts: u32,
    pub attempts_used: u32,
    pub remaining_budget: u64,
    pub predecessor_state: ReworkAttemptState,
    pub digest: String,
}

impl ReworkProvenance {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != REWORK_PROVENANCE_SCHEMA
            || self.max_attempts == 0
            || self.max_attempts > 3
            || self.attempts_used == 0
            || self.attempts_used > self.max_attempts
        {
            return Err("rework_provenance_limit_invalid");
        }
        for (value, field) in [
            (&self.predecessor_packet_id, "rework_predecessor_required"),
            (&self.successor_packet_id, "rework_successor_required"),
            (&self.rejection_ref, "rework_rejection_required"),
            (&self.reason, "rework_reason_required"),
        ] {
            required(value, field)?;
        }
        if self.predecessor_packet_id == self.successor_packet_id {
            return Err("rework_successor_identity_invalid");
        }
        for digest in [
            &self.rejection_digest,
            &self.baseline_digest,
            &self.successor_packet_digest,
        ] {
            if !digest.starts_with("sha256:") || digest.len() != 71 {
                return Err("rework_digest_invalid");
            }
        }
        if self.predecessor_state.terminal() {
            return Err("rework_terminal_attempt_cannot_revive");
        }
        if !self.predecessor_state.reclaimable() {
            return Err("rework_attempt_not_reclaimable");
        }
        if self.digest != self.canonical_digest() {
            return Err("rework_provenance_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema, "predecessor_packet_id": self.predecessor_packet_id,
            "successor_packet_id": self.successor_packet_id, "rejection_ref": self.rejection_ref,
            "rejection_digest": self.rejection_digest, "baseline_digest": self.baseline_digest,
            "successor_packet_digest": self.successor_packet_digest, "reason": self.reason,
            "max_attempts": self.max_attempts, "attempts_used": self.attempts_used,
            "remaining_budget": self.remaining_budget, "predecessor_state": self.predecessor_state,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReworkLedger {
    pub successors: BTreeMap<String, ReworkProvenance>,
}

impl ReworkLedger {
    pub fn record(&mut self, provenance: ReworkProvenance) -> Result<(), &'static str> {
        provenance.validate()?;
        if let Some(existing) = self.successors.get(&provenance.predecessor_packet_id) {
            if existing.digest == provenance.digest {
                return Ok(());
            }
            return Err("rework_duplicate_provenance");
        }
        let mut cursor = provenance.successor_packet_id.clone();
        let mut seen = BTreeSet::new();
        while let Some(next) = self.successors.get(&cursor) {
            if cursor == provenance.predecessor_packet_id || !seen.insert(cursor.clone()) {
                return Err("rework_successor_cycle");
            }
            cursor = next.successor_packet_id.clone();
        }
        if cursor == provenance.predecessor_packet_id {
            return Err("rework_successor_cycle");
        }
        self.successors
            .insert(provenance.predecessor_packet_id.clone(), provenance);
        Ok(())
    }

    pub fn successor(&self, predecessor: &str) -> Option<&str> {
        self.successors
            .get(predecessor)
            .map(|p| p.successor_packet_id.as_str())
    }
}
