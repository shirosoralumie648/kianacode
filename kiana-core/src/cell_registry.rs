//! Process-local CompanyOS cell admission.
//!
//! This is the local adapter for the CompanyOS resource boundary. It keeps
//! admission, path locks, and budget reservations in one critical section.
//! The state is intentionally process-local; durable recovery belongs to the
//! event/state-store phase and must implement the same port contract.

use async_trait::async_trait;
use kiana_domain::{
    AgentTemplate, BudgetLease, BudgetLeaseId, CellId, CellLifecycle, CellSpec, RequestId,
    RetirementRecord, RoleSpec, RunId, SpawnPlanId, SpawnPlanStatus, WorkFingerprint,
    allow_list_covers, builder_lock_paths,
};
use kiana_ports::{
    CapabilityLease, CapabilityOutcome, CellRegistryPort, PortError, SpawnReservation,
    SpawnReservationRequest,
};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const TEMPLATE_VERSION: &str = "1.0.0";
const MAX_ROOT_CELLS: usize = 64;
const MAX_ACTIVE_CELLS: usize = 64;
const MAX_CHILDREN_PER_PARENT: usize = 8;

struct CellRecord {
    reservation: SpawnReservation,
    locked_paths: Vec<String>,
    resources_released: bool,
    active_capabilities: HashMap<RequestId, CapabilityLease>,
}

#[derive(Default)]
struct RegistryState {
    templates: HashMap<(String, String), AgentTemplate>,
    records: HashMap<SpawnPlanId, CellRecord>,
    by_idempotency: HashMap<String, SpawnPlanId>,
    by_fingerprint: HashMap<WorkFingerprint, SpawnPlanId>,
    by_cell: HashMap<CellId, SpawnPlanId>,
    by_run: HashMap<RunId, Vec<CellId>>,
    budgets: HashMap<kiana_domain::BudgetLeaseId, BudgetLease>,
    path_locks: HashMap<String, CellId>,
}

/// Process-local registry used by the default control-plane composition.
#[derive(Default)]
pub struct MemoryCellRegistry {
    state: Mutex<RegistryState>,
}

