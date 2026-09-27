//! DaemonHost read-only route for observability evidence validation.

use kiana_domain::LifecycleEvidenceBundle;
use kiana_ports::PortError;

pub(crate) fn validate(bundle: &LifecycleEvidenceBundle) -> Result<(), PortError> {
    kiana_core::validate_deployment_observability(bundle).map_err(PortError::Failed)
}
