//! Atomic Cell admission and resource-release contract for CompanyOS.
//!
//! The concrete CellRegistry remains the effect boundary.  This module is the replayable domain
//! decision that binds one SpawnPlan to exactly one Cell, BudgetLease, CapabilityGrant,
//! SupervisionLease and path set.  Reserve/commit/rollback/retire are explicit and idempotent;
//! an uncertain outcome never guesses that a resource was released.

use crate::{
    json_digest, normalize_role_path, BudgetLeaseId, CapabilityGrantId, CellId, RunId, SpawnPlanId,
    SupervisionLeaseId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const CELL_ADMISSION_SCHEMA: &str = "kiana.cell-admission.v1";
pub const CELL_ADMISSION_LEDGER_SCHEMA: &str = "kiana.cell-admission-ledger.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellAdmissionResources {
    pub plan_id: SpawnPlanId,
    pub cell_id: CellId,
    pub budget_lease_id: BudgetLeaseId,
    pub capability_grant_id: CapabilityGrantId,
    pub supervision_lease_id: SupervisionLeaseId,
    pub owned_paths: Vec<String>,
}

impl CellAdmissionResources {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.plan_id.as_uuid().is_nil()
            || self.cell_id.as_uuid().is_nil()
            || self.budget_lease_id.as_uuid().is_nil()
            || self.capability_grant_id.as_uuid().is_nil()
            || self.supervision_lease_id.as_uuid().is_nil()
        {
            return Err("cell_admission_resource_identity_invalid");
        }
        let mut paths = self.owned_paths.clone();
        for path in &mut paths {
            *path = normalize_role_path(path).ok_or("cell_admission_path_invalid")?;
        }
        paths.sort();
        if paths.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("cell_admission_path_duplicate");
        }
        Ok(())
    }

    pub fn canonical_paths(&self) -> Result<Vec<String>, &'static str> {
        self.validate()?;
        let mut paths = self
            .owned_paths
            .iter()
            .map(|path| normalize_role_path(path).ok_or("cell_admission_path_invalid"))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        Ok(paths)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellAdmissionStatus {
    Reserved,
    Committed,
    RolledBack,
    Retired,
    Unknown,
}

