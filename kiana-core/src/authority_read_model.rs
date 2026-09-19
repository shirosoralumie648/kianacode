//! PD-11 fact-only Cell/Grant/Budget/Lease/Authority projection.

use kiana_domain::{
    BudgetLeaseId, CapabilityGrantId, CellId, CellLifecycle, RuntimeEvent, StorageLockId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const AUTHORITY_READ_MODEL_SCHEMA: &str = "kiana.persistence-authority-read-model.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CellAuthorityProjection {
    pub cell_id: CellId,
    pub parent_cell_id: Option<CellId>,
    pub lifecycle: CellLifecycle,
    pub authority_epoch: u64,
    pub fenced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GrantAuthorityProjection {
    pub grant_id: CapabilityGrantId,
    pub state: String,
    pub authority_epoch: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BudgetAuthorityProjection {
    pub budget_lease_id: BudgetLeaseId,
    pub reservations: u64,
    pub settlements: u64,
    pub authority_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LeaseAuthorityProjection {
    pub lease_id: StorageLockId,
    pub active: bool,
    pub fenced: bool,
    pub authority_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthorityReadModel {
    pub schema: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<kiana_domain::EventId>,
    pub authority_epoch: u64,
    pub cells: Vec<CellAuthorityProjection>,
    pub grants: Vec<GrantAuthorityProjection>,
    pub budgets: Vec<BudgetAuthorityProjection>,
    pub leases: Vec<LeaseAuthorityProjection>,
}

impl AuthorityReadModel {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHORITY_READ_MODEL_SCHEMA
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.source_event_ids.is_empty()
        {
            return Err("authority_read_model_header_invalid".to_owned());
        }
        if self
            .source_event_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != self.source_event_ids.len()
        {
            return Err("authority_read_model_event_duplicate".to_owned());
        }
        if self
            .cells
            .windows(2)
            .any(|pair| pair[0].cell_id > pair[1].cell_id)
            || self
                .grants
                .windows(2)
                .any(|pair| pair[0].grant_id > pair[1].grant_id)
            || self
                .budgets
                .windows(2)
                .any(|pair| pair[0].budget_lease_id > pair[1].budget_lease_id)
            || self
                .leases
                .windows(2)
                .any(|pair| pair[0].lease_id > pair[1].lease_id)
        {
            return Err("authority_read_model_order_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthorityProjectionError {
    #[error("authority_source_invalid")]
    SourceInvalid,
    #[error("authority_epoch_rollback")]
    EpochRollback,
    #[error("authority_epoch_stale")]
    EpochStale,
    #[error("authority_cell_invalid:{0}")]
    CellInvalid(String),
    #[error("authority_grant_invalid:{0}")]
    GrantInvalid(String),
    #[error("authority_budget_invalid:{0}")]
    BudgetInvalid(String),
    #[error("authority_lease_invalid:{0}")]
    LeaseInvalid(String),
    #[error("authority_child_parent_missing")]
    ChildParentMissing,
    #[error("authority_settlement_without_reservation")]
    SettlementWithoutReservation,
}

fn id<T: for<'de> Deserialize<'de>>(
    data: &Value,
    fields: &[&str],
) -> Result<T, AuthorityProjectionError> {
    fields
        .iter()
        .find_map(|field| data.get(*field))
        .ok_or_else(|| AuthorityProjectionError::SourceInvalid)
        .and_then(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| AuthorityProjectionError::SourceInvalid)
        })
}

fn epoch(event: &RuntimeEvent) -> Result<u64, AuthorityProjectionError> {
    event
        .data
        .get("authority_epoch")
        .and_then(Value::as_u64)
        .filter(|epoch| *epoch > 0)
        .ok_or(AuthorityProjectionError::EpochStale)
}

fn lifecycle(event: &RuntimeEvent) -> Result<CellLifecycle, AuthorityProjectionError> {
    event
        .data
        .get("lifecycle")
        .or_else(|| event.data.get("state"))
        .ok_or_else(|| AuthorityProjectionError::CellInvalid("lifecycle_missing".to_owned()))
        .and_then(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| AuthorityProjectionError::CellInvalid("lifecycle_invalid".to_owned()))
        })
}

pub fn project_authority_read_model(
    events: &[RuntimeEvent],
    source_cursor: u64,
    now_unix_ms: u64,
) -> Result<AuthorityReadModel, AuthorityProjectionError> {
    if source_cursor == 0 || now_unix_ms == 0 || events.is_empty() {
        return Err(AuthorityProjectionError::SourceInvalid);
    }
    let mut source_event_ids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut authority_epoch = 0;
    let mut cells = BTreeMap::<CellId, CellAuthorityProjection>::new();
    let mut grants = BTreeMap::<CapabilityGrantId, GrantAuthorityProjection>::new();
    let mut budgets = BTreeMap::<BudgetLeaseId, BudgetAuthorityProjection>::new();
    let mut leases = BTreeMap::<StorageLockId, LeaseAuthorityProjection>::new();
    let mut reservations = BTreeSet::<(BudgetLeaseId, String)>::new();

    for event in events {
        if !seen.insert(event.event_id) {
            continue;
        }
        source_event_ids.push(event.event_id);
        let event_epoch = match event.kind.as_str() {
            kind if kind.starts_with("cell.")
                || kind.starts_with("grant.")
                || kind.starts_with("lease.")
                || kind.starts_with("budget.")
                || matches!(kind, "model.reserved" | "model.settled") =>
            {
                Some(epoch(event)?)
            }
            _ => None,
        };
        if let Some(event_epoch) = event_epoch {
            if authority_epoch > event_epoch {
                return Err(AuthorityProjectionError::EpochRollback);
            }
            authority_epoch = authority_epoch.max(event_epoch);
        }

        if event.kind.starts_with("cell.") {
            let cell_id: CellId = id(&event.data, &["cell_id"])
                .map_err(|_| AuthorityProjectionError::CellInvalid("cell_id_missing".to_owned()))?;
            let parent_cell_id = event
                .data
                .get("parent_cell_id")
                .and_then(|value| serde_json::from_value(value.clone()).ok());
            let state = lifecycle(event)?;
            let fenced = matches!(
                state,
                CellLifecycle::CancelRequested
                    | CellLifecycle::Blocked
                    | CellLifecycle::Quarantined
                    | CellLifecycle::Failed
                    | CellLifecycle::Retired
            );
            cells.insert(
                cell_id,
                CellAuthorityProjection {
                    cell_id,
                    parent_cell_id,
                    lifecycle: state,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                    fenced,
                },
            );
        } else if event.kind.starts_with("grant.") {
            let grant_id: CapabilityGrantId = id(&event.data, &["capability_grant_id", "grant_id"])
                .map_err(|_| {
                    AuthorityProjectionError::GrantInvalid("grant_id_missing".to_owned())
                })?;
            let state = event
                .data
                .get("state")
                .or_else(|| event.data.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| match event.kind.as_str() {
                    "grant.revoked" => "revoked",
                    "grant.expired" => "expired",
                    _ => "active",
                })
                .to_owned();
            let expires_at_unix_ms = event
                .data
                .get("expires_at_unix_ms")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX);
            if expires_at_unix_ms <= now_unix_ms && state == "active" {
                return Err(AuthorityProjectionError::GrantInvalid(
                    "grant_expired_as_active".to_owned(),
                ));
            }
            grants.insert(
                grant_id,
                GrantAuthorityProjection {
                    grant_id,
                    state,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                    expires_at_unix_ms,
                },
            );
        } else if matches!(event.kind.as_str(), "model.reserved" | "budget.reserved") {
            let budget_lease_id: BudgetLeaseId = id(&event.data, &["budget_lease_id"])
                .or_else(|_| {
                    id(
                        event.data.get("reservation_fact").unwrap_or(&Value::Null),
                        &["budget_lease_id"],
                    )
                })
                .map_err(|_| {
                    AuthorityProjectionError::BudgetInvalid("budget_lease_id_missing".to_owned())
                })?;
            let reservation_id = event
                .data
                .get("reservation_id")
                .or_else(|| {
                    event
                        .data
                        .get("reservation_fact")
                        .and_then(|value| value.get("reservation_id"))
                })
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| event.event_id.to_string());
            if !reservations.insert((budget_lease_id, reservation_id)) {
                return Err(AuthorityProjectionError::BudgetInvalid(
                    "reservation_duplicate".to_owned(),
                ));
            }
            let entry = budgets
                .entry(budget_lease_id)
                .or_insert(BudgetAuthorityProjection {
                    budget_lease_id,
                    reservations: 0,
                    settlements: 0,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                });
            entry.reservations = entry.reservations.saturating_add(1);
        } else if matches!(event.kind.as_str(), "model.settled" | "budget.settled") {
            let budget_lease_id: BudgetLeaseId = id(&event.data, &["budget_lease_id"])
                .or_else(|_| {
                    id(
                        event.data.get("settlement_fact").unwrap_or(&Value::Null),
                        &["budget_lease_id"],
                    )
                })
                .map_err(|_| {
                    AuthorityProjectionError::BudgetInvalid("budget_lease_id_missing".to_owned())
                })?;
            let reservation_id = event
                .data
                .get("reservation_id")
                .or_else(|| {
                    event
                        .data
                        .get("settlement_fact")
                        .and_then(|value| value.get("reservation_id"))
                })
                .and_then(Value::as_str)
                .ok_or(AuthorityProjectionError::SettlementWithoutReservation)?;
            if !reservations.remove(&(budget_lease_id, reservation_id.to_owned())) {
                return Err(AuthorityProjectionError::SettlementWithoutReservation);
            }
            let entry = budgets
                .entry(budget_lease_id)
                .or_insert(BudgetAuthorityProjection {
                    budget_lease_id,
                    reservations: 0,
                    settlements: 0,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                });
            entry.settlements = entry.settlements.saturating_add(1);
        } else if matches!(
            event.kind.as_str(),
            "lease.issued" | "lease.released" | "lease.fenced"
        ) {
            let lease_id: StorageLockId = id(
                &event.data,
                &["lease_id", "resource_lease_id", "storage_lock_id"],
            )
            .map_err(|_| AuthorityProjectionError::LeaseInvalid("lease_id_missing".to_owned()))?;
            let active = event.kind == "lease.issued";
            let fenced = event.kind == "lease.fenced";
            if !active && !leases.contains_key(&lease_id) {
                return Err(AuthorityProjectionError::LeaseInvalid(
                    "lease_without_issue".to_owned(),
                ));
            }
            leases.insert(
                lease_id,
                LeaseAuthorityProjection {
                    lease_id,
                    active,
                    fenced,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                },
            );
        }
    }

    for cell in cells.values() {
        if let Some(parent) = cell.parent_cell_id {
            if !cells.contains_key(&parent) {
                return Err(AuthorityProjectionError::ChildParentMissing);
            }
        }
    }
    let model = AuthorityReadModel {
        schema: AUTHORITY_READ_MODEL_SCHEMA.to_owned(),
        source_cursor,
        source_event_ids,
        authority_epoch,
        cells: cells.into_values().collect(),
        grants: grants.into_values().collect(),
        budgets: budgets.into_values().collect(),
        leases: leases.into_values().collect(),
    };
    model
        .validate()
        .map_err(|_| AuthorityProjectionError::SourceInvalid)?;
    Ok(model)
}
