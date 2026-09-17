//! Read-only reconstruction of approval, budget, lease and Cell ownership facts.
//!
//! This reducer deliberately consumes only committed EventLog values. It never repairs facts,
//! acquires a resource, consumes an approval, or issues a permit; a missing source is distinct
//! from an empty projection and an invalid fact stops reconstruction.

use super::{ControlPlane, CoreError};
use kiana_domain::{
    ApprovalId, ApprovalState, BudgetLeaseId, BudgetReservationFact, BudgetSettlementFact, CellId,
    CellLifecycle, RecoveryResourceSnapshot, RequestId, RuntimeEvent, StorageLockId,
};
use kiana_ports::PortError;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};

const LEASE_ISSUED_KINDS: &[&str] = &["lease.issued", "resource.lease_issued"];
const LEASE_RELEASED_KINDS: &[&str] = &[
    "lease.released",
    "lease.fenced",
    "resource.lease_released",
    "resource.lease_fenced",
];

fn failed(reason: impl Into<String>) -> String {
    reason.into()
}

fn decode<T: DeserializeOwned>(value: Option<&Value>, reason: &str) -> Result<T, String> {
    let value = value.ok_or_else(|| failed(reason))?;
    serde_json::from_value(value.clone()).map_err(|_| failed(reason))
}

fn approval_id(event: &RuntimeEvent) -> Result<ApprovalId, String> {
    event
        .data
        .get("approval_id")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .or_else(|| {
            (event.aggregate_type.as_deref() == Some("approval")).then(|| {
                event
                    .aggregate_id
                    .as_deref()
                    .and_then(ApprovalId::parse_str)
            })
        })
        .flatten()
        .ok_or_else(|| failed("recovery_approval_id_missing"))
}

fn lease_id(event: &RuntimeEvent) -> Result<StorageLockId, String> {
    ["lease_id", "resource_lease_id", "storage_lock_id"]
        .iter()
        .find_map(|field| event.data.get(*field))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .ok_or_else(|| failed("recovery_resource_lease_id_missing"))
}

fn cell_id(event: &RuntimeEvent) -> Result<CellId, String> {
    decode(event.data.get("cell_id"), "recovery_cell_id_missing")
}

fn approval_next(kind: &str) -> Option<ApprovalState> {
    match kind {
        "approval.staged" => Some(ApprovalState::Staged),
        "approval.activated" => Some(ApprovalState::Active),
        "approval.approved" => Some(ApprovalState::Approved),
        "approval.denied" => Some(ApprovalState::Denied),
        "approval.expired" => Some(ApprovalState::Expired),
        "approval.cancelled" => Some(ApprovalState::Cancelled),
        "approval.consumed" => Some(ApprovalState::Consumed),
        _ => None,
    }
}

fn cell_lifecycle(event: &RuntimeEvent) -> Result<CellLifecycle, String> {
    let value = event
        .data
        .get("lifecycle")
        .or_else(|| event.data.get("state"))
        .ok_or_else(|| failed("recovery_cell_lifecycle_missing"))?;
    serde_json::from_value(value.clone()).map_err(|_| failed("recovery_cell_lifecycle_invalid"))
}