impl CellAdmissionStatus {
    pub fn terminal(self) -> bool {
        matches!(self, Self::RolledBack | Self::Retired | Self::Unknown)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellAdmission {
    pub schema: String,
    pub admission_id: String,
    pub idempotency_key: String,
    pub root_run_id: RunId,
    pub owner_cell_id: CellId,
    pub resources: CellAdmissionResources,
    pub status: CellAdmissionStatus,
    pub reserved_at: u64,
    pub committed_at: Option<u64>,
    pub released_at: Option<u64>,
    pub release_reason: Option<String>,
    pub active_capabilities: u32,
    pub resources_released: bool,
    pub digest: String,
}

impl CellAdmission {
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(
        admission_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        root_run_id: RunId,
        owner_cell_id: CellId,
        resources: CellAdmissionResources,
        reserved_at: u64,
    ) -> Result<Self, &'static str> {
        let mut admission = Self {
            schema: CELL_ADMISSION_SCHEMA.to_owned(),
            admission_id: admission_id.into(),
            idempotency_key: idempotency_key.into(),
            root_run_id,
            owner_cell_id,
            resources,
            status: CellAdmissionStatus::Reserved,
            reserved_at,
            committed_at: None,
            released_at: None,
            release_reason: None,
            active_capabilities: 0,
            resources_released: false,
            digest: String::new(),
        };
        admission.digest = admission.canonical_digest();
        admission.validate()?;
        Ok(admission)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CELL_ADMISSION_SCHEMA
            || self.root_run_id.as_uuid().is_nil()
            || self.owner_cell_id.as_uuid().is_nil()
            || self.reserved_at == 0
        {
            return Err("cell_admission_identity_invalid");
        }
        required(&self.admission_id, "cell_admission_id_invalid")?;
        required(&self.idempotency_key, "cell_admission_idempotency_invalid")?;
        self.resources.validate()?;
        if self.resources.cell_id != self.owner_cell_id {
            return Err("cell_admission_owner_cell_mismatch");
        }
        if self.status == CellAdmissionStatus::Committed && self.committed_at.is_none() {
            return Err("cell_admission_commit_time_required");
        }
        if self.status.terminal()
            && self.released_at.is_none()
            && self.status != CellAdmissionStatus::Unknown
        {
            return Err("cell_admission_release_time_required");
        }
        if self.resources_released
            && !matches!(
                self.status,
                CellAdmissionStatus::RolledBack | CellAdmissionStatus::Retired
            )
        {
            return Err("cell_admission_release_status_invalid");
        }
        if self.resources_released && self.active_capabilities != 0 {
            return Err("cell_admission_active_capability_release_forbidden");
        }
        if self
            .release_reason
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty())
        {
            return Err("cell_admission_release_reason_invalid");
        }
        if self.digest != self.canonical_digest() {
            return Err("cell_admission_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "admission_id": self.admission_id,
            "idempotency_key": self.idempotency_key,
            "root_run_id": self.root_run_id,
            "owner_cell_id": self.owner_cell_id,
            "resources": self.resources,
            "status": self.status,
            "reserved_at": self.reserved_at,
            "committed_at": self.committed_at,
            "released_at": self.released_at,
            "release_reason": self.release_reason,
            "active_capabilities": self.active_capabilities,
            "resources_released": self.resources_released,
        }))
    }

    pub fn mark_capability_started(&mut self) -> Result<(), &'static str> {
        if !matches!(
            self.status,
            CellAdmissionStatus::Reserved | CellAdmissionStatus::Committed
        ) || self.resources_released
        {
            return Err("cell_admission_capability_start_denied");
        }
        self.active_capabilities = self.active_capabilities.saturating_add(1);
        self.digest = self.canonical_digest();
        self.validate()
    }

    pub fn mark_capability_finished(&mut self) -> Result<(), &'static str> {
        if self.active_capabilities == 0 {
            return Err("cell_admission_capability_not_active");
        }
        self.active_capabilities -= 1;
        self.digest = self.canonical_digest();
        self.validate()
    }

    pub fn commit(&mut self, committed_at: u64) -> Result<(), &'static str> {
        self.validate()?;
        if self.status == CellAdmissionStatus::Committed {
            return Ok(());
        }
        if self.status != CellAdmissionStatus::Reserved || committed_at < self.reserved_at {
            return Err("cell_admission_commit_invalid");
        }
        self.status = CellAdmissionStatus::Committed;
        self.committed_at = Some(committed_at);
        self.digest = self.canonical_digest();
        self.validate()
    }

    fn release(
        &mut self,
        status: CellAdmissionStatus,
        reason: &str,
        released_at: u64,
    ) -> Result<(), &'static str> {
        self.validate()?;
        if self.status == CellAdmissionStatus::Unknown {
            return Err("cell_admission_release_conflict");
        }
        if self.resources_released {
            if self.status == status {
                return Ok(());
            }
            return Err("cell_admission_release_conflict");
        }
        if self.active_capabilities != 0 {
            return Err("cell_admission_active_capability_release_forbidden");
        }
        if !matches!(
            status,
            CellAdmissionStatus::RolledBack | CellAdmissionStatus::Retired
        ) || released_at < self.reserved_at
        {
            return Err("cell_admission_release_invalid");
        }
        if status == CellAdmissionStatus::RolledBack && self.status != CellAdmissionStatus::Reserved
        {
            return Err("cell_admission_rollback_invalid");
        }
        if status == CellAdmissionStatus::Retired && self.status != CellAdmissionStatus::Committed {
            return Err("cell_admission_retire_invalid");
        }
        required(reason, "cell_admission_release_reason_invalid")?;
        self.status = status;
        self.released_at = Some(released_at);
        self.release_reason = Some(reason.to_owned());
        self.resources_released = true;
        self.digest = self.canonical_digest();
        self.validate()
    }

    pub fn rollback(&mut self, reason: &str, released_at: u64) -> Result<(), &'static str> {
        self.release(CellAdmissionStatus::RolledBack, reason, released_at)
    }

    pub fn retire(&mut self, reason: &str, retired_at: u64) -> Result<(), &'static str> {
        self.release(CellAdmissionStatus::Retired, reason, retired_at)
    }

    pub fn mark_unknown(&mut self, reason: &str, observed_at: u64) -> Result<(), &'static str> {
        self.validate()?;
        if self.status.terminal() || observed_at < self.reserved_at {
            return Err("cell_admission_unknown_invalid");
        }
        required(reason, "cell_admission_unknown_reason_invalid")?;
        self.status = CellAdmissionStatus::Unknown;
        self.release_reason = Some(reason.to_owned());
        self.released_at = Some(observed_at);
        self.digest = self.canonical_digest();
        self.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellAdmissionLedger {
    pub schema: String,
    #[serde(default)]
    pub admissions: BTreeMap<String, CellAdmission>,
}

