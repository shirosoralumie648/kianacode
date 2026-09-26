//! Project-level acceptance aggregate over milestone-local acceptance facts.

use crate::{json_digest, IndependentReview, MilestoneAcceptanceDecision, ReviewVerdict};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const PROJECT_ACCEPTANCE_SCHEMA: &str = "kiana.project-acceptance.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAcceptanceDecision {
    Accept,
    Reject,
    Waive,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAcceptanceRequest {
    pub schema: String,
    pub acceptance_id: String,
    pub project_id: String,
    pub project_version: u64,
    pub required_milestone_acceptance_ids: Vec<String>,
    pub milestone_acceptance_digests: BTreeMap<String, String>,
    pub review: IndependentReview,
    pub criteria_refs: Vec<String>,
    pub acceptor_assignment: String,
    pub acceptor_session: String,
    pub acceptor_role: String,
    pub decision: ProjectAcceptanceDecision,
    pub reasons: Vec<String>,
    pub waiver_ref: Option<String>,
    pub decided_at: u64,
    pub digest: String,
}

impl ProjectAcceptanceRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PROJECT_ACCEPTANCE_SCHEMA
            || self.project_version == 0
            || self.decided_at == 0
        {
            return Err("project_acceptance_invalid");
        }
        for (value, field) in [
            (&self.acceptance_id, "project_acceptance_id_required"),
            (&self.project_id, "project_acceptance_project_required"),
            (
                &self.acceptor_assignment,
                "project_acceptance_assignment_required",
            ),
            (
                &self.acceptor_session,
                "project_acceptance_session_required",
            ),
        ] {
            required(value, field)?;
        }
        if self.required_milestone_acceptance_ids.is_empty() {
            return Err("project_acceptance_milestones_required");
        }
        let required_set = self
            .required_milestone_acceptance_ids
            .iter()
            .collect::<BTreeSet<_>>();
        if required_set.len() != self.required_milestone_acceptance_ids.len()
            || self
                .milestone_acceptance_digests
                .keys()
                .any(|id| !required_set.contains(id))
        {
            return Err("project_acceptance_milestone_binding_invalid");
        }
        if self.milestone_acceptance_digests.len() != required_set.len()
            || self
                .milestone_acceptance_digests
                .values()
                .any(|digest| !digest.starts_with("sha256:") || digest.len() != 71)
        {
            return Err("project_acceptance_milestone_digest_invalid");
        }
        self.review.validate()?;
        if self.review.assignment.project_id != self.project_id
            || self.review.assignment.packet_id != "project"
        {
            return Err("project_acceptance_review_binding_mismatch");
        }
        if self.acceptor_session == self.review.assignment.reviewer_session_id {
            return Err("project_acceptance_acceptor_not_independent");
        }
        if self.criteria_refs.is_empty()
            || self.criteria_refs.iter().any(|criterion| {
                required(criterion, "project_acceptance_criteria_invalid").is_err()
            })
        {
            return Err("project_acceptance_criteria_required");
        }
        match self.decision {
            ProjectAcceptanceDecision::Accept => {
                if self.review.criterion_results.values().any(|result| {
                    matches!(
                        result.verdict,
                        ReviewVerdict::Fail | ReviewVerdict::InsufficientEvidence
                    )
                }) {
                    return Err("project_acceptance_criteria_failed");
                }
                if self.acceptor_role != "sponsor" && self.acceptor_role != "closer" {
                    return Err("project_acceptance_acceptor_role_invalid");
                }
            }
            ProjectAcceptanceDecision::Reject => {
                if self.reasons.is_empty() {
                    return Err("project_acceptance_rejection_reason_required");
                }
            }
            ProjectAcceptanceDecision::Waive => {
                if self.acceptor_role != "sponsor"
                    || self
                        .waiver_ref
                        .as_deref()
                        .is_none_or(|reference| !reference.starts_with("waiver:"))
                {
                    return Err("project_acceptance_waiver_requires_sponsor");
                }
                if self.reasons.is_empty() {
                    return Err("project_acceptance_waiver_reason_required");
                }
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("project_acceptance_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema, "acceptance_id": self.acceptance_id, "project_id": self.project_id,
            "project_version": self.project_version, "required_milestone_acceptance_ids": self.required_milestone_acceptance_ids,
            "milestone_acceptance_digests": self.milestone_acceptance_digests, "review": self.review,
            "criteria_refs": self.criteria_refs, "acceptor_assignment": self.acceptor_assignment,
            "acceptor_session": self.acceptor_session, "acceptor_role": self.acceptor_role,
            "decision": self.decision, "reasons": self.reasons, "waiver_ref": self.waiver_ref,
            "decided_at": self.decided_at,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectAcceptanceLedger {
    pub requests: BTreeMap<String, ProjectAcceptanceRequest>,
}

impl ProjectAcceptanceLedger {
    pub fn record(&mut self, request: ProjectAcceptanceRequest) -> Result<(), &'static str> {
        request.validate()?;
        if let Some(existing) = self.requests.get(&request.acceptance_id) {
            if existing.digest == request.digest {
                return Ok(());
            }
            return Err("project_acceptance_duplicate_digest_mismatch");
        }
        self.requests.insert(request.acceptance_id.clone(), request);
        Ok(())
    }
}
