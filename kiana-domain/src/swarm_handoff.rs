//! SW-16 directed structured handoff/status/evidence/incident records.
//!
//! These records are addressed to one recipient and retain evidence. They are not a free message
//! bus, prompt transcript or runtime authorization; any action must re-enter ControlPlane.

use crate::{json_digest, SwarmPlanId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SWARM_HANDOFF_SCHEMA: &str = "kiana.swarm-handoff.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmHandoffKind {
    WorkPacket,
    HandoffAck,
    StatusReport,
    Evidence,
    Incident,
    Symposium,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmHandoffStatus {
    Sent,
    Acknowledged,
    Rejected,
    NeedsReconciliation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmHandoffRecord {
    pub schema: String,
    pub swarm_plan_id: SwarmPlanId,
    pub sender_id: String,
    pub recipient_id: String,
    pub kind: SwarmHandoffKind,
    pub packet_ref: String,
    pub source_cursor: u64,
    pub status: SwarmHandoffStatus,
    pub summary: String,
    pub evidence_refs: Vec<String>,
    pub ack_digest: Option<String>,
    pub symposium_round: Option<u32>,
    pub symposium_max_rounds: Option<u32>,
    pub authority_granted: bool,
    pub record_digest: String,
}

impl SwarmHandoffRecord {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_HANDOFF_SCHEMA
            || self.swarm_plan_id.as_uuid().is_nil()
            || !valid_text(&self.sender_id)
            || !valid_text(&self.recipient_id)
            || self.recipient_id == "broadcast"
            || !valid_text(&self.packet_ref)
            || self.source_cursor == 0
            || !valid_text(&self.summary)
            || self.evidence_refs.len() > 64
            || self.evidence_refs.iter().any(|value| !valid_text(value))
            || has_duplicates(&self.evidence_refs)
            || self
                .ack_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self.authority_granted
            || !valid_digest(&self.record_digest)
            || self.record_digest != self.digest()
        {
            return Err("swarm_handoff_record_invalid");
        }
        if matches!(
            self.kind,
            SwarmHandoffKind::Evidence | SwarmHandoffKind::Incident
        ) && self.evidence_refs.is_empty()
        {
            return Err("swarm_handoff_evidence_required");
        }
        if self.kind == SwarmHandoffKind::HandoffAck && self.ack_digest.is_none() {
            return Err("swarm_handoff_ack_required");
        }
        if self.kind == SwarmHandoffKind::Symposium {
            if self
                .symposium_round
                .zip(self.symposium_max_rounds)
                .is_none_or(|(round, max)| round == 0 || max == 0 || round > max || max > 8)
            {
                return Err("swarm_handoff_symposium_bounds_invalid");
            }
        } else if self.symposium_round.is_some() || self.symposium_max_rounds.is_some() {
            return Err("swarm_handoff_symposium_fields_unexpected");
        }
        if self.status == SwarmHandoffStatus::NeedsReconciliation && self.evidence_refs.is_empty() {
            return Err("swarm_handoff_reconciliation_evidence_required");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "swarm_plan_id": self.swarm_plan_id,
            "sender_id": self.sender_id,
            "recipient_id": self.recipient_id,
            "kind": self.kind,
            "packet_ref": self.packet_ref,
            "source_cursor": self.source_cursor,
            "status": self.status,
            "summary": self.summary,
            "evidence_refs": self.evidence_refs,
            "ack_digest": self.ack_digest,
            "symposium_round": self.symposium_round,
            "symposium_max_rounds": self.symposium_max_rounds,
            "authority_granted": self.authority_granted,
        }))
    }
}

pub fn validate_swarm_handoff(record: &SwarmHandoffRecord) -> Result<(), &'static str> {
    record.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\r', '\n'])
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
