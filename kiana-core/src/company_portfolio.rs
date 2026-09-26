//! Core adapter for Company portfolio capacity/cost facts.

use kiana_domain::{CompanyCapacityReservation, CompanyCostState, CompanyPortfolioLedger};

pub(crate) fn reserve_company_capacity(
    ledger: &mut CompanyPortfolioLedger,
    reservation: CompanyCapacityReservation,
) -> Result<(), &'static str> {
    ledger.reserve(reservation)
}

pub(crate) fn settle_company_capacity(
    ledger: &mut CompanyPortfolioLedger,
    reservation_id: &str,
    state: CompanyCostState,
    source_ref: impl Into<String>,
) -> Result<(), &'static str> {
    ledger.settle(reservation_id, state, source_ref)
}
