//! Versioned CompanyOS accountability handoffs.
//!
//! `PacketHandoff` predates the department packet and assignment contracts.  This module keeps
//! that compatibility type intact and adds the stricter CO-17 contract: a transfer freezes the
//! exact packet revision and material basis, binds both sides to assignment revisions, and moves
//! accountability only after a recipient ACK.  Rejection and expiry leave the original owner in
//! charge and create an explicit escalation object.  No runtime grant or lease is carried here.

use crate::{json_digest, DepartmentPacket, RoleSpec, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const COMPANY_HANDOFF_SCHEMA: &str = "kiana.company-handoff.v1";
pub const COMPANY_HANDOFF_ACK_SCHEMA: &str = "kiana.company-handoff-ack.v1";
pub const COMPANY_HANDOFF_ESCALATION_SCHEMA: &str = "kiana.company-handoff-escalation.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(field);
    }
    Ok(())
}

/// The assignment snapshot that owns one side of a handoff.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffAssignment {
    pub assignment_id: String,
    pub assignment_version: u64,
    pub principal_id: String,
    pub project_id: String,
    pub department_id: String,
    pub role_id: String,
    pub session_id: SessionId,
    pub valid_from: u64,
    pub expires_at: u64,
    #[serde(default)]
    pub revoked: bool,
}

impl HandoffAssignment {
    pub fn validate(&self) -> Result<(), &'static str> {
        for (value, field) in [
            (&self.assignment_id, "handoff_assignment_id_invalid"),
            (&self.principal_id, "handoff_assignment_principal_invalid"),
            (&self.project_id, "handoff_assignment_project_invalid"),
            (&self.department_id, "handoff_assignment_department_invalid"),
            (&self.role_id, "handoff_assignment_role_invalid"),
        ] {
            required(value, field)?;
        }
        if self.assignment_version == 0
            || self.valid_from == 0
            || self.expires_at <= self.valid_from
            || self.session_id.as_str().trim().is_empty()
        {
            return Err("handoff_assignment_window_invalid");
        }
        let role = RoleSpec::lookup(&self.role_id).ok_or("handoff_assignment_role_unknown")?;
        if role.department_id != self.department_id {
            return Err("handoff_assignment_department_mismatch");
        }
        Ok(())
    }

    pub fn active_at(&self, now_ms: u64) -> bool {
        !self.revoked && now_ms >= self.valid_from && now_ms < self.expires_at
    }
}

/// Frozen packet material transferred by a handoff.  The receiver must ACK this exact basis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffPacketRevision {
    pub packet: DepartmentPacket,
    pub packet_digest: String,
    pub budget_ref: String,
}

impl HandoffPacketRevision {
    pub fn new(
        packet: DepartmentPacket,
        packet_digest: impl Into<String>,
        budget_ref: impl Into<String>,
    ) -> Self {
        Self {
            packet,
            packet_digest: packet_digest.into(),
            budget_ref: budget_ref.into(),
        }
    }

