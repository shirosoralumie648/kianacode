//! Read-only BQ-19 allocation projection.
//!
//! One accepted `CostAllocation` is one leaf usage. The same leaf is indexed under run/cell/
//! workflow/project/organization views, but each view folds the leaf once and no view is added to
//! another. Rebuilding from the source events is deterministic and never appends ledger facts,
//! mutates reservations or authorizes a project transfer.

use kiana_domain::{
    AllocationCostKind, CostAllocation, CostAllocationId, EventId, OrganizationId, ProjectId,
    RunId, RuntimeEvent, SchemaVersion, UsageId, WorkflowInstanceId, COST_ALLOCATION_EVENT,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const COST_ALLOCATION_PROJECTION_SCHEMA: &str = "kiana.cost-allocation-projection.v1";
pub const COST_ALLOCATION_PROJECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CostAllocationProjectionError {
    #[error("cost_allocation_projection_run_invalid")]
    RunInvalid,
    #[error("cost_allocation_projection_source_empty")]
    SourceEmpty,
    #[error("cost_allocation_projection_invalid:{0}")]
    Invalid(String),
}

/// Numeric totals stay separated by estimate/measured/unknown state. `leaf_count` counts unique
/// usage IDs, not dimensions or parent/child rows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationTotals {
    pub estimated: Option<kiana_domain::Money>,
    pub measured: Option<kiana_domain::Money>,
    pub unknown_count: u64,
    pub leaf_count: u64,
}

impl AllocationTotals {
    fn add_money(
        slot: &mut Option<kiana_domain::Money>,
        amount: &kiana_domain::Money,
    ) -> Result<(), String> {
        if let Some(existing) = slot.as_ref() {
            if existing.currency != amount.currency {
                return Err("cost_allocation_currency_mismatch".to_owned());
            }
        }
        let next = slot
            .as_ref()
            .map(|existing| {
                existing
                    .micros
                    .checked_add(amount.micros)
                    .ok_or_else(|| "cost_allocation_total_overflow".to_owned())
                    .and_then(|micros| kiana_domain::Money::new(amount.currency.clone(), micros))
            })
            .unwrap_or_else(|| Ok(amount.clone()))?;
        *slot = Some(next);
        Ok(())
    }

