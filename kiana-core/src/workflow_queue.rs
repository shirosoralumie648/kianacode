//! ControlPlane-side queue lease admission helpers.
//!
//! The queue store owns atomic claim transitions, while this module keeps the dispatch boundary
//! explicit: a queue lease is not a capability permit and cannot call the Broker by itself.

use kiana_domain::{WorkflowQueueEffectState, WorkflowQueueLease, WorkflowQueueLeaseStatus};

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
