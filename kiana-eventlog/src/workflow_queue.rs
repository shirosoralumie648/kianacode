//! CI-semantics workflow queue adapter.
//!
//! This adapter provides one atomic in-process state transition for queue claims.  It is useful
//! for exercising the deny/reclaim/Unknown contract in GitHub CI; it deliberately does not claim
//! fsync, process recovery or cross-process fencing.  A durable implementation must use the same
//! domain transitions behind an EventLog-backed store.

use async_trait::async_trait;
use kiana_domain::{
    WorkflowQueueClaimContract, WorkflowQueueClaimRequest, WorkflowQueueClaimStatus,
    WorkflowQueueEffectRequest, WorkflowQueueEffectState, WorkflowQueueFenceRequest,
    WorkflowQueueHeartbeatRequest, WorkflowQueueLease, WorkflowQueueLeaseStatus,
    WorkflowQueueReclaimRequest, WORKFLOW_QUEUE_LEASE_TTL_MAX_MS,
};
use kiana_ports::{PortError, WorkflowQueueStore};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
struct QueueEntry {
    claim: WorkflowQueueClaimContract,
    lease: Option<WorkflowQueueLease>,
}

#[derive(Clone, Debug, Default)]
struct QueueState {
    entries: BTreeMap<String, QueueEntry>,
}

/// In-process queue adapter for CI and deterministic source behavior checks.
#[derive(Clone, Debug, Default)]
pub struct MemoryWorkflowQueueStore {
    state: Arc<Mutex<QueueState>>,
}

impl MemoryWorkflowQueueStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a validated ready item in the queue without claiming it.
    pub async fn enqueue(&self, claim: WorkflowQueueClaimContract) -> Result<(), PortError> {
        claim.validate().map_err(PortError::Failed)?;
        if claim.status != WorkflowQueueClaimStatus::Ready {
            return Err(PortError::Failed(
                "workflow_queue_enqueue_requires_ready".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        if state.entries.contains_key(&claim.item_id) {
            return Err(PortError::Conflict(
                "workflow_queue_item_duplicate".to_owned(),
            ));
        }
        state
            .entries
            .insert(claim.item_id.clone(), QueueEntry { claim, lease: None });
        Ok(())
    }

    fn expiry(now: u64, ttl_ms: u64) -> Result<u64, PortError> {
        if now == 0 || ttl_ms == 0 || ttl_ms > WORKFLOW_QUEUE_LEASE_TTL_MAX_MS {
            return Err(PortError::Failed(
                "workflow_queue_lease_ttl_invalid".to_owned(),
            ));
        }
        now.checked_add(ttl_ms)
            .ok_or_else(|| PortError::Failed("workflow_queue_lease_expiry_overflow".to_owned()))
    }

    fn item_id_valid(item_id: &str) -> Result<(), PortError> {
        if item_id.trim().is_empty() || item_id.len() > 256 || item_id.contains(['\0', '\n', '\r'])
        {
            return Err(PortError::Failed(
                "workflow_queue_item_id_invalid".to_owned(),
            ));
        }
        Ok(())
    }

    fn owner_valid(owner_id: &str) -> Result<(), PortError> {
        if owner_id.trim().is_empty()
            || owner_id.len() > 256
            || owner_id.contains(['\0', '\n', '\r'])
        {
            return Err(PortError::Failed("workflow_queue_owner_invalid".to_owned()));
        }
        Ok(())
    }

    fn lease_error(reason: String) -> PortError {
        PortError::Conflict(reason)
    }

    async fn entry_for_claim(
        &self,
        request: &WorkflowQueueClaimRequest,
    ) -> Result<QueueEntry, PortError> {
        request.claim.validate().map_err(PortError::Failed)?;
        Self::item_id_valid(&request.claim.item_id)?;
        Self::owner_valid(&request.owner_id)?;
        if request.fence_token == 0 || request.authority_epoch == 0 {
            return Err(PortError::Failed(
                "workflow_queue_authority_or_fence_invalid".to_owned(),
            ));
        }
        let _ = Self::expiry(request.observed_at_unix_ms, request.lease_ttl_ms)?;
        if request.claim.status != WorkflowQueueClaimStatus::Ready {
            return Err(PortError::Failed(
                "workflow_queue_claim_not_ready".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        if let Some(entry) = state.entries.get(&request.claim.item_id) {
            if entry.claim.claim_digest != request.claim.claim_digest {
                return Err(PortError::Conflict(
                    "workflow_queue_claim_digest_conflict".to_owned(),
                ));
            }
            return Ok(entry.clone());
        }
        let entry = QueueEntry {
            claim: request.claim.clone(),
            lease: None,
        };
        state
            .entries
            .insert(request.claim.item_id.clone(), entry.clone());
        Ok(entry)
    }
}

#[async_trait]
impl WorkflowQueueStore for MemoryWorkflowQueueStore {
    async fn claim(
        &self,
        request: WorkflowQueueClaimRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        let entry = self.entry_for_claim(&request).await?;
        let expires_at = Self::expiry(request.observed_at_unix_ms, request.lease_ttl_ms)?
            .min(request.claim.claim_expires_at_unix_ms);
        if expires_at <= request.observed_at_unix_ms {
            return Err(PortError::Conflict(
                "workflow_queue_claim_expired".to_owned(),
            ));
        }
        if let Some(lease) = &entry.lease {
            if lease.status == WorkflowQueueLeaseStatus::ResultUnknown
                || lease.effect_state == WorkflowQueueEffectState::ResultUnknown
            {
                return Err(PortError::Conflict(
                    "workflow_queue_recovery_required".to_owned(),
                ));
            }
            if lease.effect_state == WorkflowQueueEffectState::Running {
                return Err(PortError::Conflict(
                    "workflow_queue_reclaim_effect_in_flight".to_owned(),
                ));
            }
            return Err(PortError::Conflict(
                if lease.is_expired(request.observed_at_unix_ms) {
                    "workflow_queue_reclaim_required"
                } else {
                    "workflow_queue_claim_contended"
                }
                .to_owned(),
            ));
        }
        let lease = WorkflowQueueLease::issue(
            &entry.claim,
            request.owner_id,
            request.fence_token,
            request.authority_epoch,
            request.observed_at_unix_ms,
            expires_at,
        )
        .map_err(Self::lease_error)?;
        let mut state = self.state.lock().await;
        let current = state
            .entries
            .get_mut(&lease.item_id)
            .ok_or_else(|| PortError::Failed("workflow_queue_item_lost".to_owned()))?;
        if current.lease.is_some() {
            return Err(PortError::Conflict(
                "workflow_queue_claim_contended".to_owned(),
            ));
        }
        current.lease = Some(lease.clone());
        Ok(lease)
    }

    async fn heartbeat(
        &self,
        request: WorkflowQueueHeartbeatRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        Self::item_id_valid(&request.item_id)?;
        Self::owner_valid(&request.owner_id)?;
        let expires_at = Self::expiry(request.observed_at_unix_ms, request.lease_ttl_ms)?;
        if request.fence_token == 0 || request.authority_epoch == 0 {
            return Err(PortError::Failed(
                "workflow_queue_authority_or_fence_invalid".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        let entry = state
            .entries
            .get_mut(&request.item_id)
            .ok_or_else(|| PortError::Conflict("workflow_queue_item_not_found".to_owned()))?;
        let current = entry
            .lease
            .as_ref()
            .ok_or_else(|| PortError::Conflict("workflow_queue_lease_not_found".to_owned()))?;
        let next = current
            .renew(
                &request.owner_id,
                request.fence_token,
                request.authority_epoch,
                request.observed_at_unix_ms,
                expires_at,
            )
            .map_err(Self::lease_error)?;
        entry.lease = Some(next.clone());
        Ok(next)
    }

    async fn record_effect(
        &self,
        request: WorkflowQueueEffectRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        Self::item_id_valid(&request.item_id)?;
        Self::owner_valid(&request.owner_id)?;
        let mut state = self.state.lock().await;
        let entry = state
            .entries
            .get_mut(&request.item_id)
            .ok_or_else(|| PortError::Conflict("workflow_queue_item_not_found".to_owned()))?;
        let current = entry
            .lease
            .as_ref()
            .ok_or_else(|| PortError::Conflict("workflow_queue_lease_not_found".to_owned()))?;
        let next = current
            .record_effect(
                &request.owner_id,
                request.fence_token,
                request.authority_epoch,
                request.observed_at_unix_ms,
                request.effect_state,
            )
            .map_err(Self::lease_error)?;
        entry.lease = Some(next.clone());
        Ok(next)
    }

    async fn fence(
        &self,
        request: WorkflowQueueFenceRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        Self::item_id_valid(&request.item_id)?;
        Self::owner_valid(&request.owner_id)?;
        let mut state = self.state.lock().await;
        let entry = state
            .entries
            .get_mut(&request.item_id)
            .ok_or_else(|| PortError::Conflict("workflow_queue_item_not_found".to_owned()))?;
        let current = entry
            .lease
            .as_ref()
            .ok_or_else(|| PortError::Conflict("workflow_queue_lease_not_found".to_owned()))?;
        let next = current
            .fence(
                &request.owner_id,
                request.fence_token,
                request.authority_epoch,
                request.observed_at_unix_ms,
            )
            .map_err(Self::lease_error)?;
        entry.lease = Some(next.clone());
        Ok(next)
    }

    async fn reclaim(
        &self,
        request: WorkflowQueueReclaimRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        Self::item_id_valid(&request.item_id)?;
        Self::owner_valid(&request.new_owner_id)?;
        let expires_at = Self::expiry(request.observed_at_unix_ms, request.lease_ttl_ms)?;
        if request.new_fence_token == 0 || request.authority_epoch == 0 {
            return Err(PortError::Failed(
                "workflow_queue_authority_or_fence_invalid".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        let entry = state
            .entries
            .get_mut(&request.item_id)
            .ok_or_else(|| PortError::Conflict("workflow_queue_item_not_found".to_owned()))?;
        let current = entry
            .lease
            .as_ref()
            .ok_or_else(|| PortError::Conflict("workflow_queue_lease_not_found".to_owned()))?;
        let next = current
            .reclaim(
                request.new_owner_id,
                request.new_fence_token,
                request.authority_epoch,
                request.observed_at_unix_ms,
                expires_at,
            )
            .map_err(Self::lease_error)?;
        entry.lease = Some(next.clone());
        Ok(next)
    }

    async fn ready(&self, limit: usize) -> Result<Vec<WorkflowQueueClaimContract>, PortError> {
        if limit == 0 || limit > 256 {
            return Err(PortError::Failed(
                "workflow_queue_ready_limit_invalid".to_owned(),
            ));
        }
        let state = self.state.lock().await;
        Ok(state
            .entries
            .values()
            .filter(|entry| entry.lease.is_none())
            .take(limit)
            .map(|entry| entry.claim.clone())
            .collect())
    }
}
