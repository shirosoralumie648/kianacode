//! Shared WorkPacket/workflow queue claim contract.
//!
//! This is a pure admission shape for AUT-07. It does not persist a queue, acquire a lease or
//! dispatch a capability; AUT-08 owns the durable queue/heartbeat/fence implementation.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const WORKFLOW_QUEUE_CLAIM_SCHEMA: &str = "kiana.workflow-queue-claim.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowQueueClaimStatus {
    Ready,
    Claimed,
    Expired,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueClaimContract {
    pub schema: String,
    pub item_id: String,
    pub work_packet_digest: String,
    pub parent_scope_digest: Option<String>,
    pub item_scope_digest: String,
    pub parent_budget_digest: Option<String>,
    pub item_budget_digest: String,
    pub path_lock_digest: String,
    pub dependencies_resolved: bool,
    pub dependency_cycle: bool,
    pub scope_intersection_valid: bool,
    pub budget_subset_valid: bool,
    pub active_claim_count: u32,
    pub max_parallel_claims: u32,
    pub duplicate_claim: bool,
    pub claim_owner: Option<String>,
    pub claim_expires_at_unix_ms: u64,
    pub observed_at_unix_ms: u64,
    pub status: WorkflowQueueClaimStatus,
    pub claim_digest: String,
}

impl WorkflowQueueClaimContract {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        item_id: impl Into<String>,
        work_packet_digest: impl Into<String>,
        parent_scope_digest: Option<String>,
        item_scope_digest: impl Into<String>,
        parent_budget_digest: Option<String>,
        item_budget_digest: impl Into<String>,
        path_lock_digest: impl Into<String>,
        dependencies_resolved: bool,
        dependency_cycle: bool,
        scope_intersection_valid: bool,
        budget_subset_valid: bool,
        active_claim_count: u32,
        max_parallel_claims: u32,
        duplicate_claim: bool,
        claim_owner: Option<String>,
        claim_expires_at_unix_ms: u64,
        observed_at_unix_ms: u64,
        status: WorkflowQueueClaimStatus,
    ) -> Result<Self, String> {
        let mut contract = Self {
            schema: WORKFLOW_QUEUE_CLAIM_SCHEMA.to_owned(),
            item_id: item_id.into(),
            work_packet_digest: work_packet_digest.into(),
            parent_scope_digest,
            item_scope_digest: item_scope_digest.into(),
            parent_budget_digest,
            item_budget_digest: item_budget_digest.into(),
            path_lock_digest: path_lock_digest.into(),
            dependencies_resolved,
            dependency_cycle,
            scope_intersection_valid,
            budget_subset_valid,
            active_claim_count,
            max_parallel_claims,
            duplicate_claim,
            claim_owner,
            claim_expires_at_unix_ms,
            observed_at_unix_ms,
            status,
            claim_digest: String::new(),
        };
        contract.claim_digest = contract.digest();
        contract.validate()?;
        Ok(contract)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_QUEUE_CLAIM_SCHEMA {
            return Err("workflow_queue_claim_schema_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.item_id, "workflow_queue_item_id", 256),
            (&self.claim_digest, "workflow_queue_claim_digest", 512),
        ] {
            bounded(value, field, max)?;
        }
        for (value, field) in [
            (&self.work_packet_digest, "workflow_queue_packet_digest"),
            (&self.item_scope_digest, "workflow_queue_item_scope_digest"),
            (
                &self.item_budget_digest,
                "workflow_queue_item_budget_digest",
            ),
            (&self.path_lock_digest, "workflow_queue_path_lock_digest"),
        ] {
            digest(value, field)?;
        }
        for (value, field) in [
            (
                &self.parent_scope_digest,
                "workflow_queue_parent_scope_digest",
            ),
            (
                &self.parent_budget_digest,
                "workflow_queue_parent_budget_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        if self.parent_scope_digest.is_none() {
            return Err("workflow_queue_parent_scope_required".to_owned());
        }
        if self.parent_budget_digest.is_none() {
            return Err("workflow_queue_parent_budget_required".to_owned());
        }
        if self.max_parallel_claims == 0 || self.active_claim_count > self.max_parallel_claims {
            return Err("workflow_queue_parallel_limit_exceeded".to_owned());
        }
        if self.dependency_cycle {
            return Err("workflow_queue_dependency_cycle".to_owned());
        }
        if !self.dependencies_resolved {
            return Err("workflow_queue_dependency_unresolved".to_owned());
        }
        if !self.scope_intersection_valid {
            return Err("workflow_queue_scope_intersection_invalid".to_owned());
        }
        if !self.budget_subset_valid {
            return Err("workflow_queue_budget_subset_invalid".to_owned());
        }
        if self.duplicate_claim {
            return Err("workflow_queue_duplicate_claim".to_owned());
        }
        if self.observed_at_unix_ms == 0
            || self.claim_expires_at_unix_ms <= self.observed_at_unix_ms
        {
            return Err("workflow_queue_claim_expired".to_owned());
        }
        if self.status == WorkflowQueueClaimStatus::Claimed
            && self.claim_owner.as_deref().is_none_or(str::is_empty)
        {
            return Err("workflow_queue_claim_owner_required".to_owned());
        }
        if self.status == WorkflowQueueClaimStatus::Ready && self.claim_owner.is_some() {
            return Err("workflow_queue_ready_claim_owner_invalid".to_owned());
        }
        if self.claim_digest != self.digest() {
            return Err("workflow_queue_claim_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "item_id": self.item_id,
            "work_packet_digest": self.work_packet_digest,
            "parent_scope_digest": self.parent_scope_digest,
            "item_scope_digest": self.item_scope_digest,
            "parent_budget_digest": self.parent_budget_digest,
            "item_budget_digest": self.item_budget_digest,
            "path_lock_digest": self.path_lock_digest,
            "dependencies_resolved": self.dependencies_resolved,
            "dependency_cycle": self.dependency_cycle,
            "scope_intersection_valid": self.scope_intersection_valid,
            "budget_subset_valid": self.budget_subset_valid,
            "active_claim_count": self.active_claim_count,
            "max_parallel_claims": self.max_parallel_claims,
            "duplicate_claim": self.duplicate_claim,
            "claim_owner": self.claim_owner,
            "claim_expires_at_unix_ms": self.claim_expires_at_unix_ms,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "status": self.status,
        }))
    }
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