impl Default for CellAdmissionLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl CellAdmissionLedger {
    pub fn new() -> Self {
        Self {
            schema: CELL_ADMISSION_LEDGER_SCHEMA.to_owned(),
            admissions: BTreeMap::new(),
        }
    }

    pub fn reserve(&mut self, admission: CellAdmission) -> Result<CellAdmission, &'static str> {
        admission.validate()?;
        let paths = admission.resources.canonical_paths()?;
        if let Some(existing) = self.admissions.get(&admission.admission_id) {
            if existing.digest == admission.digest {
                return Ok(existing.clone());
            }
            return Err("cell_admission_duplicate_digest_mismatch");
        }
        if self.admissions.values().any(|existing| {
            !existing.resources_released
                && (existing.resources.cell_id == admission.resources.cell_id
                    || existing.resources.plan_id == admission.resources.plan_id
                    || existing.resources.budget_lease_id == admission.resources.budget_lease_id
                    || existing.resources.capability_grant_id
                        == admission.resources.capability_grant_id
                    || existing.resources.supervision_lease_id
                        == admission.resources.supervision_lease_id)
                || (!existing.resources_released
                    && existing.resources.canonical_paths().is_ok_and(|held| {
                        held.iter().any(|held_path| {
                            paths
                                .iter()
                                .any(|path| crate::path_locks_conflict(path, held_path))
                        })
                    }))
        }) {
            return Err("cell_admission_resource_conflict");
        }
        self.schema = CELL_ADMISSION_LEDGER_SCHEMA.to_owned();
        self.admissions
            .insert(admission.admission_id.clone(), admission.clone());
        Ok(admission)
    }

    pub fn commit(&mut self, admission_id: &str, committed_at: u64) -> Result<(), &'static str> {
        self.admissions
            .get_mut(admission_id)
            .ok_or("cell_admission_not_found")?
            .commit(committed_at)
    }

    pub fn rollback(
        &mut self,
        admission_id: &str,
        reason: &str,
        released_at: u64,
    ) -> Result<(), &'static str> {
        self.admissions
            .get_mut(admission_id)
            .ok_or("cell_admission_not_found")?
            .rollback(reason, released_at)
    }

    pub fn retire(
        &mut self,
        admission_id: &str,
        reason: &str,
        retired_at: u64,
    ) -> Result<(), &'static str> {
        self.admissions
            .get_mut(admission_id)
            .ok_or("cell_admission_not_found")?
            .retire(reason, retired_at)
    }

    pub fn mark_unknown(
        &mut self,
        admission_id: &str,
        reason: &str,
        observed_at: u64,
    ) -> Result<(), &'static str> {
        self.admissions
            .get_mut(admission_id)
            .ok_or("cell_admission_not_found")?
            .mark_unknown(reason, observed_at)
    }

    pub fn get(&self, admission_id: &str) -> Option<&CellAdmission> {
        self.admissions.get(admission_id)
    }
}