impl MemoryCellRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reservation(&self, plan_id: SpawnPlanId) -> Option<SpawnReservation> {
        self.state.lock().ok().and_then(|state| {
            state
                .records
                .get(&plan_id)
                .map(|record| record.reservation.clone())
        })
    }

    pub fn active_cells(&self) -> usize {
        self.state
            .lock()
            .map(|state| {
                state
                    .records
                    .values()
                    .filter(|record| !record.reservation.cell.lifecycle.is_terminal())
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn reservations(&self) -> Vec<SpawnReservation> {
        self.state
            .lock()
            .map(|state| {
                state
                    .records
                    .values()
                    .map(|record| record.reservation.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, RegistryState>, PortError> {
        self.state
            .lock()
            .map_err(|_| PortError::Failed("cell_registry_poisoned".to_owned()))
    }

    fn active_root_cells(state: &RegistryState) -> usize {
        state
            .records
            .values()
            .filter(|record| {
                record.reservation.cell.parent_cell_id.is_none()
                    && !record.reservation.cell.lifecycle.is_terminal()
            })
            .count()
    }

    fn active_children(state: &RegistryState, parent: CellId) -> usize {
        state
            .records
            .values()
            .filter(|record| {
                record.reservation.cell.parent_cell_id == Some(parent)
                    && !record.reservation.cell.lifecycle.is_terminal()
            })
            .count()
    }

    fn active_by_fingerprint(
        state: &RegistryState,
        fingerprint: &WorkFingerprint,
    ) -> Option<SpawnPlanId> {
        state
            .by_fingerprint
            .get(fingerprint)
            .copied()
            .filter(|plan_id| {
                state
                    .records
                    .get(plan_id)
                    .is_some_and(|record| !record.reservation.cell.lifecycle.is_terminal())
            })
    }

    fn immutable_budget_matches(left: &BudgetLease, right: &BudgetLease) -> bool {
        left.schema == right.schema
            && left.lease_id == right.lease_id
            && left.max_tool_calls == right.max_tool_calls
            && left.max_tokens == right.max_tokens
            && left.max_wall_clock_ms == right.max_wall_clock_ms
            && left.max_concurrency == right.max_concurrency
            && left.max_effects == right.max_effects
            && left.max_reserved_budget == right.max_reserved_budget
    }

    fn release_resources(
        state: &mut RegistryState,
        record: &mut CellRecord,
    ) -> Result<(), PortError> {
        if record.resources_released {
            return Ok(());
        }
        let budget = state
            .budgets
            .get_mut(&record.reservation.budget.lease_id)
            .ok_or_else(|| PortError::Failed("spawn_budget_ledger_missing".to_owned()))?;
        budget
            .release(record.reservation.plan.budget_reservation)
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        record.reservation.budget = budget.clone();
        for path in &record.locked_paths {
            if state.path_locks.get(path) == Some(&record.reservation.cell.cell_id) {
                state.path_locks.remove(path);
            }
        }
        record.resources_released = true;
        Ok(())
    }

    fn now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }
}

#[async_trait]
impl CellRegistryPort for MemoryCellRegistry {
    async fn resolve_template(
        &self,
        role_id: &str,
        version: &str,
    ) -> Result<AgentTemplate, PortError> {
        let role_id = role_id.trim();
        let version = version.trim();
        let role = RoleSpec::lookup(role_id)
            .ok_or_else(|| PortError::Failed("spawn_template_not_found".to_owned()))?;
        if version.is_empty() {
            return Err(PortError::Failed(
                "spawn_template_version_required".to_owned(),
            ));
        }
        if version != TEMPLATE_VERSION {
            return Err(PortError::Failed(
                "spawn_template_version_mismatch".to_owned(),
            ));
        }
        let mut state = self.lock_state()?;
        let key = (role.role_id.clone(), version.to_owned());
        if let Some(template) = state.templates.get(&key) {
            return Ok(template.clone());
        }
        let template = AgentTemplate::for_role(&role, version.to_owned());
        template
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        state.templates.insert(key, template.clone());
        Ok(template)
    }

    async fn reserve_spawn(
        &self,
        request: SpawnReservationRequest,
    ) -> Result<SpawnReservation, PortError> {
        let mut state = self.lock_state()?;
        let SpawnReservationRequest {
            mut plan,
            mut cell,
            template,
            budget,
            grant,
            supervision,
            fingerprint,
            owned_paths,
        } = request;

        plan.validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        if plan.status != SpawnPlanStatus::Proposed {
            return Err(PortError::Failed("spawn_plan_not_proposed".to_owned()));
        }
        if plan.count != 1 || plan.candidate_templates.len() != 1 {
            return Err(PortError::Failed("spawn_count_unsupported".to_owned()));
        }
        if plan.candidate_templates[0] != template.template_id {
            return Err(PortError::Failed(
                "spawn_template_version_mismatch".to_owned(),
            ));
        }
        let template_key = (template.role_id.clone(), template.version.clone());
        if state.templates.get(&template_key) != Some(&template) {
            return Err(PortError::Failed(
                "spawn_template_not_registered".to_owned(),
            ));
        }
        let now = Self::now_unix_ms();
        if plan.deadline_unix_ms <= now {
            return Err(PortError::Failed("spawn_deadline_expired".to_owned()));
        }
        if grant.expires_at_unix_ms <= now
            || grant.expires_at_unix_ms < plan.deadline_unix_ms
            || grant.expires_at_unix_ms
                > now.saturating_add(template.ttl_seconds.saturating_mul(1_000))
            || supervision.stall_threshold_seconds.saturating_mul(1_000)
                < plan.deadline_unix_ms.saturating_sub(now)
        {
            return Err(PortError::Failed("spawn_ttl_invalid".to_owned()));
        }
        template
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        grant
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        budget
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        supervision
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        cell.validate(&template)
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        if cell.lifecycle != CellLifecycle::Proposed {
            return Err(PortError::Failed("cell_not_proposed".to_owned()));
        }
        if cell.parent_cell_id != plan.parent_cell_id {
            return Err(PortError::Failed("spawn_parent_mismatch".to_owned()));
        }
        if cell.capability_grant_id != grant.grant_id
            || cell.budget_lease_id != budget.lease_id
            || cell.supervision_lease_id != supervision.lease_id
        {
            return Err(PortError::Failed(
                "spawn_resource_identity_mismatch".to_owned(),
            ));
        }
        if cell.input_refs != plan.input_refs
            || cell.partition_key != plan.partition
            || cell.output_contract != plan.output_contract
        {
            return Err(PortError::Failed("spawn_contract_mismatch".to_owned()));
        }
        if cell.owned_paths != owned_paths {
            return Err(PortError::Failed("spawn_owned_paths_mismatch".to_owned()));
        }
        if cell
            .owned_paths
            .iter()
            .any(|path| !allow_list_covers(&grant.paths, path))
        {
            return Err(PortError::Failed("spawn_grant_scope_invalid".to_owned()));
        }

        let idempotency_key = plan.idempotency_key.trim();
        if let Some(existing_plan_id) = state.by_idempotency.get(idempotency_key).copied() {
            let existing = state
                .records
                .get(&existing_plan_id)
                .ok_or_else(|| PortError::Failed("spawn_registry_inconsistent".to_owned()))?;
            if existing.reservation.fingerprint != fingerprint {
                return Err(PortError::Conflict("spawn_idempotency_conflict".to_owned()));
            }
            let mut replay = existing.reservation.clone();
            replay.replayed = true;
            return Ok(replay);
        }
        if state.records.contains_key(&plan.plan_id) || state.by_cell.contains_key(&cell.cell_id) {
            return Err(PortError::Conflict("spawn_identity_conflict".to_owned()));
        }
        if Self::active_by_fingerprint(&state, &fingerprint).is_some() {
            return Err(PortError::Conflict("spawn_fingerprint_active".to_owned()));
        }

        let active_cells = state
            .records
            .values()
            .filter(|record| !record.reservation.cell.lifecycle.is_terminal())
            .count();
        if active_cells >= MAX_ACTIVE_CELLS {
            return Err(PortError::Conflict(
                "spawn_concurrency_limit_exceeded".to_owned(),
            ));
        }
        if cell.parent_cell_id.is_none() {
            if Self::active_root_cells(&state) >= MAX_ROOT_CELLS {
                return Err(PortError::Conflict("spawn_root_limit_exceeded".to_owned()));
            }
            if cell.depth != 0 {
                return Err(PortError::Failed("spawn_depth_exceeded".to_owned()));
            }
        } else {
            let parent_id = cell.parent_cell_id.expect("checked above");
            let parent_plan = state
                .by_cell
                .get(&parent_id)
                .copied()
                .ok_or_else(|| PortError::Failed("spawn_parent_not_found".to_owned()))?;
            let parent = state
                .records
                .get(&parent_plan)
                .ok_or_else(|| PortError::Failed("spawn_registry_inconsistent".to_owned()))?;
            if !matches!(
                parent.reservation.cell.lifecycle,
                CellLifecycle::Running | CellLifecycle::ReadyToMerge
            ) {
                return Err(PortError::Failed("spawn_parent_not_active".to_owned()));
            }
            if !parent.reservation.template.delegation_allowed
                || !parent.reservation.grant.delegation_allowed
            {
                return Err(PortError::Failed("spawn_delegation_denied".to_owned()));
            }
            let child_count = Self::active_children(&state, parent_id);
            if child_count >= MAX_CHILDREN_PER_PARENT
                || child_count >= parent.reservation.cell.spawn_quota as usize
            {
                return Err(PortError::Conflict(
                    "spawn_children_limit_exceeded".to_owned(),
                ));
            }
            if cell.depth != parent.reservation.cell.depth.saturating_add(1) {
                return Err(PortError::Failed("spawn_depth_exceeded".to_owned()));
            }
            if !parent.reservation.grant.contains(&grant) {
                return Err(PortError::Failed("spawn_grant_not_contained".to_owned()));
            }
        }

        let locked_paths = builder_lock_paths(&owned_paths);
        for path in &locked_paths {
            if state.path_locks.iter().any(|(held, owner)| {
                *owner != cell.cell_id && kiana_domain::path_locks_conflict(path, held)
            }) {
                return Err(PortError::Conflict("path_lock_conflict".to_owned()));
            }
        }

        let budget_ledger = state
            .budgets
            .entry(budget.lease_id)
            .or_insert_with(|| budget.clone());
        if !Self::immutable_budget_matches(budget_ledger, &budget) {
            return Err(PortError::Conflict(
                "spawn_budget_identity_conflict".to_owned(),
            ));
        }
        budget_ledger
            .reserve(plan.budget_reservation)
            .map_err(|reason| PortError::Conflict(reason.to_owned()))?;
        let budget = budget_ledger.clone();

        plan.status = plan
            .status
            .transition(SpawnPlanStatus::Validated)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        plan.status = plan
            .status
            .transition(SpawnPlanStatus::Reserved)
            .map_err(|error| PortError::Failed(error.to_string()))?;
        cell.lifecycle = cell
            .lifecycle
            .transition(CellLifecycle::Validated)
            .map_err(|error| PortError::Failed(error.to_string()))?;

        let reservation = SpawnReservation {
            plan,
            cell,
            template,
            budget,
            grant,
            supervision,
            fingerprint,
            owned_paths: locked_paths.clone(),
            replayed: false,
        };
        let plan_id = reservation.plan.plan_id;
        let cell_id = reservation.cell.cell_id;
        state
            .by_idempotency
            .insert(reservation.plan.idempotency_key.trim().to_owned(), plan_id);
        state
            .by_fingerprint
            .insert(reservation.fingerprint.clone(), plan_id);
        state.by_cell.insert(cell_id, plan_id);
        state
            .by_run
            .entry(reservation.cell.root_run_id)
            .or_default()
            .push(cell_id);
        for path in &locked_paths {
            state.path_locks.insert(path.clone(), cell_id);
        }
        state.records.insert(
            plan_id,
            CellRecord {
                reservation: reservation.clone(),
                locked_paths,
                resources_released: false,
                active_capabilities: HashMap::new(),
            },
        );
        Ok(reservation)
    }

    async fn transition_cell(
        &self,
        cell_id: CellId,
        expected: CellLifecycle,
        next: CellLifecycle,
    ) -> Result<CellSpec, PortError> {
        let mut state = self.lock_state()?;
        let plan_id = state
            .by_cell
            .get(&cell_id)
            .copied()
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let mut record = state
            .records
            .remove(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
        let result = (|| {
            if record.reservation.cell.lifecycle != expected {
                return Err(PortError::Conflict("cell_state_conflict".to_owned()));
            }
            let next_lifecycle = record
                .reservation
                .cell
                .lifecycle
                .transition(next)
                .map_err(|error| PortError::Conflict(error.to_string()))?;
            let next_plan = if next == CellLifecycle::Running
                && record.reservation.plan.status == SpawnPlanStatus::Reserved
            {
                Some(
                    record
                        .reservation
                        .plan
                        .status
                        .transition(SpawnPlanStatus::Committed)
                        .map_err(|error| PortError::Conflict(error.to_string()))?,
                )
            } else {
                None
            };
            record.reservation.cell.lifecycle = next_lifecycle;
            if let Some(next_plan) = next_plan {
                record.reservation.plan.status = next_plan;
            }
            Ok(record.reservation.cell.clone())
        })();
        state.records.insert(plan_id, record);
        result
    }

    async fn begin_capability(
        &self,
        cell_id: CellId,
        capability_grant_id: kiana_domain::CapabilityGrantId,
        budget_lease_id: BudgetLeaseId,
        request: &kiana_domain::CapabilityRequest,
    ) -> Result<CapabilityLease, PortError> {
        let mut state = self.lock_state()?;
        let plan_id = state
            .by_cell
            .get(&cell_id)
            .copied()
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let (reservation, active_count, resources_released) = {
            let record = state
                .records
                .get(&plan_id)
                .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
            (
                record.reservation.clone(),
                record.active_capabilities.len(),
                record.resources_released,
            )
        };
        if reservation.cell.lifecycle != CellLifecycle::Running || resources_released {
            return Err(PortError::Conflict(
                "cell_capability_cell_not_running".to_owned(),
            ));
        }
        if request.cell_id != Some(cell_id)
            || request.capability_grant_id != Some(capability_grant_id)
            || request.budget_lease_id != Some(budget_lease_id)
        {
            return Err(PortError::Failed(
                "cell_capability_scope_mismatch".to_owned(),
            ));
        }
        if reservation.grant.grant_id != capability_grant_id
            || reservation.budget.lease_id != budget_lease_id
        {
            return Err(PortError::Failed(
                "cell_capability_resource_mismatch".to_owned(),
            ));
        }
        if reservation.grant.expires_at_unix_ms <= Self::now_unix_ms() {
            return Err(PortError::Failed(
                "cell_capability_grant_expired".to_owned(),
            ));
        }
        if !reservation.grant.allows_request(request) {
            return Err(PortError::Failed("cell_capability_grant_denied".to_owned()));
        }
        if state
            .records
            .get(&plan_id)
            .is_some_and(|record| record.active_capabilities.contains_key(&request.request_id))
        {
            return Err(PortError::Conflict(
                "cell_capability_invocation_duplicate".to_owned(),
            ));
        }
        let effect_count = u32::from(request.risk != kiana_domain::RiskLevel::ReadOnly);
        let budget = state
            .budgets
            .get_mut(&budget_lease_id)
            .ok_or_else(|| PortError::Failed("spawn_budget_ledger_missing".to_owned()))?;
        if active_count >= budget.max_concurrency as usize {
            return Err(PortError::Conflict(
                "cell_capability_concurrency_exceeded".to_owned(),
            ));
        }
        budget
            .consume(1, 0, effect_count)
            .map_err(|reason| PortError::Conflict(reason.to_owned()))?;
        let budget_snapshot = budget.clone();
        let lease = CapabilityLease {
            request_id: request.request_id,
            cell_id,
            capability_grant_id,
            budget_lease_id,
            effect_count,
        };
        let record = state
            .records
            .get_mut(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
        record.reservation.budget = budget_snapshot;
        record
            .active_capabilities
            .insert(request.request_id, lease.clone());
        Ok(lease)
    }

    async fn finish_capability(
        &self,
        lease: CapabilityLease,
        _outcome: CapabilityOutcome,
    ) -> Result<(), PortError> {
        let mut state = self.lock_state()?;
        let plan_id = state
            .by_cell
            .get(&lease.cell_id)
            .copied()
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let active = state
            .records
            .get(&plan_id)
            .and_then(|record| record.active_capabilities.get(&lease.request_id))
            .cloned()
            .ok_or_else(|| PortError::Conflict("cell_capability_not_active".to_owned()))?;
        if active != lease {
            return Err(PortError::Conflict(
                "cell_capability_lease_mismatch".to_owned(),
            ));
        }
        let budget_snapshot = state
            .budgets
            .get(&lease.budget_lease_id)
            .cloned()
            .ok_or_else(|| PortError::Failed("spawn_budget_ledger_missing".to_owned()))?;
        let record = state
            .records
            .get_mut(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
        record.active_capabilities.remove(&lease.request_id);
        record.reservation.budget = budget_snapshot;
        Ok(())
    }

    async fn commit_spawn(&self, plan_id: SpawnPlanId) -> Result<SpawnReservation, PortError> {
        let mut state = self.lock_state()?;
        let record = state
            .records
            .get_mut(&plan_id)
            .ok_or_else(|| PortError::Failed("spawn_reservation_not_found".to_owned()))?;
        if record.reservation.plan.status != SpawnPlanStatus::Reserved {
            if record.reservation.plan.status == SpawnPlanStatus::Committed {
                return Ok(record.reservation.clone());
            }
            return Err(PortError::Conflict("spawn_plan_not_reserved".to_owned()));
        }
        if record.reservation.cell.lifecycle != CellLifecycle::Validated {
            return Err(PortError::Conflict("cell_not_validated".to_owned()));
        }
        let committed_plan = record
            .reservation
            .plan
            .status
            .transition(SpawnPlanStatus::Committed)
            .map_err(|error| PortError::Conflict(error.to_string()))?;
        let spawning_cell = record
            .reservation
            .cell
            .lifecycle
            .transition(CellLifecycle::Spawning)
            .map_err(|error| PortError::Conflict(error.to_string()))?;
        let ready_cell = spawning_cell
            .transition(CellLifecycle::Ready)
            .map_err(|error| PortError::Conflict(error.to_string()))?;
        record.reservation.plan.status = committed_plan;
        record.reservation.cell.lifecycle = ready_cell;
        Ok(record.reservation.clone())
    }

    async fn abort_spawn(&self, plan_id: SpawnPlanId, _reason: &str) -> Result<(), PortError> {
        let mut state = self.lock_state()?;
        let mut record = state
            .records
            .remove(&plan_id)
            .ok_or_else(|| PortError::Failed("spawn_reservation_not_found".to_owned()))?;
        if record.resources_released {
            state.records.insert(plan_id, record);
            return Ok(());
        }
        let lifecycle = record.reservation.cell.lifecycle;
        if lifecycle.can_transition_to(CellLifecycle::CancelRequested) {
            let cancelled = lifecycle
                .transition(CellLifecycle::CancelRequested)
                .map_err(|error| PortError::Failed(error.to_string()))?
                .transition(CellLifecycle::Cancelled)
                .map_err(|error| PortError::Failed(error.to_string()))?;
            record.reservation.cell.lifecycle = cancelled;
        } else if !lifecycle.is_terminal() {
            state.records.insert(plan_id, record);
            return Err(PortError::Conflict("cell_abort_state_conflict".to_owned()));
        }
        if record
            .reservation
            .plan
            .status
            .can_transition_to(SpawnPlanStatus::RolledBack)
        {
            record.reservation.plan.status = record
                .reservation
                .plan
                .status
                .transition(SpawnPlanStatus::RolledBack)
                .map_err(|error| PortError::Failed(error.to_string()))?;
        }
        Self::release_resources(&mut state, &mut record)?;
        state.records.insert(plan_id, record);
        Ok(())
    }

    async fn retire_cell(
        &self,
        cell_id: CellId,
        reason: &str,
    ) -> Result<RetirementRecord, PortError> {
        let mut state = self.lock_state()?;
        let plan_id = state
            .by_cell
            .get(&cell_id)
            .copied()
            .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
        let mut record = state
            .records
            .remove(&plan_id)
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))?;
        if record.reservation.cell.lifecycle == CellLifecycle::Retired {
            state.records.insert(plan_id, record);
            return Err(PortError::Conflict(
                "spawn_reservation_already_released".to_owned(),
            ));
        }
        if !record.resources_released {
            if !record.active_capabilities.is_empty() {
                state.records.insert(plan_id, record);
                return Err(PortError::Conflict("cell_capability_in_flight".to_owned()));
            }
            let lifecycle = record.reservation.cell.lifecycle;
            let retired = match lifecycle {
                CellLifecycle::Succeeded | CellLifecycle::ReadyToMerge => lifecycle
                    .transition(CellLifecycle::Retiring)
                    .map_err(|error| PortError::Failed(error.to_string()))?
                    .transition(CellLifecycle::Retired)
                    .map_err(|error| PortError::Failed(error.to_string()))?,
                CellLifecycle::Cancelled | CellLifecycle::Quarantined | CellLifecycle::Blocked => {
                    lifecycle
                        .transition(CellLifecycle::Retiring)
                        .map_err(|error| PortError::Failed(error.to_string()))?
                        .transition(CellLifecycle::Retired)
                        .map_err(|error| PortError::Failed(error.to_string()))?
                }
                CellLifecycle::Failed => lifecycle
                    .transition(CellLifecycle::Quarantined)
                    .map_err(|error| PortError::Failed(error.to_string()))?
                    .transition(CellLifecycle::Retiring)
                    .map_err(|error| PortError::Failed(error.to_string()))?
                    .transition(CellLifecycle::Retired)
                    .map_err(|error| PortError::Failed(error.to_string()))?,
                CellLifecycle::Retiring => lifecycle
                    .transition(CellLifecycle::Retired)
                    .map_err(|error| PortError::Failed(error.to_string()))?,
                _ => {
                    state.records.insert(plan_id, record);
                    return Err(PortError::Conflict("cell_not_ready_to_retire".to_owned()));
                }
            };
            record.reservation.cell.lifecycle = retired;
            Self::release_resources(&mut state, &mut record)?;
        }
        let retirement = RetirementRecord {
            schema: kiana_domain::RETIREMENT_RECORD_SCHEMA.to_owned(),
            cell_id,
            grant_id: record.reservation.grant.grant_id,
            budget_lease_id: record.reservation.budget.lease_id,
            supervision_lease_id: record.reservation.supervision.lease_id,
            released_paths: record.reservation.owned_paths.clone(),
            reason: if reason.trim().is_empty() {
                "retired".to_owned()
            } else {
                reason.trim().to_owned()
            },
            retired_at_unix_ms: Self::now_unix_ms(),
        };
        retirement
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        state.records.insert(plan_id, record);
        Ok(retirement)
    }

    async fn reservation_for_cell(
        &self,
        cell_id: CellId,
    ) -> Result<Option<SpawnReservation>, PortError> {
        let state = self.lock_state()?;
        let Some(plan_id) = state.by_cell.get(&cell_id).copied() else {
            return Ok(None);
        };
        state
            .records
            .get(&plan_id)
            .map(|record| record.reservation.clone())
            .ok_or_else(|| PortError::Failed("cell_registry_inconsistent".to_owned()))
            .map(Some)
    }

    async fn cell_for_run(&self, run_id: RunId) -> Result<Option<CellId>, PortError> {
        let state = self.lock_state()?;
        let Some(cell_ids) = state.by_run.get(&run_id) else {
            return Ok(None);
        };
        Ok(cell_ids
            .iter()
            .find(|cell_id| {
                state
                    .by_cell
                    .get(cell_id)
                    .and_then(|plan_id| state.records.get(plan_id))
                    .is_some_and(|record| !record.reservation.cell.lifecycle.is_terminal())
            })
            .copied()
            .or_else(|| cell_ids.first().copied()))
    }
}
