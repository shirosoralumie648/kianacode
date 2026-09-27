//! INT-28 Core read-only connector surface DTO validation facade.

use kiana_domain::{
    validate_connector_surface_query, validate_connector_surface_response, ConnectorSurfaceQuery,
    ConnectorSurfaceResponse,
};

pub fn validate_connector_query(query: &ConnectorSurfaceQuery) -> Result<(), String> {
    validate_connector_surface_query(query)
}

pub fn validate_connector_response(response: &ConnectorSurfaceResponse) -> Result<(), String> {
    validate_connector_surface_response(response)
}
