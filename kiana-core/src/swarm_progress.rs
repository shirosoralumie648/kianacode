//! SW-09 read-only Core facade for progress observations.

use kiana_domain::{SwarmProgressLedger, SwarmProgressObservation};

pub fn record_swarm_progress(
    ledger: &mut SwarmProgressLedger,
    observation: SwarmProgressObservation,
) -> Result<SwarmProgressObservation, &'static str> {
    ledger.record(observation)
}
