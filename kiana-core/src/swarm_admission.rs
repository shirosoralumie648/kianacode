//! SW-05 read-only ControlPlane facade for atomic swarm admission evidence.

use kiana_domain::{SwarmAdmissionLedger, SwarmAdmissionReceipt, SwarmAdmissionRequest};

pub fn admit_swarm_resources(
    ledger: &mut SwarmAdmissionLedger,
    request: &SwarmAdmissionRequest,
) -> Result<SwarmAdmissionReceipt, &'static str> {
    ledger.admit(request)
}
