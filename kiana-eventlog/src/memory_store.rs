//! Memory-side invalidation boundary shared by memory/index/cache projections.
//!
//! This is an in-process CI adapter. It records the epoch fence and target receipts; it does not
//! claim physical JSONL deletion or cross-process recovery. Reads must check the committed epoch
//! before ranking or reinjecting a memory row.

use kiana_domain::{
    DataPropagationPlan, DataPropagationReceipt, DataPropagationState, DataPropagationTarget,
};
use kiana_ports::PortError;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Default)]
struct MemoryGovernanceState {
    plans: BTreeMap<(String, u64), DataPropagationPlan>,
    receipts: BTreeMap<(String, u64, String), DataPropagationReceipt>,
}

/// Non-durable memory/index/cache invalidation store used by remote CI fixtures and composition.
#[derive(Clone, Debug, Default)]
pub struct MemoryDataGovernanceStore {
    state: Arc<Mutex<MemoryGovernanceState>>,
}

impl MemoryDataGovernanceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn apply_plan(&self, plan: DataPropagationPlan) -> Result<(), PortError> {
        plan.validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_plan:{error}")))?;
        let mut state = self.state.lock().await;
        let key = (plan.project_ref.clone(), plan.data_epoch);
        if let Some(existing) = state.plans.get(&key) {
            if existing.plan_digest == plan.plan_digest {
                return Ok(());
            }
            return Err(PortError::Conflict(
                "data_propagation_plan_conflict".to_owned(),
            ));
        }
        if state
            .plans
            .keys()
            .any(|(project, epoch)| project == &plan.project_ref && *epoch > plan.data_epoch)
        {
            return Err(PortError::Conflict(
                "data_propagation_epoch_regressed".to_owned(),
            ));
        }
        state.plans.insert(key, plan);
        Ok(())
    }

    pub async fn append_propagation_receipt(
        &self,
        plan: &DataPropagationPlan,
        receipt: DataPropagationReceipt,
    ) -> Result<(), PortError> {
        plan.validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_plan:{error}")))?;
        receipt
            .validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_receipt:{error}")))?;
        if receipt.target == DataPropagationTarget::EventProjection
            || !plan.targets.iter().any(|target| {
                target.target == receipt.target
                    && target.project_ref == receipt.project_ref
                    && target.data_epoch == receipt.data_epoch
            })
        {
            return Err(PortError::Conflict(
                "memory_propagation_target_not_in_plan".to_owned(),
            ));
        }
        let mut state = self.state.lock().await;
        if !state
            .plans
            .contains_key(&(plan.project_ref.clone(), plan.data_epoch))
        {
            return Err(PortError::Unavailable(
                "memory_propagation_plan_missing".to_owned(),
            ));
        }
        let key = (
            receipt.project_ref.clone(),
            receipt.data_epoch,
            receipt.target.as_str().to_owned(),
        );
        if let Some(existing) = state.receipts.get(&key) {
            if existing == &receipt {
                return Ok(());
            }
            return Err(PortError::Conflict(
                "memory_propagation_receipt_conflict".to_owned(),
            ));
        }
        state.receipts.insert(key, receipt);
        Ok(())
    }

    /// Return whether a memory/index/cache row may be read under the current server epoch.
    /// Invalidated and Unknown targets fail closed; historical receipt metadata is not a payload.
    pub async fn read_allowed(
        &self,
        project_ref: &str,
        observed_data_epoch: u64,
        target: DataPropagationTarget,
    ) -> Result<bool, PortError> {
        let state = self.state.lock().await;
        let Some((_, plan)) = state
            .plans
            .iter()
            .filter(|((project, _), _)| project == project_ref)
            .max_by_key(|((_, epoch), _)| *epoch)
        else {
            return Ok(true);
        };
        if observed_data_epoch < plan.data_epoch {
            return Ok(false);
        }
        let completed = state.receipts.values().find(|receipt| {
            receipt.project_ref == project_ref
                && receipt.data_epoch == plan.data_epoch
                && receipt.target == target
        });
        Ok(completed.is_some_and(|receipt| {
            receipt.state == DataPropagationState::PreservedMetadata
                && target == DataPropagationTarget::Audit
        }))
    }
}
