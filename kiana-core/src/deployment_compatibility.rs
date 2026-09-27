//! Read-only Core facade for the deployment compatibility matrix.

use kiana_domain::DeploymentCompatibilityMatrix;

pub fn validate_deployment_compatibility(
    matrix: &DeploymentCompatibilityMatrix,
) -> Result<(), String> {
    matrix.validate()
}
