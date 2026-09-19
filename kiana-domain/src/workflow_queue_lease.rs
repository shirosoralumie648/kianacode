//! Lease, heartbeat, fencing and recovery contracts for the workflow queue.
//!
//! The domain contract is intentionally independent from a queue implementation.  A store may
//! persist these transitions in EventLog or use an in-memory adapter for CI, but no adapter may
//! turn an expired or uncertain lease into a new dispatch without the checks below.

use crate::{json_digest, WorkflowQueueClaimContract, WorkflowQueueClaimStatus};
use serde::{Deserialize, Serialize};

pub const WORKFLOW_QUEUE_LEASE_SCHEMA: &str = "kiana.workflow-queue-lease.v1";
pub const WORKFLOW_QUEUE_LEASE_TTL_MAX_MS: u64 = 300_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowQueueLeaseStatus {
    Active,
    Completed,
    Fenced,
    ResultUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowQueueEffectState {
    NotStarted,
    Running,
    Succeeded,
    Failed,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueLease {
    pub schema: String,
    pub item_id: String,
    pub claim_digest: String,
    pub owner_id: String,
    pub sequence: u64,
    pub fence_token: u64,
    pub authority_epoch: u64,
    pub issued_at_unix_ms: u64,
    pub heartbeat_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub effect_state: WorkflowQueueEffectState,
    pub status: WorkflowQueueLeaseStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_lease_digest: Option<String>,
    pub lease_digest: String,
}

impl WorkflowQueueLease {
    pub fn issue(
        claim: &WorkflowQueueClaimContract,
        owner_id: impl Into<String>,
        fence_token: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        claim.validate()?;
        if claim.status != WorkflowQueueClaimStatus::Ready {
            return Err("workflow_queue_claim_not_ready".to_owned());
        }
        let mut lease = Self {
            schema: WORKFLOW_QUEUE_LEASE_SCHEMA.to_owned(),
            item_id: claim.item_id.clone(),
            claim_digest: claim.claim_digest.clone(),
            owner_id: owner_id.into(),
            sequence: 1,
            fence_token,
            authority_epoch,
            issued_at_unix_ms: observed_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms,
            expires_at_unix_ms,
            effect_state: WorkflowQueueEffectState::NotStarted,
            status: WorkflowQueueLeaseStatus::Active,
            previous_lease_digest: None,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    pub fn is_expired(&self, now_unix_ms: u64) -> bool {
        now_unix_ms >= self.expires_at_unix_ms
    }

    pub fn is_dispatchable(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate_actor(owner_id, fence_token, authority_epoch)?;
        if self.status != WorkflowQueueLeaseStatus::Active {
            return Err("workflow_queue_lease_not_active".to_owned());
        }
        if self.effect_state != WorkflowQueueEffectState::NotStarted {
            return Err("workflow_queue_effect_already_started".to_owned());
        }
        if self.is_expired(now_unix_ms) {
            return Err("workflow_queue_lease_expired".to_owned());
        }
        Ok(())
    }

    pub fn renew(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate_actor(owner_id, fence_token, authority_epoch)?;
        if self.status != WorkflowQueueLeaseStatus::Active {
            return Err("workflow_queue_lease_not_active".to_owned());
        }
        if self.is_expired(observed_at_unix_ms) {
            return Err("workflow_queue_lease_expired".to_owned());
        }
        if observed_at_unix_ms < self.heartbeat_at_unix_ms {
            return Err("workflow_queue_heartbeat_rollback".to_owned());
        }
        if expires_at_unix_ms <= observed_at_unix_ms
            || expires_at_unix_ms <= self.heartbeat_at_unix_ms
            || expires_at_unix_ms.saturating_sub(observed_at_unix_ms)
                > WORKFLOW_QUEUE_LEASE_TTL_MAX_MS
        {
            return Err("workflow_queue_lease_expiry_invalid".to_owned());
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "workflow_queue_lease_sequence_exhausted".to_owned())?;
        let mut next = Self {
            schema: self.schema.clone(),
            item_id: self.item_id.clone(),
            claim_digest: self.claim_digest.clone(),
            owner_id: self.owner_id.clone(),
            sequence,
            fence_token: self.fence_token,
            authority_epoch: self.authority_epoch,
            issued_at_unix_ms: self.issued_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms,
            expires_at_unix_ms,
            effect_state: self.effect_state,
            status: self.status,
            previous_lease_digest: Some(self.lease_digest.clone()),
            lease_digest: String::new(),
        };
        next.lease_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn record_effect(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
        effect_state: WorkflowQueueEffectState,
    ) -> Result<Self, String> {
        self.validate_actor(owner_id, fence_token, authority_epoch)?;
        if self.status != WorkflowQueueLeaseStatus::Active {
            return Err("workflow_queue_lease_not_active".to_owned());
        }
        if self.is_expired(observed_at_unix_ms) {
            return Err("workflow_queue_lease_expired".to_owned());
        }
        if !effect_transition_allowed(self.effect_state, effect_state) {
            return Err("workflow_queue_effect_transition_invalid".to_owned());
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "workflow_queue_lease_sequence_exhausted".to_owned())?;
        let status = match effect_state {
            WorkflowQueueEffectState::Succeeded | WorkflowQueueEffectState::Failed => {
                WorkflowQueueLeaseStatus::Completed
            }
            WorkflowQueueEffectState::ResultUnknown => WorkflowQueueLeaseStatus::ResultUnknown,
            WorkflowQueueEffectState::NotStarted | WorkflowQueueEffectState::Running => {
                WorkflowQueueLeaseStatus::Active
            }
        };
        let mut next = Self {
            schema: self.schema.clone(),
            item_id: self.item_id.clone(),
            claim_digest: self.claim_digest.clone(),
            owner_id: self.owner_id.clone(),
            sequence,
            fence_token: self.fence_token,
            authority_epoch: self.authority_epoch,
            issued_at_unix_ms: self.issued_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms,
            expires_at_unix_ms: self.expires_at_unix_ms,
            effect_state,
            status,
            previous_lease_digest: Some(self.lease_digest.clone()),
            lease_digest: String::new(),
        };
        next.lease_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn fence(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate_actor(owner_id, fence_token, authority_epoch)?;
        if matches!(
            self.status,
            WorkflowQueueLeaseStatus::Completed | WorkflowQueueLeaseStatus::Fenced
        ) {
            return Err("workflow_queue_lease_already_terminal".to_owned());
        }
        let effect_state = if matches!(
            self.effect_state,
            WorkflowQueueEffectState::Running | WorkflowQueueEffectState::ResultUnknown
        ) {
            WorkflowQueueEffectState::ResultUnknown
        } else {
            self.effect_state
        };
        let status = if effect_state == WorkflowQueueEffectState::ResultUnknown {
            WorkflowQueueLeaseStatus::ResultUnknown
        } else {
            WorkflowQueueLeaseStatus::Fenced
        };
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "workflow_queue_lease_sequence_exhausted".to_owned())?;
        let mut next = Self {
            schema: self.schema.clone(),
            item_id: self.item_id.clone(),
            claim_digest: self.claim_digest.clone(),
            owner_id: self.owner_id.clone(),
            sequence,
            fence_token: self.fence_token,
            authority_epoch: self.authority_epoch,
            issued_at_unix_ms: self.issued_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms.max(self.heartbeat_at_unix_ms),
            expires_at_unix_ms: self.expires_at_unix_ms,
            effect_state,
            status,
            previous_lease_digest: Some(self.lease_digest.clone()),
            lease_digest: String::new(),
        };
        next.lease_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn reclaim(
        &self,
        new_owner_id: impl Into<String>,
        new_fence_token: u64,
        new_authority_epoch: u64,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate()?;
        if !self.is_expired(observed_at_unix_ms) {
            return Err("workflow_queue_lease_not_expired".to_owned());
        }
        if self.status != WorkflowQueueLeaseStatus::Active {
            return Err("workflow_queue_lease_not_reclaimable".to_owned());
        }
        if self.effect_state != WorkflowQueueEffectState::NotStarted {
            return Err("workflow_queue_reclaim_effect_in_flight".to_owned());
        }
        if new_fence_token <= self.fence_token {
            return Err("workflow_queue_fence_not_monotonic".to_owned());
        }
        if new_authority_epoch < self.authority_epoch {
            return Err("workflow_queue_authority_epoch_rollback".to_owned());
        }
        let new_owner_id = new_owner_id.into();
        bounded(&new_owner_id, "workflow_queue_owner", 256)?;
        if new_owner_id == self.owner_id {
            return Err("workflow_queue_reclaim_owner_reused".to_owned());
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "workflow_queue_lease_sequence_exhausted".to_owned())?;
        let mut next = Self {
            schema: self.schema.clone(),
            item_id: self.item_id.clone(),
            claim_digest: self.claim_digest.clone(),
            owner_id: new_owner_id,
            sequence,
            fence_token: new_fence_token,
            authority_epoch: new_authority_epoch,
            issued_at_unix_ms: observed_at_unix_ms,
            heartbeat_at_unix_ms: observed_at_unix_ms,
            expires_at_unix_ms,
            effect_state: WorkflowQueueEffectState::NotStarted,
            status: WorkflowQueueLeaseStatus::Active,
            previous_lease_digest: Some(self.lease_digest.clone()),
            lease_digest: String::new(),
        };
        next.lease_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn validate_actor(
        &self,
        owner_id: &str,
        fence_token: u64,
        authority_epoch: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.owner_id != owner_id {
            return Err("workflow_queue_owner_mismatch".to_owned());
        }
        if self.fence_token != fence_token {
            return Err("workflow_queue_fence_mismatch".to_owned());
        }
        if authority_epoch != self.authority_epoch {
            return Err(if authority_epoch < self.authority_epoch {
                "workflow_queue_authority_epoch_rollback"
            } else {
                "workflow_queue_authority_epoch_stale"
            }
            .to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_QUEUE_LEASE_SCHEMA
            || self.sequence == 0
            || self.fence_token == 0
            || self.authority_epoch == 0
            || self.issued_at_unix_ms == 0
            || self.heartbeat_at_unix_ms < self.issued_at_unix_ms
            || self.expires_at_unix_ms <= self.heartbeat_at_unix_ms
            || self
                .expires_at_unix_ms
                .saturating_sub(self.heartbeat_at_unix_ms)
                > WORKFLOW_QUEUE_LEASE_TTL_MAX_MS
        {
            return Err("workflow_queue_lease_header_invalid".to_owned());
        }
        bounded(&self.item_id, "workflow_queue_item_id", 256)?;
        bounded(&self.owner_id, "workflow_queue_owner", 256)?;
        digest(&self.claim_digest, "workflow_queue_claim_digest")?;
        if let Some(previous) = &self.previous_lease_digest {
            digest(previous, "workflow_queue_previous_lease_digest")?;
            if self.sequence == 1 {
                return Err("workflow_queue_genesis_parent_unexpected".to_owned());
            }
        } else if self.sequence != 1 {
            return Err("workflow_queue_lease_parent_required".to_owned());
        }
        if self.effect_state == WorkflowQueueEffectState::ResultUnknown
            && self.status != WorkflowQueueLeaseStatus::ResultUnknown
        {
            return Err("workflow_queue_unknown_effect_not_fenced".to_owned());
        }
        if self.status == WorkflowQueueLeaseStatus::ResultUnknown
            && self.effect_state != WorkflowQueueEffectState::ResultUnknown
        {
            return Err("workflow_queue_unknown_status_without_effect".to_owned());
        }
        if self.status == WorkflowQueueLeaseStatus::Completed
            && !matches!(
                self.effect_state,
                WorkflowQueueEffectState::Succeeded | WorkflowQueueEffectState::Failed
            )
        {
            return Err("workflow_queue_completed_effect_invalid".to_owned());
        }
        if self.status == WorkflowQueueLeaseStatus::Fenced
            && matches!(
                self.effect_state,
                WorkflowQueueEffectState::Running | WorkflowQueueEffectState::ResultUnknown
            )
        {
            return Err("workflow_queue_fenced_effect_unknown".to_owned());
        }
        if !valid_digest(&self.lease_digest) || self.lease_digest != self.digest() {
            return Err("workflow_queue_lease_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "item_id": self.item_id,
            "claim_digest": self.claim_digest,
            "owner_id": self.owner_id,
            "sequence": self.sequence,
            "fence_token": self.fence_token,
            "authority_epoch": self.authority_epoch,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "heartbeat_at_unix_ms": self.heartbeat_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "effect_state": self.effect_state,
            "status": self.status,
            "previous_lease_digest": self.previous_lease_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueClaimRequest {
    pub claim: WorkflowQueueClaimContract,
    pub owner_id: String,
    pub fence_token: u64,
    pub authority_epoch: u64,
    pub observed_at_unix_ms: u64,
    pub lease_ttl_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueHeartbeatRequest {
    pub item_id: String,
    pub owner_id: String,
    pub fence_token: u64,
    pub authority_epoch: u64,
    pub observed_at_unix_ms: u64,
    pub lease_ttl_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueEffectRequest {
    pub item_id: String,
    pub owner_id: String,
    pub fence_token: u64,
    pub authority_epoch: u64,
    pub observed_at_unix_ms: u64,
    pub effect_state: WorkflowQueueEffectState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueFenceRequest {
    pub item_id: String,
    pub owner_id: String,
    pub fence_token: u64,
    pub authority_epoch: u64,
    pub observed_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueReclaimRequest {
    pub item_id: String,
    pub new_owner_id: String,
    pub new_fence_token: u64,
    pub authority_epoch: u64,
    pub observed_at_unix_ms: u64,
    pub lease_ttl_ms: u64,
}

fn effect_transition_allowed(
    current: WorkflowQueueEffectState,
    next: WorkflowQueueEffectState,
) -> bool {
    matches!(
        (current, next),
        (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::Running
        ) | (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::Succeeded
        ) | (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::Failed
        ) | (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::ResultUnknown
        ) | (
            WorkflowQueueEffectState::Running,
            WorkflowQueueEffectState::Succeeded
        ) | (
            WorkflowQueueEffectState::Running,
            WorkflowQueueEffectState::Failed
        ) | (
            WorkflowQueueEffectState::Running,
            WorkflowQueueEffectState::ResultUnknown
        ) | (
            WorkflowQueueEffectState::Running,
            WorkflowQueueEffectState::Running
        ) | (
            WorkflowQueueEffectState::NotStarted,
            WorkflowQueueEffectState::NotStarted
        )
    )
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    if !valid_digest(value) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
