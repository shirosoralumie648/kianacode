//! INT-25 Core read-only mapping/artifact/cursor validation facade.

use kiana_domain::{validate_connector_mapping, ConnectorObjectMapping};

pub fn validate_connector_object_mapping(mapping: &ConnectorObjectMapping) -> Result<(), String> {
    validate_connector_mapping(mapping)
}
