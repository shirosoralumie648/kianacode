//! Company decisions, lesson candidates and bounded promotion facts.
//!
//! A candidate is an attributed projection of a published decision/ClosingReceipt. It is not a
//! MemoryRecord and cannot change role policy, write private scratch or bypass memory.review.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_KNOWLEDGE_SCHEMA: &str = "kiana.company-knowledge.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

fn refs(values: &[String], field: &'static str) -> Result<(), &'static str> {
    if values.is_empty() || values.len() > 128 {
        return Err(field);
    }
    if values.iter().any(|value| value.trim().is_empty())
        || values.iter().collect::<BTreeSet<_>>().len() != values.len()
    {
        return Err(field);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyDecisionKind {
    Symposium,
    Closing,
    Review,
    Acceptance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyDecisionRecord {
    pub schema: String,
    pub decision_id: String,
    pub project_id: String,
    pub department_id: String,
    pub kind: CompanyDecisionKind,
    pub source_event_ref: String,
    pub closing_receipt_ref: String,
    pub summary: String,
    pub evidence_refs: Vec<String>,
    pub published_by: String,
    pub published_at: u64,
    pub digest: String,
}

impl CompanyDecisionRecord {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_KNOWLEDGE_SCHEMA || self.published_at == 0 {
            return Err("company_decision_header_invalid");
        }
        for (value, field) in [
            (&self.decision_id, "company_decision_id_required"),
            (&self.project_id, "company_decision_project_required"),
            (&self.department_id, "company_decision_department_required"),
            (&self.source_event_ref, "company_decision_event_required"),
            (
                &self.closing_receipt_ref,
                "company_decision_receipt_required",
            ),
            (&self.summary, "company_decision_summary_required"),
            (&self.published_by, "company_decision_publisher_required"),
        ] {
            required(value, field)?;
        }
        if !self.source_event_ref.starts_with("event:")
            || !self.closing_receipt_ref.starts_with("receipt:")
        {
            return Err("company_decision_source_ref_invalid");
        }
        refs(&self.evidence_refs, "company_decision_evidence_required")?;
        for evidence in &self.evidence_refs {
            required(evidence, "company_decision_evidence_invalid")?;
        }
        digest(&self.digest, "company_decision_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_decision_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "decision_id": self.decision_id,
            "project_id": self.project_id,
            "department_id": self.department_id,
            "kind": self.kind,
            "source_event_ref": self.source_event_ref,
            "closing_receipt_ref": self.closing_receipt_ref,
            "summary": self.summary,
            "evidence_refs": self.evidence_refs,
            "published_by": self.published_by,
            "published_at": self.published_at,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyKnowledgeScope {
    Department,
    Role,
    Project,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyCandidateState {
    Candidate,
    Approved,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyLessonCandidate {
    pub schema: String,
    pub candidate_id: String,
    pub decision_id: String,
    pub project_id: String,
    pub department_id: String,
    pub role_id: String,
    pub scope: CompanyKnowledgeScope,
    pub collection: String,
    pub kind: String,
    pub text: String,
    pub evidence_refs: Vec<String>,
    pub proposed_by: String,
    pub proposed_at: u64,
    pub state: CompanyCandidateState,
    #[serde(default)]
    pub reviewed_by: Option<String>,
    #[serde(default)]
    pub review_reason: Option<String>,
    pub digest: String,
}

impl CompanyLessonCandidate {
    pub fn validate_against(&self, decision: &CompanyDecisionRecord) -> Result<(), &'static str> {
        decision.validate()?;
        if self.schema != COMPANY_KNOWLEDGE_SCHEMA
            || self.decision_id != decision.decision_id
            || self.project_id != decision.project_id
            || self.department_id != decision.department_id
            || self.proposed_at == 0
            || !matches!(self.kind.as_str(), "lesson" | "decision")
        {
            return Err("company_candidate_binding_invalid");
        }
        for (value, field) in [
            (&self.candidate_id, "company_candidate_id_required"),
            (&self.role_id, "company_candidate_role_required"),
            (&self.collection, "company_candidate_collection_required"),
            (&self.text, "company_candidate_text_required"),
            (&self.proposed_by, "company_candidate_proposer_required"),
        ] {
            required(value, field)?;
        }
        if self.text.len() > 8_192 {
            return Err("company_candidate_text_too_large");
        }
        refs(&self.evidence_refs, "company_candidate_evidence_required")?;
        if self
            .evidence_refs
            .iter()
            .any(|reference| !decision.evidence_refs.contains(reference))
        {
            return Err("company_candidate_evidence_not_in_decision");
        }
        let expected_collection = match self.scope {
            CompanyKnowledgeScope::Department => format!("department:{}", self.department_id),
            CompanyKnowledgeScope::Role => format!("role:{}", self.role_id),
            CompanyKnowledgeScope::Project => format!("project:{}", self.project_id),
        };
        if self.collection != expected_collection
            || self.collection.starts_with("user:")
            || self.collection == "instance-scratch"
            || self.collection.contains("private")
        {
            return Err("company_candidate_collection_forbidden");
        }
        if self.state != CompanyCandidateState::Candidate
            || self.reviewed_by.is_some()
            || self.review_reason.is_some()
        {
            return Err("company_candidate_initial_state_invalid");
        }
        digest(&self.digest, "company_candidate_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_candidate_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "candidate_id": self.candidate_id,
            "decision_id": self.decision_id,
            "project_id": self.project_id,
            "department_id": self.department_id,
            "role_id": self.role_id,
            "scope": self.scope,
            "collection": self.collection,
            "kind": self.kind,
            "text": self.text,
            "evidence_refs": self.evidence_refs,
            "proposed_by": self.proposed_by,
            "proposed_at": self.proposed_at,
            "state": self.state,
            "reviewed_by": self.reviewed_by,
            "review_reason": self.review_reason,
        }))
    }

    fn reviewed(
        &self,
        state: CompanyCandidateState,
        reviewer: &str,
        reason: &str,
    ) -> Result<Self, &'static str> {
        self.validate_for_review()?;
        if reviewer == self.proposed_by {
            return Err("company_candidate_self_review_forbidden");
        }
        required(reviewer, "company_candidate_reviewer_required")?;
        required(reason, "company_candidate_review_reason_required")?;
        let mut next = self.clone();
        next.state = state;
        next.reviewed_by = Some(reviewer.to_owned());
        next.review_reason = Some(reason.to_owned());
        next.digest = next.canonical_digest();
        Ok(next)
    }

    fn validate_for_review(&self) -> Result<(), &'static str> {
        if self.state != CompanyCandidateState::Candidate
            || self.reviewed_by.is_some()
            || self.review_reason.is_some()
        {
            return Err("company_candidate_already_reviewed");
        }
        digest(&self.digest, "company_candidate_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_candidate_digest_mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyKnowledgePromotion {
    pub schema: String,
    pub promotion_id: String,
    pub candidate_id: String,
    pub project_id: String,
    pub collection: String,
    pub memory_record_ref: String,
    pub approved_by: String,
    pub evidence_refs: Vec<String>,
    pub promoted_at: u64,
    pub digest: String,
}

impl CompanyKnowledgePromotion {
    fn validate_against(
        &self,
        candidate: &CompanyLessonCandidate,
        decision: &CompanyDecisionRecord,
    ) -> Result<(), &'static str> {
        if self.schema != COMPANY_KNOWLEDGE_SCHEMA
            || candidate.state != CompanyCandidateState::Approved
            || self.candidate_id != candidate.candidate_id
            || self.project_id != candidate.project_id
            || self.collection != candidate.collection
            || self.approved_by != candidate.reviewed_by.as_deref().unwrap_or_default()
            || self.promoted_at == 0
        {
            return Err("company_promotion_binding_invalid");
        }
        decision.validate()?;
        refs(&self.evidence_refs, "company_promotion_evidence_required")?;
        if self
            .evidence_refs
            .iter()
            .any(|reference| !decision.evidence_refs.contains(reference))
        {
            return Err("company_promotion_evidence_not_in_decision");
        }
        required(&self.promotion_id, "company_promotion_id_required")?;
        required(&self.memory_record_ref, "company_promotion_record_required")?;
        digest(&self.digest, "company_promotion_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_promotion_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "promotion_id": self.promotion_id,
            "candidate_id": self.candidate_id,
            "project_id": self.project_id,
            "collection": self.collection,
            "memory_record_ref": self.memory_record_ref,
            "approved_by": self.approved_by,
            "evidence_refs": self.evidence_refs,
            "promoted_at": self.promoted_at,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompanyKnowledgeLedger {
    pub decisions: BTreeMap<String, CompanyDecisionRecord>,
    pub candidates: BTreeMap<String, CompanyLessonCandidate>,
    pub promotions: BTreeMap<String, CompanyKnowledgePromotion>,
}

impl CompanyKnowledgeLedger {
    pub fn publish_decision(
        &mut self,
        decision: CompanyDecisionRecord,
    ) -> Result<(), &'static str> {
        decision.validate()?;
        if let Some(existing) = self.decisions.get(&decision.decision_id) {
            if existing.digest == decision.digest {
                return Ok(());
            }
            return Err("company_decision_duplicate_digest_mismatch");
        }
        self.decisions
            .insert(decision.decision_id.clone(), decision);
        Ok(())
    }

    pub fn propose_candidate(
        &mut self,
        candidate: CompanyLessonCandidate,
    ) -> Result<(), &'static str> {
        let decision = self
            .decisions
            .get(&candidate.decision_id)
            .ok_or("company_decision_not_found")?;
        candidate.validate_against(decision)?;
        if let Some(existing) = self.candidates.get(&candidate.candidate_id) {
            if existing.digest == candidate.digest {
                return Ok(());
            }
            return Err("company_candidate_duplicate_digest_mismatch");
        }
        self.candidates
            .insert(candidate.candidate_id.clone(), candidate);
        Ok(())
    }

    pub fn review_candidate(
        &mut self,
        candidate_id: &str,
        approve: bool,
        reviewer: &str,
        reason: &str,
    ) -> Result<(), &'static str> {
        let candidate = self
            .candidates
            .get(candidate_id)
            .ok_or("company_candidate_not_found")?
            .clone();
        let next = candidate.reviewed(
            if approve {
                CompanyCandidateState::Approved
            } else {
                CompanyCandidateState::Rejected
            },
            reviewer,
            reason,
        )?;
        self.candidates.insert(candidate_id.to_owned(), next);
        Ok(())
    }

    pub fn promote(&mut self, promotion: CompanyKnowledgePromotion) -> Result<(), &'static str> {
        let candidate = self
            .candidates
            .get(&promotion.candidate_id)
            .ok_or("company_candidate_not_found")?;
        let decision = self
            .decisions
            .get(&candidate.decision_id)
            .ok_or("company_decision_not_found")?;
        promotion.validate_against(candidate, decision)?;
        if let Some(existing) = self.promotions.get(&promotion.promotion_id) {
            if existing.digest == promotion.digest {
                return Ok(());
            }
            return Err("company_promotion_duplicate_digest_mismatch");
        }
        self.promotions
            .insert(promotion.promotion_id.clone(), promotion);
        Ok(())
    }
}
