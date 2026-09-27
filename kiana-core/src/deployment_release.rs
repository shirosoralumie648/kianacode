//! Read-only Core facade for deployment release evidence.

use kiana_domain::{DeploymentProfile, DeploymentReleaseBundle};

pub fn validate_deployment_release(
    profile: DeploymentProfile,
    bundle: &DeploymentReleaseBundle,
) -> Result<(), String> {
    if bundle.profile != profile {
        return Err("deployment_release_profile_mismatch".to_owned());
    }
    bundle.validate()
}
