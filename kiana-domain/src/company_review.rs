//! Independent reviewer assignment and per-criterion evidence contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_REVIEW_ASSIGNMENT_SCHEMA: &str = "kiana.company-review-assignment.v1";
pub const COMPANY_INDEPENDENT_REVIEW_SCHEMA: &str = "kiana.company-independent-review.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerAssignment {
    pub schema: String,
    pub assignment_id: String,
    pub project_id: String,
    pub packet_id: String,
    pub packet_version: u64,
    pub baseline_version: u64,
    pub author_principal_ids: Vec<String>,
    pub author_session_ids: Vec<String>,
    pub reviewer_principal_id: String,
    pub reviewer_session_id: String,
    pub reviewer_role: String,
    pub reviewer_role_instance_id: String,
    pub criteria_snapshot_digest: String,
    pub evidence_bundle_digest: String,
    pub digest: String,
}

impl ReviewerAssignment {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_REVIEW_ASSIGNMENT_SCHEMA
            || self.packet_version == 0
            || self.baseline_version == 0
            || self.reviewer_role != "reviewer"
        {
            return Err("company_reviewer_assignment_invalid");
        }
        for (value, field) in [
            (
                &self.assignment_id,
                "company_reviewer_assignment_id_required",
            ),
            (&self.project_id, "company_reviewer_project_required"),
            (&self.packet_id, "company_reviewer_packet_required"),
            (
                &self.reviewer_principal_id,
                "company_reviewer_principal_required",
            ),
            (
                &self.reviewer_session_id,
                "company_reviewer_session_required",
            ),
            (
                &self.reviewer_role_instance_id,
                "company_reviewer_role_instance_required",
            ),
        ] {
            required(value, field)?;
        }
        if self.author_principal_ids.is_empty() || self.author_session_ids.is_empty() {
            return Err("company_reviewer_authors_required");
        }
        if self
            .author_session_ids
            .iter()
            .any(|session| session == &self.reviewer_session_id)
        {
            return Err("company_reviewer_session_not_independent");
        }
        if self
            .author_session_ids
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != self.author_session_ids.len()
        {
            return Err("company_reviewer_author_session_duplicate");
        }
        digest(
            &self.criteria_snapshot_digest,
            "company_reviewer_criteria_digest_invalid",
        )?;
        digest(
            &self.evidence_bundle_digest,
            "company_reviewer_evidence_digest_invalid",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("company_reviewer_assignment_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "assignment_id": self.assignment_id,
            "project_id": self.project_id,
            "packet_id": self.packet_id,
            "packet_version": self.packet_version,
            "baseline_version": self.baseline_version,
            "author_principal_ids": self.author_principal_ids,
            "author_session_ids": self.author_session_ids,
            "reviewer_principal_id": self.reviewer_principal_id,
            "reviewer_session_id": self.reviewer_session_id,
            "reviewer_role": self.reviewer_role,
            "reviewer_role_instance_id": self.reviewer_role_instance_id,
            "criteria_snapshot_digest": self.criteria_snapshot_digest,
            "evidence_bundle_digest": self.evidence_bundle_digest,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Pass,
    Fail,
    InsufficientEvidence,
    NotApplicable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionReviewResult {
    pub verdict: ReviewVerdict,
    pub evidence_refs: Vec<String>,
    pub reason: String,
    pub waiver_ref: Option<String>,
}

impl CriterionReviewResult {
    fn validate(&self) -> Result<(), &'static str> {
        required(&self.reason, "company_review_reason_required")?;
        if self.evidence_refs.is_empty()
            || self.evidence_refs.iter().any(|reference| {
                !reference.starts_with("evidence:") && !reference.starts_with("artifact:")
            })
        {
            return Err("company_review_evidence_refs_required");
        }
        if self.verdict == ReviewVerdict::NotApplicable && self.waiver_ref.is_none() {
            return Err("company_review_not_applicable_waiver_required");
        }
        if self.waiver_ref.as_deref().is_some_and(|reference| {
            !reference.starts_with("waiver:") || reference.len() <= "waiver:".len()
        }) {
            return Err("company_review_waiver_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndependentReview {
    pub schema: String,
    pub review_id: String,
    pub assignment: ReviewerAssignment,
    pub criterion_results: BTreeMap<String, CriterionReviewResult>,
    pub completed_at: u64,
    pub digest: String,
}

impl IndependentReview {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_INDEPENDENT_REVIEW_SCHEMA || self.completed_at == 0 {
            return Err("company_independent_review_invalid");
        }
        required(&self.review_id, "company_review_id_required")?;
        self.assignment.validate()?;
        if self.criterion_results.is_empty()
            || self
                .criterion_results
                .keys()
                .any(|criterion| criterion.trim().is_empty())
        {
            return Err("company_review_criteria_required");
        }
        for result in self.criterion_results.values() {
            result.validate()?;
        }
        if self.digest != self.canonical_digest() {
            return Err("company_independent_review_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "review_id": self.review_id,
            "assignment": self.assignment,
            "criterion_results": self.criterion_results,
            "completed_at": self.completed_at,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndependentReviewLedger {
    pub reviews: BTreeMap<String, IndependentReview>,
}

impl IndependentReviewLedger {
    pub fn record(&mut self, review: IndependentReview) -> Result<(), &'static str> {
        review.validate()?;
        if let Some(existing) = self.reviews.get(&review.review_id) {
            if existing.digest == review.digest {
                return Ok(());
            }
            return Err("company_review_duplicate_digest_mismatch");
        }
        self.reviews.insert(review.review_id.clone(), review);
        Ok(())
    }

    pub fn get(&self, review_id: &str) -> Option<&IndependentReview> {
        self.reviews.get(review_id)
    }
}
