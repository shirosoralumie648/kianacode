//! INT-31 Core read-only connector pilot gate facade.

use kiana_domain::{validate_connector_pilot, ConnectorPilotGate};

pub fn validate_connector_pilot_gate(gate: &ConnectorPilotGate) -> Result<(), &'static str> {
    validate_connector_pilot(gate)
}
