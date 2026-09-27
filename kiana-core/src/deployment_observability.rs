//! Read-only Core facade for operation-bound observability evidence.

use kiana_domain::LifecycleEvidenceBundle;

pub fn validate_deployment_observability(bundle: &LifecycleEvidenceBundle) -> Result<(), String> {
    bundle.validate()
}