    pub fn canonical_packet_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.packet.schema,
            "packet_id": self.packet.packet_id,
            "project_id": self.packet.project_id,
            "version": self.packet.version,
            "kind": self.packet.kind,
            "from_department": self.packet.from_department,
            "target_role": self.packet.target_role,
            "assignment_ref": self.packet.assignment_ref,
            "input_basis": self.packet.input_basis,
            "input_refs": self.packet.input_refs,
            "result": self.packet.result,
            "write_scope": self.packet.write_scope,
            "plan_ref": self.packet.plan_ref,
        }))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        self.packet
            .validate()
            .map_err(|_| "handoff_packet_invalid")?;
        if self.packet.version == 0 {
            return Err("handoff_packet_revision_invalid");
        }
        digest(&self.packet_digest, "handoff_packet_digest_invalid")?;
        required(&self.budget_ref, "handoff_budget_ref_required")?;
        if self.packet_digest != self.canonical_packet_digest() {
            return Err("handoff_packet_digest_mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyHandoffStatus {
    Pending,
    Acknowledged,
    Rejected,
    Expired,
}

impl CompanyHandoffStatus {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffAcceptanceEvidence {
    pub packet_digest: String,
    pub input_refs: Vec<String>,
    pub budget_ref: String,
    pub write_scope: Vec<String>,
}

impl HandoffAcceptanceEvidence {
    fn matches(&self, packet: &HandoffPacketRevision) -> bool {
        self.packet_digest == packet.packet_digest
            && self.input_refs == packet.packet.input_refs
            && self.budget_ref == packet.budget_ref
            && self.write_scope == packet.packet.write_scope
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyHandoffAck {
    pub schema: String,
    pub handoff_id: String,
    pub receiver_assignment_id: String,
    pub receiver_assignment_version: u64,
    pub receiver_session: SessionId,
    pub accepted: bool,
    pub reason: String,
    pub evidence: HandoffAcceptanceEvidence,
    pub acknowledged_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffEscalation {
    pub schema: String,
    pub escalation_id: String,
    pub handoff_id: String,
    pub owner_assignment_id: String,
    pub reason: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyHandoff {
    pub schema: String,
    pub handoff_id: String,
    pub packet: HandoffPacketRevision,
    pub owner: HandoffAssignment,
    pub recipient: HandoffAssignment,
    pub created_at: u64,
    pub expires_at: u64,
    pub status: CompanyHandoffStatus,
    /// The accountability owner remains the sender until an accepted ACK is committed.
    pub current_owner_assignment_id: String,
    /// A pending recipient is visible to both department projections.
    pub pending_recipient_assignment_id: Option<String>,
    pub acknowledgement: Option<CompanyHandoffAck>,
    pub escalation: Option<HandoffEscalation>,
    /// Deliberately kept empty: an ACK never acquires a runtime lease.
    #[serde(default)]
    pub runtime_lease_id: Option<String>,
    pub digest: String,
}

impl CompanyHandoff {
    #[allow(clippy::too_many_arguments)]
    pub fn offer(
        handoff_id: impl Into<String>,
        packet: HandoffPacketRevision,
        owner: HandoffAssignment,
        recipient: HandoffAssignment,
        created_at: u64,
        expires_at: u64,
    ) -> Result<Self, &'static str> {
        let handoff_id = handoff_id.into();
        packet.validate()?;
        owner.validate()?;
        recipient.validate()?;
        required(&handoff_id, "handoff_id_invalid")?;
        if created_at == 0 || expires_at <= created_at {
            return Err("handoff_window_invalid");
        }
        if owner.project_id != packet.packet.project_id
            || recipient.project_id != packet.packet.project_id
        {
            return Err("handoff_assignment_project_mismatch");
        }
        if recipient.role_id != packet.packet.target_role
            || recipient.assignment_id != packet.packet.assignment_ref
        {
            return Err("handoff_recipient_assignment_mismatch");
        }
        if owner.assignment_id == recipient.assignment_id
            || owner.session_id == recipient.session_id
        {
            return Err("handoff_parties_must_be_distinct");
        }
        if !owner.active_at(created_at) || !recipient.active_at(created_at) {
            return Err("handoff_assignment_inactive");
        }
        if expires_at > recipient.expires_at || expires_at > owner.expires_at {
            return Err("handoff_expiry_exceeds_assignment");
        }
        let mut handoff = Self {
            schema: COMPANY_HANDOFF_SCHEMA.to_owned(),
            handoff_id,
            packet,
            owner: owner.clone(),
            recipient: recipient.clone(),
            created_at,
            expires_at,
            status: CompanyHandoffStatus::Pending,
            current_owner_assignment_id: owner.assignment_id,
            pending_recipient_assignment_id: Some(recipient.assignment_id),
            acknowledgement: None,
            escalation: None,
            runtime_lease_id: None,
            digest: String::new(),
        };
        handoff.digest = handoff.canonical_digest();
        handoff.validate()?;
        Ok(handoff)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_HANDOFF_SCHEMA {
            return Err("handoff_schema_invalid");
        }
        required(&self.handoff_id, "handoff_id_invalid")?;
        self.packet.validate()?;
        self.owner.validate()?;
        self.recipient.validate()?;
        if self.created_at == 0 || self.expires_at <= self.created_at {
            return Err("handoff_window_invalid");
        }
        if self.owner.project_id != self.packet.packet.project_id
            || self.recipient.project_id != self.packet.packet.project_id
            || self.recipient.role_id != self.packet.packet.target_role
            || self.recipient.assignment_id != self.packet.packet.assignment_ref
        {
            return Err("handoff_assignment_binding_invalid");
        }
        if self.expires_at > self.owner.expires_at || self.expires_at > self.recipient.expires_at {
            return Err("handoff_expiry_exceeds_assignment");
        }
        if self.current_owner_assignment_id != self.owner.assignment_id
            && self.current_owner_assignment_id != self.recipient.assignment_id
        {
            return Err("handoff_current_owner_invalid");
        }
        if self.status == CompanyHandoffStatus::Pending
            && self.pending_recipient_assignment_id.as_deref()
                != Some(self.recipient.assignment_id.as_str())
        {
            return Err("handoff_pending_recipient_invalid");
        }
        if self.status != CompanyHandoffStatus::Pending
            && self.pending_recipient_assignment_id.is_some()
        {
            return Err("handoff_terminal_recipient_invalid");
        }
        if self.runtime_lease_id.is_some() {
            return Err("handoff_runtime_lease_forbidden");
        }
        if let Some(ack) = &self.acknowledgement {
            if ack.schema != COMPANY_HANDOFF_ACK_SCHEMA
                || ack.handoff_id != self.handoff_id
                || ack.receiver_assignment_id != self.recipient.assignment_id
                || ack.receiver_assignment_version != self.recipient.assignment_version
                || ack.receiver_session != self.recipient.session_id
                || ack.reason.trim().is_empty()
                || ack.acknowledged_at == 0
                || !ack.evidence.matches(&self.packet)
            {
                return Err("handoff_ack_invalid");
            }
            if ack.accepted != (self.status == CompanyHandoffStatus::Acknowledged) {
                return Err("handoff_ack_status_mismatch");
            }
        } else if self.status != CompanyHandoffStatus::Pending {
            return Err("handoff_terminal_ack_missing");
        }
        if let Some(escalation) = &self.escalation {
            if escalation.schema != COMPANY_HANDOFF_ESCALATION_SCHEMA
                || escalation.handoff_id != self.handoff_id
                || escalation.owner_assignment_id != self.owner.assignment_id
                || escalation.reason.trim().is_empty()
                || escalation.created_at == 0
            {
                return Err("handoff_escalation_invalid");
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("handoff_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "handoff_id": self.handoff_id,
            "packet": self.packet,
            "owner": self.owner,
            "recipient": self.recipient,
            "created_at": self.created_at,
            "expires_at": self.expires_at,
            "status": self.status,
            "current_owner_assignment_id": self.current_owner_assignment_id,
            "pending_recipient_assignment_id": self.pending_recipient_assignment_id,
            "acknowledgement": self.acknowledgement,
            "escalation": self.escalation,
            "runtime_lease_id": self.runtime_lease_id,
        }))
    }

    /// Digest of the immutable offer.  Mutable ACK/expiry state is excluded so a retry cannot
    /// create a second transfer after the first decision has been committed.
    pub fn immutable_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "handoff_id": self.handoff_id,
            "packet": self.packet,
            "owner": self.owner,
            "recipient": self.recipient,
            "created_at": self.created_at,
            "expires_at": self.expires_at,
        }))
    }

    pub fn acknowledge(
        &mut self,
        receiver: &HandoffAssignment,
        accepted: bool,
        reason: impl Into<String>,
        evidence: HandoffAcceptanceEvidence,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.validate()?;
        let reason = reason.into();
        if self.status != CompanyHandoffStatus::Pending || now_ms >= self.expires_at {
            return Err("handoff_not_pending_or_expired");
        }
        receiver.validate()?;
        if receiver != &self.recipient || !receiver.active_at(now_ms) {
            return Err("handoff_receiver_assignment_invalid");
        }
        if reason.trim().is_empty() {
            return Err("handoff_ack_reason_required");
        }
        if !evidence.matches(&self.packet) {
            return Err("handoff_acceptance_evidence_mismatch");
        }
        self.status = if accepted {
            CompanyHandoffStatus::Acknowledged
        } else {
            CompanyHandoffStatus::Rejected
        };
        self.current_owner_assignment_id = if accepted {
            self.recipient.assignment_id.clone()
        } else {
            self.owner.assignment_id.clone()
        };
        self.pending_recipient_assignment_id = None;
        self.acknowledgement = Some(CompanyHandoffAck {
            schema: COMPANY_HANDOFF_ACK_SCHEMA.to_owned(),
            handoff_id: self.handoff_id.clone(),
            receiver_assignment_id: receiver.assignment_id.clone(),
            receiver_assignment_version: receiver.assignment_version,
            receiver_session: receiver.session_id.clone(),
            accepted,
            reason: reason.clone(),
            evidence,
            acknowledged_at: now_ms,
        });
        if !accepted {
            self.escalation = Some(self.escalation_for("recipient_rejected", now_ms));
        }
        self.digest = self.canonical_digest();
        self.validate()
    }

    pub fn expire(&mut self, now_ms: u64) -> Result<(), &'static str> {
        self.validate()?;
        if self.status != CompanyHandoffStatus::Pending {
            return Err("handoff_not_pending");
        }
        if now_ms < self.expires_at {
            return Err("handoff_not_expired");
        }
        self.status = CompanyHandoffStatus::Expired;
        self.current_owner_assignment_id = self.owner.assignment_id.clone();
        self.pending_recipient_assignment_id = None;
        self.escalation = Some(self.escalation_for("recipient_ack_timeout", now_ms));
        self.digest = self.canonical_digest();
        self.validate()
    }

    fn escalation_for(&self, reason: &str, created_at: u64) -> HandoffEscalation {
        HandoffEscalation {
            schema: COMPANY_HANDOFF_ESCALATION_SCHEMA.to_owned(),
            escalation_id: format!("{}:escalation", self.handoff_id),
            handoff_id: self.handoff_id.clone(),
            owner_assignment_id: self.owner.assignment_id.clone(),
            reason: reason.to_owned(),
            created_at,
        }
    }

    pub fn projection(&self) -> HandoffProjection {
        HandoffProjection {
            handoff_id: self.handoff_id.clone(),
            packet_id: self.packet.packet.packet_id.clone(),
            packet_version: self.packet.packet.version,
            status: self.status,
            current_owner_assignment_id: self.current_owner_assignment_id.clone(),
            pending_recipient_assignment_id: self.pending_recipient_assignment_id.clone(),
            escalation_id: self
                .escalation
                .as_ref()
                .map(|value| value.escalation_id.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffProjection {
    pub handoff_id: String,
    pub packet_id: String,
    pub packet_version: u64,
    pub status: CompanyHandoffStatus,
    pub current_owner_assignment_id: String,
    pub pending_recipient_assignment_id: Option<String>,
    pub escalation_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyHandoffLedger {
    #[serde(default)]
    pub handoffs: BTreeMap<String, CompanyHandoff>,
}

impl CompanyHandoffLedger {
    pub fn offer(&mut self, handoff: CompanyHandoff) -> Result<(), &'static str> {
        handoff.validate()?;
        if let Some(existing) = self.handoffs.get(&handoff.handoff_id) {
            if existing.immutable_digest() == handoff.immutable_digest() {
                return Ok(());
            }
            return Err("handoff_duplicate_digest_mismatch");
        }
        self.handoffs.insert(handoff.handoff_id.clone(), handoff);
        Ok(())
    }

    pub fn acknowledge(
        &mut self,
        handoff_id: &str,
        receiver: &HandoffAssignment,
        accepted: bool,
        reason: impl Into<String>,
        evidence: HandoffAcceptanceEvidence,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        let handoff = self
            .handoffs
            .get_mut(handoff_id)
            .ok_or("handoff_not_found")?;
        handoff.acknowledge(receiver, accepted, reason, evidence, now_ms)
    }

    pub fn expire(&mut self, handoff_id: &str, now_ms: u64) -> Result<(), &'static str> {
        let handoff = self
            .handoffs
            .get_mut(handoff_id)
            .ok_or("handoff_not_found")?;
        handoff.expire(now_ms)
    }

    pub fn projection(&self, handoff_id: &str) -> Option<HandoffProjection> {
        self.handoffs
            .get(handoff_id)
            .map(CompanyHandoff::projection)
    }

    pub fn pending_for_assignment(&self, assignment_id: &str) -> Vec<HandoffProjection> {
        self.handoffs
            .values()
            .filter(|handoff| {
                handoff.status == CompanyHandoffStatus::Pending
                    && handoff.pending_recipient_assignment_id.as_deref() == Some(assignment_id)
            })
            .map(CompanyHandoff::projection)
            .collect()
    }
}