    fn add(&mut self, cost: &AllocationCostKind) -> Result<(), String> {
        self.leaf_count = self
            .leaf_count
            .checked_add(1)
            .ok_or_else(|| "cost_allocation_total_overflow".to_owned())?;
        match cost {
            AllocationCostKind::Estimated { amount, .. } => {
                Self::add_money(&mut self.estimated, amount)
            }
            AllocationCostKind::Measured { amount, .. } => {
                Self::add_money(&mut self.measured, amount)
            }
            AllocationCostKind::Unknown { .. } => {
                self.unknown_count = self
                    .unknown_count
                    .checked_add(1)
                    .ok_or_else(|| "cost_allocation_total_overflow".to_owned())?;
                Ok(())
            }
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.unknown_count > self.leaf_count {
            return Err("cost_allocation_unknown_count_invalid".to_owned());
        }
        for amount in [self.estimated.as_ref(), self.measured.as_ref()]
            .into_iter()
            .flatten()
        {
            amount.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CostAllocationProjection {
    pub schema: &'static str,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub allocations: Vec<CostAllocation>,
    pub run_totals: AllocationTotals,
    pub organization_totals: BTreeMap<OrganizationId, AllocationTotals>,
    pub project_totals: BTreeMap<ProjectId, AllocationTotals>,
    pub workflow_totals: BTreeMap<WorkflowInstanceId, AllocationTotals>,
    pub cell_totals: BTreeMap<kiana_domain::CellId, AllocationTotals>,
}

impl CostAllocationProjection {
    pub fn validate(&self) -> Result<(), CostAllocationProjectionError> {
        if self.schema != COST_ALLOCATION_PROJECTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&COST_ALLOCATION_PROJECTION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() != self.allocations.len()
        {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_projection_header_invalid".to_owned(),
            ));
        }
        self.run_totals
            .validate()
            .map_err(CostAllocationProjectionError::Invalid)?;
        let mut usage_ids = BTreeSet::new();
        for allocation in &self.allocations {
            allocation
                .validate()
                .map_err(CostAllocationProjectionError::Invalid)?;
            if allocation.scope.run_id != self.run_id || !usage_ids.insert(allocation.usage_id) {
                return Err(CostAllocationProjectionError::Invalid(
                    "cost_allocation_leaf_duplicate_or_run_mismatch".to_owned(),
                ));
            }
        }
        for totals in self
            .organization_totals
            .values()
            .chain(self.project_totals.values())
            .chain(self.workflow_totals.values())
            .chain(self.cell_totals.values())
        {
            totals
                .validate()
                .map_err(CostAllocationProjectionError::Invalid)?;
        }
        Ok(())
    }

    pub fn project_total(&self, project_id: ProjectId) -> Option<&AllocationTotals> {
        self.project_totals.get(&project_id)
    }

    pub fn organization_total(&self, organization_id: OrganizationId) -> Option<&AllocationTotals> {
        self.organization_totals.get(&organization_id)
    }

    pub fn workflow_total(&self, workflow_id: WorkflowInstanceId) -> Option<&AllocationTotals> {
        self.workflow_totals.get(&workflow_id)
    }

    pub fn cell_total(&self, cell_id: kiana_domain::CellId) -> Option<&AllocationTotals> {
        self.cell_totals.get(&cell_id)
    }

    /// The run total is the only total that represents the ledger amount. Dimension views are
    /// labels over the same leaves and must never be summed together.
    pub fn ledger_total(&self) -> &AllocationTotals {
        &self.run_totals
    }
}

/// Rebuild one run's allocation views from committed source events. EventLog ownership and CAS
/// remain above this query-only function.
pub fn project_cost_allocations(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostAllocationProjection, CostAllocationProjectionError> {
    if run_id.as_uuid().is_nil() {
        return Err(CostAllocationProjectionError::RunInvalid);
    }
    let mut allocations = Vec::new();
    let mut source_event_ids = Vec::new();
    let mut seen_event_ids = BTreeSet::new();
    let mut seen_usage_ids = BTreeSet::new();
    let mut source_cursor = 0;
    let mut previous_sequence = 0;
    let run_text = run_id.to_string();
    let mut run_totals = AllocationTotals::default();
    let mut organization_totals = BTreeMap::new();
    let mut project_totals = BTreeMap::new();
    let mut workflow_totals = BTreeMap::new();
    let mut cell_totals = BTreeMap::new();

    for event in events {
        if event.kind != COST_ALLOCATION_EVENT {
            continue;
        }
        if !seen_event_ids.insert(event.event_id) {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_projection_duplicate_source_event".to_owned(),
            ));
        }
        if event.sequence == 0 || event.sequence <= previous_sequence {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_projection_sequence_regression".to_owned(),
            ));
        }
        previous_sequence = event.sequence;
        let allocation: CostAllocation =
            serde_json::from_value(event.data.clone()).map_err(|_| {
                CostAllocationProjectionError::Invalid("cost_allocation_decode_failed".to_owned())
            })?;
        allocation
            .validate()
            .map_err(CostAllocationProjectionError::Invalid)?;
        if allocation.scope.run_id != run_id
            || event.data.get("run_id").and_then(serde_json::Value::as_str)
                != Some(run_text.as_str())
            || event.aggregate_type.as_deref() != Some("cost_allocation")
            || event.aggregate_id.as_deref() != Some(allocation.allocation_id.to_string().as_str())
            || event.stream_version != Some(allocation.revision)
        {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_identity_or_revision_mismatch".to_owned(),
            ));
        }
        if !seen_usage_ids.insert(allocation.usage_id) {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_leaf_usage_repeated".to_owned(),
            ));
        }
        run_totals
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        organization_totals
            .entry(allocation.scope.organization_id)
            .or_default()
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        project_totals
            .entry(allocation.scope.project_id)
            .or_default()
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        workflow_totals
            .entry(allocation.scope.workflow_id)
            .or_default()
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        cell_totals
            .entry(allocation.scope.cell_id)
            .or_default()
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        source_cursor = source_cursor.max(event.sequence);
        source_event_ids.push(event.event_id);
        allocations.push(allocation);
    }
    if source_event_ids.is_empty() {
        return Err(CostAllocationProjectionError::SourceEmpty);
    }
    let projection = CostAllocationProjection {
        schema: COST_ALLOCATION_PROJECTION_SCHEMA,
        version: COST_ALLOCATION_PROJECTION_VERSION,
        run_id,
        source_cursor,
        source_event_ids,
        allocations,
        run_totals,
        organization_totals,
        project_totals,
        workflow_totals,
        cell_totals,
    };
    projection.validate()?;
    Ok(projection)
}

/// Singular alias used by adapters that project one allocation stream.
pub fn project_cost_allocation(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostAllocationProjection, CostAllocationProjectionError> {
    project_cost_allocations(run_id, events)
}

/// Compile-time marker for source guards: the query projection never writes or authorizes.
pub const ALLOCATION_PROJECTOR_IS_READ_ONLY: bool = true;

#[allow(dead_code)]
fn _keep_ids_linked(_: CostAllocationId, _: UsageId) {}
