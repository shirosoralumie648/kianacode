//! Company portfolio capacity and project cost-state contract.
//!
//! This ledger separates reserved/spent/released/unknown capacity facts. Unknown holds capacity;
//! a new attempt or project cannot bypass an existing reservation by changing its identity.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const COMPANY_PORTFOLIO_SCHEMA: &str = "kiana.company-portfolio.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 512 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyCostState {
    Reserved,
    Spent,
    Released,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyCapacityReservation {
    pub schema: String,
    pub reservation_id: String,
    pub organization_id: String,
    pub project_id: String,
    pub run_id: String,
    pub amount: u64,
    pub priority: u32,
    pub authority_epoch: u64,
    pub state: CompanyCostState,
    pub source_ref: String,
    pub digest: String,
}

impl CompanyCapacityReservation {
    fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PORTFOLIO_SCHEMA || self.amount == 0 || self.authority_epoch == 0
        {
            return Err("company_capacity_reservation_header_invalid");
        }
        for (value, field) in [
            (
                &self.reservation_id,
                "company_capacity_reservation_id_required",
            ),
            (
                &self.organization_id,
                "company_capacity_organization_required",
            ),
            (&self.project_id, "company_capacity_project_required"),
            (&self.run_id, "company_capacity_run_required"),
            (&self.source_ref, "company_capacity_source_required"),
        ] {
            required(value, field)?;
        }
        digest(&self.digest, "company_capacity_reservation_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_capacity_reservation_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "reservation_id": self.reservation_id,
            "organization_id": self.organization_id,
            "project_id": self.project_id,
            "run_id": self.run_id,
            "amount": self.amount,
            "priority": self.priority,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "source_ref": self.source_ref,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompanyPortfolioLedger {
    pub organization_limits: BTreeMap<String, u64>,
    pub project_limits: BTreeMap<String, u64>,
    pub reservations: BTreeMap<String, CompanyCapacityReservation>,
}

impl CompanyPortfolioLedger {
    pub fn reserve(&mut self, reservation: CompanyCapacityReservation) -> Result<(), &'static str> {
        reservation.validate()?;
        if let Some(existing) = self.reservations.get(&reservation.reservation_id) {
            if existing.digest == reservation.digest {
                return Ok(());
            }
            return Err("company_capacity_duplicate_digest_mismatch");
        }
        let organization_limit = *self
            .organization_limits
            .get(&reservation.organization_id)
            .ok_or("company_organization_budget_missing")?;
        let project_limit = *self
            .project_limits
            .get(&reservation.project_id)
            .ok_or("company_project_budget_missing")?;
        if reservation.amount > project_limit
            || self.project_total(&reservation.project_id)
                > project_limit.saturating_sub(reservation.amount)
            || self.organization_total(&reservation.organization_id)
                > organization_limit.saturating_sub(reservation.amount)
        {
            return Err("company_capacity_exceeded");
        }
        self.reservations
            .insert(reservation.reservation_id.clone(), reservation);
        Ok(())
    }

    pub fn settle(
        &mut self,
        reservation_id: &str,
        state: CompanyCostState,
        source_ref: impl Into<String>,
    ) -> Result<(), &'static str> {
        let source_ref = source_ref.into();
        if !matches!(
            state,
            CompanyCostState::Spent | CompanyCostState::Released | CompanyCostState::Unknown
        ) {
            return Err("company_capacity_settlement_state_invalid");
        }
        let reservation = self
            .reservations
            .get_mut(reservation_id)
            .ok_or("company_capacity_reservation_not_found")?;
        if reservation.state != CompanyCostState::Reserved {
            if reservation.state == state && reservation.source_ref == source_ref {
                return Ok(());
            }
            return Err("company_capacity_settlement_conflict");
        }
        required(&source_ref, "company_capacity_source_required")?;
        reservation.state = state;
        reservation.source_ref = source_ref;
        reservation.digest = reservation.canonical_digest();
        Ok(())
    }

    pub fn project_total(&self, project_id: &str) -> u64 {
        self.reservations
            .values()
            .filter(|reservation| {
                reservation.project_id == project_id
                    && matches!(
                        reservation.state,
                        CompanyCostState::Reserved | CompanyCostState::Unknown
                    )
            })
            .map(|reservation| reservation.amount)
            .sum()
    }

    pub fn organization_total(&self, organization_id: &str) -> u64 {
        self.reservations
            .values()
            .filter(|reservation| {
                reservation.organization_id == organization_id
                    && matches!(
                        reservation.state,
                        CompanyCostState::Reserved | CompanyCostState::Unknown
                    )
            })
            .map(|reservation| reservation.amount)
            .sum()
    }
}
