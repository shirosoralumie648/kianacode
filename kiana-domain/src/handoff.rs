//! A typed packet transfer and its independently authored ACK, retained in Company facts.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffStatus {
    Pending,
    Acknowledged,
    Rejected,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PacketHandoff {
    pub handoff_id: String,
    pub packet_id: String,
    pub packet_version: u64,
    pub from_department: String,
    pub from_role: String,
    pub from_session: SessionId,
    pub to_department: String,
    pub to_role: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub status: HandoffStatus,
    pub acknowledgement: Option<HandoffAck>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandoffAck {
    pub handoff_id: String,
    pub receiver_session: SessionId,
    pub receiver_role: String,
    pub accepted: bool,
    pub reason: String,
    pub acknowledged_at: u64,
}
impl PacketHandoff {
    pub fn acknowledge(
        &mut self,
        a: &CompanyAuthority,
        accepted: bool,
        reason: &str,
    ) -> Result<(), &'static str> {
        if self.status != HandoffStatus::Pending || a.now_ms >= self.expires_at {
            return Err("handoff_not_pending_or_expired");
        }
        if a.role_id != self.to_role || a.session_id == self.from_session {
            return Err("handoff_receiver_identity_invalid");
        }
        if reason.trim().is_empty() {
            return Err("handoff_ack_reason_required");
        }
        self.status = if accepted {
            HandoffStatus::Acknowledged
        } else {
            HandoffStatus::Rejected
        };
        self.acknowledgement = Some(HandoffAck {
            handoff_id: self.handoff_id.clone(),
            receiver_session: a.session_id.clone(),
            receiver_role: a.role_id.clone(),
            accepted,
            reason: reason.into(),
            acknowledged_at: a.now_ms,
        });
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PacketReview {
    pub review_id: String,
    pub packet_id: String,
    pub packet_version: u64,
    pub author_run_id: RunId,
    pub author_session_id: SessionId,
    pub reviewer_session_id: SessionId,
    pub criterion_results: BTreeMap<String, bool>,
    pub evidence_refs: Vec<String>,
    pub reviewed_at: u64,
}
