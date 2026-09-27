//! INT-26 Core read-only connector propagation validation facade.

use kiana_domain::{validate_connector_propagation, ConnectorPropagationFact};

pub fn validate_connector_propagation_fact(
    fact: &ConnectorPropagationFact,
) -> Result<(), &'static str> {
    validate_connector_propagation(fact)
}
