//! Core adapter for packet-level acceptance facts.
use kiana_domain::{PacketAcceptanceLedger, PacketAcceptanceRequest};
pub(crate) fn record_packet_acceptance(
    ledger: &mut PacketAcceptanceLedger,
    request: PacketAcceptanceRequest,
) -> Result<(), &'static str> {
    ledger.record(request)
}
