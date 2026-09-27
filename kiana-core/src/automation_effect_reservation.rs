//! AUT-14 read-only Core facade for effect reservation facts.

use kiana_domain::{AutomationEffectReservation, AutomationReservationLedger};

pub fn reserve_automation_effect(
    ledger: &mut AutomationReservationLedger,
    reservation: AutomationEffectReservation,
) -> Result<AutomationEffectReservation, &'static str> {
    ledger.reserve(reservation)
}
