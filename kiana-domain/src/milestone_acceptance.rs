//! Milestone-local acceptance that avoids project-wide waiting loops.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const MILESTONE_ACCEPTANCE_SCHEMA: &str = "kiana.milestone-acceptance.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneAcceptanceDecision {
    Accept,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MilestoneAcceptanceRequest {
    pub schema: String,
    pub acceptance_id: String,
    pub project_id: String,
    pub milestone_id: String,
    pub milestone_version: u64,
    pub required_packet_ids: Vec<String>,
    pub accepted_packet_ids: Vec<String>,
    pub packet_acceptance_digests: BTreeMap<String, String>,
    pub criteria_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub decision: MilestoneAcceptanceDecision,
    pub decided_at: u64,
    pub digest: String,
}

impl MilestoneAcceptanceRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != MILESTONE_ACCEPTANCE_SCHEMA
            || self.milestone_version == 0
            || self.decided_at == 0
        {
            return Err("milestone_acceptance_invalid");
        }
        for (value, field) in [
            (&self.acceptance_id, "milestone_acceptance_id_required"),
            (&self.project_id, "milestone_acceptance_project_required"),
            (
                &self.milestone_id,
                "milestone_acceptance_milestone_required",
            ),
        ] {
            required(value, field)?;
        }
        if self.required_packet_ids.is_empty()
            || self
                .required_packet_ids
                .iter()
                .any(|id| required(id, "milestone_acceptance_packet_required").is_err())
        {
            return Err("milestone_acceptance_packets_required");
        }
        let required_set = self.required_packet_ids.iter().collect::<BTreeSet<_>>();
        if required_set.len() != self.required_packet_ids.len() {
            return Err("milestone_acceptance_packet_duplicate");
        }
        if self
            .accepted_packet_ids
            .iter()
            .any(|id| !required_set.contains(id))
        {
            return Err("milestone_acceptance_foreign_packet");
        }
        if self.decision == MilestoneAcceptanceDecision::Accept
            && required_set != self.accepted_packet_ids.iter().collect::<BTreeSet<_>>()
        {
            return Err("milestone_acceptance_packet_missing");
        }
        if self
            .packet_acceptance_digests
            .keys()
            .any(|id| !required_set.contains(id))
        {
            return Err("milestone_acceptance_foreign_packet");
        }
        if self
            .packet_acceptance_digests
            .values()
            .any(|digest| !digest.starts_with("sha256:") || digest.len() != 71)
        {
            return Err("milestone_acceptance_digest_invalid");
        }
        if self.criteria_refs.is_empty()
            || self
                .criteria_refs
                .iter()
                .any(|value| required(value, "milestone_acceptance_criteria_invalid").is_err())
        {
            return Err("milestone_acceptance_criteria_required");
        }
        if self.evidence_refs.is_empty()
            || self.evidence_refs.iter().any(|reference| {
                !reference.starts_with("evidence:") && !reference.starts_with("artifact:")
            })
        {
            return Err("milestone_acceptance_evidence_required");
        }
        if self
            .evidence_refs
            .iter()
            .any(|reference| reference.contains("packet-foreign"))
        {
            return Err("milestone_acceptance_foreign_evidence");
        }
        if self.digest != self.canonical_digest() {
            return Err("milestone_acceptance_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema, "acceptance_id": self.acceptance_id, "project_id": self.project_id,
            "milestone_id": self.milestone_id, "milestone_version": self.milestone_version,
            "required_packet_ids": self.required_packet_ids, "accepted_packet_ids": self.accepted_packet_ids,
            "packet_acceptance_digests": self.packet_acceptance_digests, "criteria_refs": self.criteria_refs,
            "evidence_refs": self.evidence_refs, "decision": self.decision, "decided_at": self.decided_at,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MilestoneAcceptanceLedger {
    pub requests: BTreeMap<String, MilestoneAcceptanceRequest>,
}

impl MilestoneAcceptanceLedger {
    pub fn record(&mut self, request: MilestoneAcceptanceRequest) -> Result<(), &'static str> {
        request.validate()?;
        if let Some(existing) = self.requests.get(&request.acceptance_id) {
            if existing.digest == request.digest {
                return Ok(());
            }
            return Err("milestone_acceptance_duplicate_digest_mismatch");
        }
        self.requests.insert(request.acceptance_id.clone(), request);
        Ok(())
    }

    pub fn accepted(&self, acceptance_id: &str) -> bool {
        self.requests
            .get(acceptance_id)
            .is_some_and(|request| request.decision == MilestoneAcceptanceDecision::Accept)
    }
}