/// Rebuild the bounded recovery resource view from a complete source read.
pub fn project_recovery_resources(
    events: &[RuntimeEvent],
) -> Result<RecoveryResourceSnapshot, String> {
    if events.is_empty() {
        return Err(failed("recovery_resource_source_empty"));
    }
    let source_cursor =
        u64::try_from(events.len()).map_err(|_| failed("recovery_cursor_overflow"))?;
    let mut source_event_ids = BTreeSet::new();
    let mut approvals: BTreeMap<ApprovalId, ApprovalState> = BTreeMap::new();
    let mut reservations: BTreeMap<BudgetLeaseId, BTreeSet<RequestId>> = BTreeMap::new();
    let mut leases: BTreeSet<StorageLockId> = BTreeSet::new();
    let mut cells: BTreeMap<CellId, (bool, bool)> = BTreeMap::new();
    let mut seen_settlements = HashSet::new();

    for event in events {
        if !source_event_ids.insert(event.event_id) {
            continue;
        }
        if let Some(next) = approval_next(&event.kind) {
            let id = approval_id(event)?;
            if next == ApprovalState::Staged {
                if approvals.insert(id, next).is_some() {
                    return Err(failed("recovery_approval_duplicate_stage"));
                }
            } else {
                let current = approvals
                    .get_mut(&id)
                    .ok_or_else(|| failed("recovery_approval_stage_missing"))?;
                *current = current
                    .transition(next)
                    .map_err(|_| failed("recovery_approval_transition_invalid"))?;
            }
            if let Some(declared) = event.data.get("state") {
                let declared: ApprovalState = serde_json::from_value(declared.clone())
                    .map_err(|_| failed("recovery_approval_state_invalid"))?;
                if declared != next {
                    return Err(failed("recovery_approval_state_conflict"));
                }
            }
            continue;
        }
        match event.kind.as_str() {
            "model.reserved" => {
                let fact: BudgetReservationFact = decode(
                    event.data.get("reservation_fact"),
                    "recovery_budget_reservation_missing",
                )?;
                fact.validate()
                    .map_err(|_| failed("recovery_budget_reservation_invalid"))?;
                let entries = reservations.entry(fact.budget_lease_id).or_default();
                if !entries.insert(fact.reservation_id) {
                    return Err(failed("recovery_budget_reservation_duplicate"));
                }
            }
            "model.settled" => {
                let fact: BudgetSettlementFact = decode(
                    event.data.get("settlement_fact"),
                    "recovery_budget_settlement_missing",
                )?;
                fact.validate()
                    .map_err(|_| failed("recovery_budget_settlement_invalid"))?;
                if !seen_settlements.insert(fact.reservation_id) {
                    return Err(failed("recovery_budget_settlement_duplicate"));
                }
                let entries = reservations
                    .get_mut(&fact.budget_lease_id)
                    .ok_or_else(|| failed("recovery_budget_reservation_missing"))?;
                if !entries.remove(&fact.reservation_id) {
                    return Err(failed("recovery_budget_settlement_without_reservation"));
                }
                if entries.is_empty() {
                    reservations.remove(&fact.budget_lease_id);
                }
            }
            kind if LEASE_ISSUED_KINDS.contains(&kind) => {
                leases.insert(lease_id(event)?);
            }
            kind if LEASE_RELEASED_KINDS.contains(&kind) => {
                leases.remove(&lease_id(event)?);
            }
            kind if kind.starts_with("cell.") => {
                let id = cell_id(event)?;
                let lifecycle = cell_lifecycle(event)?;
                let active = !lifecycle.is_terminal();
                let fenced = matches!(
                    lifecycle,
                    CellLifecycle::CancelRequested
                        | CellLifecycle::Blocked
                        | CellLifecycle::Quarantined
                        | CellLifecycle::Failed
                        | CellLifecycle::Retired
                );
                cells.insert(id, (active, fenced));
            }
            _ => {}
        }
    }

    RecoveryResourceSnapshot::new(
        source_cursor,
        source_event_ids.into_iter().collect(),
        approvals
            .into_iter()
            .filter_map(|(id, state)| (state == ApprovalState::Active).then_some(id))
            .collect(),
        reservations.into_keys().collect(),
        leases.into_iter().collect(),
        cells
            .iter()
            .filter_map(|(id, (active, _))| (*active).then_some(*id))
            .collect(),
        cells
            .into_iter()
            .filter_map(|(id, (_, fenced))| fenced.then_some(id))
            .collect(),
    )
}

impl ControlPlane {
    /// Rebuild pending/resource state from the source ledger; unsupported reads never look empty.
    pub async fn recovery_resources(&self) -> Result<RecoveryResourceSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            PortError::Unavailable("recovery_resource_projection_unsupported".to_owned())
        })?;
        project_recovery_resources(&events).map_err(|reason| PortError::Failed(reason).into())
    }
}
