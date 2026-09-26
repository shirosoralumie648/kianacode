//! Packet-level acceptance after immutable execution evidence and independent review.

use crate::{json_digest, CompanyEvidenceReady, IndependentReview, ReviewVerdict};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const PACKET_ACCEPTANCE_SCHEMA: &str = "kiana.packet-acceptance.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketAcceptanceDecision {
    Accept,
    Reject,
    Waive,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketAcceptanceRequest {
    pub schema: String,
    pub acceptance_id: String,
    pub project_id: String,
    pub packet_id: String,
    pub packet_version: u64,
    pub attempt_ref: String,
    pub author_session_ids: Vec<String>,
    pub review: IndependentReview,
    pub evidence: CompanyEvidenceReady,
    pub acceptor_assignment: String,
    pub acceptor_session: String,
    pub acceptor_role: String,
    pub decision: PacketAcceptanceDecision,
    pub reasons: Vec<String>,
    pub waiver_ref: Option<String>,
    pub declared_dependents: Vec<String>,
    pub decided_at: u64,
    pub digest: String,
}

impl PacketAcceptanceRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PACKET_ACCEPTANCE_SCHEMA
            || self.packet_version == 0
            || self.decided_at == 0
        {
            return Err("packet_acceptance_invalid");
        }
        for (value, field) in [
            (&self.acceptance_id, "packet_acceptance_id_required"),
            (&self.project_id, "packet_acceptance_project_required"),
            (&self.packet_id, "packet_acceptance_packet_required"),
            (&self.attempt_ref, "packet_acceptance_attempt_required"),
            (
                &self.acceptor_assignment,
                "packet_acceptance_assignment_required",
            ),
            (&self.acceptor_session, "packet_acceptance_session_required"),
        ] {
            required(value, field)?;
        }
        if self.author_session_ids.is_empty()
            || self
                .author_session_ids
                .iter()
                .any(|session| session.trim().is_empty())
        {
            return Err("packet_acceptance_authors_required");
        }
        if self.acceptor_session == self.review.assignment.reviewer_session_id
            || self.author_session_ids.contains(&self.acceptor_session)
        {
            return Err("packet_acceptance_acceptor_not_independent");
        }
        self.review.validate()?;
        if self.review.assignment.project_id != self.project_id
            || self.review.assignment.packet_id != self.packet_id
            || self.review.assignment.packet_version != self.packet_version
            || self.review.assignment.author_session_ids != self.author_session_ids
        {
            return Err("packet_acceptance_review_binding_mismatch");
        }
        self.evidence.validate()?;
        if self.review.assignment.evidence_bundle_digest != self.evidence.bundle_digest {
            return Err("packet_acceptance_evidence_binding_mismatch");
        }
        let mut dependents = BTreeSet::new();
        if self.declared_dependents.iter().any(|dependent| {
            required(dependent, "packet_acceptance_dependent_invalid").is_err()
                || !dependents.insert(dependent)
        }) {
            return Err("packet_acceptance_dependents_invalid");
        }
        match self.decision {
            PacketAcceptanceDecision::Accept => {
                if self.review.criterion_results.values().any(|result| {
                    matches!(
                        result.verdict,
                        ReviewVerdict::Fail | ReviewVerdict::InsufficientEvidence
                    )
                }) {
                    return Err("packet_acceptance_criteria_failed");
                }
                if self.acceptor_role != "closer" && self.acceptor_role != "sponsor" {
                    return Err("packet_acceptance_acceptor_role_invalid");
                }
            }
            PacketAcceptanceDecision::Reject => {
                if self.reasons.is_empty()
                    || self.reasons.iter().any(|reason| reason.trim().is_empty())
                {
                    return Err("packet_acceptance_rejection_reason_required");
                }
            }
            PacketAcceptanceDecision::Waive => {
                if self.acceptor_role != "sponsor"
                    || self
                        .waiver_ref
                        .as_deref()
                        .is_none_or(|reference| !reference.starts_with("waiver:"))
                {
                    return Err("packet_acceptance_waiver_requires_sponsor");
                }
                if self.reasons.is_empty() {
                    return Err("packet_acceptance_waiver_reason_required");
                }
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("packet_acceptance_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema, "acceptance_id": self.acceptance_id, "project_id": self.project_id,
            "packet_id": self.packet_id, "packet_version": self.packet_version, "attempt_ref": self.attempt_ref,
            "author_session_ids": self.author_session_ids, "review": self.review, "evidence": self.evidence,
            "acceptor_assignment": self.acceptor_assignment, "acceptor_session": self.acceptor_session,
            "acceptor_role": self.acceptor_role, "decision": self.decision, "reasons": self.reasons,
            "waiver_ref": self.waiver_ref, "declared_dependents": self.declared_dependents, "decided_at": self.decided_at,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PacketAcceptanceLedger {
    pub requests: BTreeMap<String, PacketAcceptanceRequest>,
}

impl PacketAcceptanceLedger {
    pub fn record(&mut self, request: PacketAcceptanceRequest) -> Result<(), &'static str> {
        request.validate()?;
        if let Some(existing) = self.requests.get(&request.acceptance_id) {
            if existing.digest == request.digest {
                return Ok(());
            }
            return Err("packet_acceptance_duplicate_digest_mismatch");
        }
        self.requests.insert(request.acceptance_id.clone(), request);
        Ok(())
    }

    pub fn accepted_dependents(&self, acceptance_id: &str) -> Option<&[String]> {
        self.requests
            .get(acceptance_id)
            .filter(|request| request.decision == PacketAcceptanceDecision::Accept)
            .map(|request| request.declared_dependents.as_slice())
    }
}
