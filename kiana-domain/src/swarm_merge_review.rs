//! SW-14 independent swarm merge review, decisions and receipt contract.
//!
//! A swarm merge review is distinct from Company Project acceptance. Each partition receives one
//! immutable reviewer decision; reviewer/author self-review, duplicate decisions, stale merge
//! digests and unverified outputs are rejected.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SWARM_MERGE_REVIEW_SCHEMA: &str = "kiana.swarm-merge-review.v1";
pub const SWARM_MERGE_RECEIPT_SCHEMA: &str = "kiana.swarm-merge-receipt.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmReviewDecision {
    Accepted,
    Rejected,
    NeedsRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmPartitionReview {
    pub partition_key: String,
    pub author_id: String,
    pub reviewer_id: String,
    pub output_digest: String,
    pub evidence_digest: String,
    pub decision: SwarmReviewDecision,
    pub reason: String,
    pub review_digest: String,
}

impl SwarmPartitionReview {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_text(&self.partition_key)
            || !valid_text(&self.author_id)
            || !valid_text(&self.reviewer_id)
            || self.author_id == self.reviewer_id
            || !valid_digest(&self.output_digest)
            || !valid_digest(&self.evidence_digest)
            || !valid_text(&self.reason)
            || !valid_digest(&self.review_digest)
            || self.review_digest != self.digest()
        {
            return Err("swarm_partition_review_invalid".to_owned());
        }
        if self.decision == SwarmReviewDecision::Accepted && self.output_digest.is_empty() {
            return Err("swarm_review_output_unverified".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "partition_key": self.partition_key,
            "author_id": self.author_id,
            "reviewer_id": self.reviewer_id,
            "output_digest": self.output_digest,
            "evidence_digest": self.evidence_digest,
            "decision": self.decision,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmMergeReview {
    pub schema: String,
    pub merge_digest: String,
    pub expected_partition_keys: Vec<String>,
    pub reviews: Vec<SwarmPartitionReview>,
    pub policy_digest: String,
    pub reviewer_epoch: u64,
    pub review_digest: String,
}

impl SwarmMergeReview {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_MERGE_REVIEW_SCHEMA
            || !valid_digest(&self.merge_digest)
            || self.expected_partition_keys.is_empty()
            || self
                .expected_partition_keys
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || has_duplicates(&self.expected_partition_keys)
            || self.reviews.len() != self.expected_partition_keys.len()
            || !valid_digest(&self.policy_digest)
            || self.reviewer_epoch == 0
            || !valid_digest(&self.review_digest)
            || self.review_digest != self.digest()
        {
            return Err("swarm_merge_review_invalid".to_owned());
        }
        let mut seen = BTreeSet::new();
        for (index, review) in self.reviews.iter().enumerate() {
            review.validate()?;
            if !seen.insert(review.partition_key.clone())
                || review.partition_key != self.expected_partition_keys[index]
            {
                return Err("swarm_merge_review_partition_coverage_invalid".to_owned());
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "merge_digest": self.merge_digest,
            "expected_partition_keys": self.expected_partition_keys,
            "reviews": self.reviews,
            "policy_digest": self.policy_digest,
            "reviewer_epoch": self.reviewer_epoch,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmMergeReceipt {
    pub schema: String,
    pub merge_digest: String,
    pub review_digest: String,
    pub accepted_by: String,
    pub policy_digest: String,
    pub conflict_refs: Vec<String>,
    pub company_acceptance_required: bool,
    pub receipt_digest: String,
}

impl SwarmMergeReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_MERGE_RECEIPT_SCHEMA
            || !valid_digest(&self.merge_digest)
            || !valid_digest(&self.review_digest)
            || !valid_text(&self.accepted_by)
            || !valid_digest(&self.policy_digest)
            || self.conflict_refs.iter().any(|value| !valid_text(value))
            || !self.company_acceptance_required
            || !valid_digest(&self.receipt_digest)
            || self.receipt_digest != self.digest()
        {
            return Err("swarm_merge_receipt_invalid".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "merge_digest": self.merge_digest,
            "review_digest": self.review_digest,
            "accepted_by": self.accepted_by,
            "policy_digest": self.policy_digest,
            "conflict_refs": self.conflict_refs,
            "company_acceptance_required": self.company_acceptance_required,
        }))
    }
}

pub fn validate_swarm_merge_review(review: &SwarmMergeReview) -> Result<(), String> {
    review.validate()
}

pub fn validate_swarm_merge_receipt(receipt: &SwarmMergeReceipt) -> Result<(), String> {
    receipt.validate()
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
