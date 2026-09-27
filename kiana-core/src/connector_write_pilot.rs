//! INT-32 Core read-only controlled write pilot gate facade.

use kiana_domain::{validate_connector_write_pilot, ConnectorWritePilotGate};

pub fn validate_connector_write_pilot_gate(
    gate: &ConnectorWritePilotGate,
) -> Result<(), &'static str> {
    validate_connector_write_pilot(gate)
}
