//! Query adapter for immutable source-snapshot invalidation plans.

use kiana_domain::{
    DataPayloadState, DataPropagationPlan, DataPropagationTarget, IndexInvalidationPlan,
    WorkspaceSnapshot,
};

/// Query-side epoch fence. A stale or revoked index is never treated as an empty result: callers
/// must rebuild from the source snapshot before returning hits.
pub fn index_read_allowed(
    data_epoch: u64,
    current_data_epoch: u64,
    payload_state: DataPayloadState,
) -> Result<(), String> {
    if data_epoch == 0 || current_data_epoch == 0 || data_epoch != current_data_epoch {
        return Err("index_data_epoch_stale".to_owned());
    }
    if payload_state != DataPayloadState::Available {
        return Err("index_data_revoked_or_expired".to_owned());
    }
    Ok(())
}

/// Bind a source/index invalidation to the governance propagation plan. The plan is metadata only;
/// it cannot authorize reads or claim that a rebuild has completed.
pub fn governed_index_invalidation(
    plan: &DataPropagationPlan,
    current_data_epoch: u64,
) -> Result<bool, String> {
    plan.validate()?;
    if current_data_epoch < plan.data_epoch {
        return Err("index_governance_epoch_ahead".to_owned());
    }
    let target = plan
        .targets
        .iter()
        .find(|receipt| receipt.target == DataPropagationTarget::Index)
        .ok_or_else(|| "index_propagation_target_missing".to_owned())?;
    Ok(
        target.state == kiana_domain::DataPropagationState::Invalidated
            || target.state == kiana_domain::DataPropagationState::Unknown,
    )
}

pub fn plan_index_invalidation(
    previous: Option<&WorkspaceSnapshot>,
    current: &WorkspaceSnapshot,
    source_generation: u64,
    target_generation: u64,
) -> Result<IndexInvalidationPlan, String> {
    IndexInvalidationPlan::from_snapshots(previous, current, source_generation, target_generation)
}
