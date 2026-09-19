//! ControlPlane-side queue lease admission helpers.
//!
//! The queue store owns atomic claim transitions, while this module keeps the dispatch boundary
//! explicit: a queue lease is not a capability permit and cannot call the Broker by itself.

use kiana_domain::{
    json_digest, ready_packets, WorkPacket, WorkflowQueueClaimContract, WorkflowQueueClaimStatus,
    WorkflowQueueEffectState, WorkflowQueueLease, WorkflowQueueLeaseStatus,
    WORKFLOW_QUEUE_LEASE_TTL_MAX_MS,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowQueueReadyView {
    pub claims: Vec<WorkflowQueueClaimContract>,
    pub blocked: BTreeMap<String, String>,
    pub expired_claims: Vec<String>,
    pub ready_digest: String,
}

impl WorkflowQueueReadyView {
    pub fn validate(&self) -> Result<(), String> {
        let mut item_ids = BTreeSet::new();
        for claim in &self.claims {
            claim.validate()?;
            if !claim.is_ready_packet() || !item_ids.insert(claim.item_id.clone()) {
                return Err("workflow_queue_ready_duplicate_item".to_owned());
            }
        }
        if self.ready_digest != self.digest() {
            return Err("workflow_queue_ready_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "claims": self.claims,
            "blocked": self.blocked,
            "expired_claims": self.expired_claims,
        }))
    }
}

/// Project the queue-ready view from the same WorkPacket readiness predicate used by the packet
/// board. This is intentionally a pure view: it creates no lease, writes no EventLog fact and
/// does not dispatch a capability. AUT-08 owns the atomic queue transition.
pub fn project_workflow_queue_ready(
    packets: &BTreeMap<String, WorkPacket>,
    now_unix_ms: u64,
    max_parallel_claims: u32,
) -> Result<WorkflowQueueReadyView, String> {
    if now_unix_ms == 0 || max_parallel_claims == 0 {
        return Err("workflow_queue_ready_input_invalid".to_owned());
    }
    let readiness = ready_packets(packets, now_unix_ms).map_err(|error| error.to_string())?;
    let mut claims = Vec::new();
    for item_id in &readiness.ready {
        let packet = packets
            .get(item_id)
            .ok_or_else(|| "workflow_queue_ready_packet_missing".to_owned())?;
        let parent = packet
            .parent_packet_id
            .as_ref()
            .and_then(|parent_id| packets.get(parent_id))
            .unwrap_or(packet);
        let claim = WorkflowQueueClaimContract::from_work_packet(
            packet,
            parent,
            true,
            false,
            0,
            max_parallel_claims,
            false,
            None,
            now_unix_ms
                .checked_add(WORKFLOW_QUEUE_LEASE_TTL_MAX_MS)
                .ok_or_else(|| "workflow_queue_ready_expiry_overflow".to_owned())?,
            now_unix_ms,
            WorkflowQueueClaimStatus::Ready,
        )?;
        claims.push(claim);
    }
    claims.sort_by(|left, right| left.item_id.cmp(&right.item_id));
    let mut view = WorkflowQueueReadyView {
        claims,
        blocked: readiness.blocked,
        expired_claims: readiness.expired_claims,
        ready_digest: String::new(),
    };
    view.ready_digest = view.digest();
    view.validate()?;
    Ok(view)
}

/// Validate the exact lease/fence/authority tuple immediately before a queue item is handed to
/// the existing ControlPlane command path.
pub fn validate_workflow_queue_dispatch(
    lease: &WorkflowQueueLease,
    owner_id: &str,
    fence_token: u64,
    authority_epoch: u64,
    now_unix_ms: u64,
) -> Result<(), String> {
    lease.is_dispatchable(owner_id, fence_token, authority_epoch, now_unix_ms)
}

/// Unknown effect is a recovery state, never a reclaimable ready item.
pub fn workflow_queue_requires_recovery(lease: &WorkflowQueueLease) -> bool {
    lease.status == WorkflowQueueLeaseStatus::ResultUnknown
        || lease.effect_state == WorkflowQueueEffectState::ResultUnknown
}
