//! Read-only Core facade for deployment configuration snapshots.

use kiana_domain::DeploymentConfigSnapshot;

pub fn validate_deployment_config_snapshot(
    snapshot: &DeploymentConfigSnapshot,
) -> Result<(), String> {
    snapshot.validate()
}
