//! SW-13 deterministic swarm merge reducer and complete partition coverage contract.
//!
//! Merge is pure evidence reduction. It rejects unordered/duplicate/missing partitions, Unknown
//! results and first-success semantics unless an explicit partial policy and evidence are bound.

use crate::{json_digest, SwarmPlanId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SWARM_MERGE_REDUCER_SCHEMA: &str = "kiana.swarm-merge-reducer.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmMergeStrategy {
    AllSuccess,
    AllSettled,
    ExplicitPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmMergePartitionResult {
    Success,
    Failed,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmMergePartition {
    pub partition_key: String,
    pub result: SwarmMergePartitionResult,
    pub output_digest: Option<String>,
    pub evidence_digest: String,
    pub partition_digest: String,
}

impl SwarmMergePartition {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_text(&self.partition_key)
            || !valid_digest(&self.evidence_digest)
            || !valid_digest(&self.partition_digest)
            || self.partition_digest != self.digest()
            || self
                .output_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
        {
            return Err("swarm_merge_partition_invalid".to_owned());
        }
        match self.result {
            SwarmMergePartitionResult::Success if self.output_digest.is_none() => {
                Err("swarm_merge_success_output_required".to_owned())
            }
            SwarmMergePartitionResult::Failed if self.output_digest.is_some() => {
                Err("swarm_merge_failed_output_forbidden".to_owned())
            }
            SwarmMergePartitionResult::ResultUnknown => {
                Err("swarm_merge_result_unknown_forbidden".to_owned())
            }
            _ => Ok(()),
        }
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "partition_key": self.partition_key,
            "result": self.result,
            "output_digest": self.output_digest,
            "evidence_digest": self.evidence_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmMergeDecision {
    pub schema: String,
    pub swarm_plan_id: SwarmPlanId,
    pub strategy: SwarmMergeStrategy,
    pub expected_partition_keys: Vec<String>,
    pub partitions: Vec<SwarmMergePartition>,
    pub policy_digest: Option<String>,
    pub acceptance_evidence_digest: Option<String>,
    pub accepted: bool,
    pub merge_digest: String,
}

impl SwarmMergeDecision {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_MERGE_REDUCER_SCHEMA
            || self.swarm_plan_id.as_uuid().is_nil()
            || self.expected_partition_keys.is_empty()
            || self
                .expected_partition_keys
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || has_duplicates(&self.expected_partition_keys)
            || self.partitions.len() != self.expected_partition_keys.len()
            || !valid_digest(&self.merge_digest)
            || self.merge_digest != self.digest()
        {
            return Err("swarm_merge_decision_invalid".to_owned());
        }
        let mut seen = BTreeSet::new();
        for (index, partition) in self.partitions.iter().enumerate() {
            partition.validate()?;
            if !seen.insert(partition.partition_key.clone())
                || partition.partition_key != self.expected_partition_keys[index]
            {
                return Err("swarm_merge_partition_coverage_invalid".to_owned());
            }
        }
        let has_unknown = self
            .partitions
            .iter()
            .any(|partition| partition.result == SwarmMergePartitionResult::ResultUnknown);
        if has_unknown {
            return Err("swarm_merge_is_unknown".to_owned());
        }
        match self.strategy {
            SwarmMergeStrategy::AllSuccess => {
                if !self.accepted
                    || self
                        .partitions
                        .iter()
                        .any(|partition| partition.result != SwarmMergePartitionResult::Success)
                    || self.policy_digest.is_some()
                    || self.acceptance_evidence_digest.is_some()
                {
                    return Err("swarm_merge_all_success_invalid".to_owned());
                }
            }
            SwarmMergeStrategy::AllSettled => {
                if self.accepted
                    || self.policy_digest.is_some()
                    || self.acceptance_evidence_digest.is_some()
                {
                    return Err("swarm_merge_all_settled_requires_review".to_owned());
                }
            }
            SwarmMergeStrategy::ExplicitPolicy => {
                if self
                    .policy_digest
                    .as_deref()
                    .is_none_or(|value| !valid_digest(value))
                    || self
                        .acceptance_evidence_digest
                        .as_deref()
                        .is_none_or(|value| !valid_digest(value))
                {
                    return Err("swarm_merge_explicit_policy_evidence_required".to_owned());
                }
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "swarm_plan_id": self.swarm_plan_id,
            "strategy": self.strategy,
            "expected_partition_keys": self.expected_partition_keys,
            "partitions": self.partitions,
            "policy_digest": self.policy_digest,
            "acceptance_evidence_digest": self.acceptance_evidence_digest,
            "accepted": self.accepted,
        }))
    }
}

pub fn validate_swarm_merge_decision(decision: &SwarmMergeDecision) -> Result<(), String> {
    decision.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.contains(['\0', '\r', '\n'])
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
