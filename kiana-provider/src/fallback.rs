//! Provider-side validation for BQ-18 fallback attempts.
//!
//! The provider never chooses a fallback and never mints a permit.  It only checks that the
//! route/credential/price bindings produced by ControlPlane still match the candidate prepared at
//! the existing gateway effect boundary.

use kiana_domain::{FallbackAttemptAdmission, ModelError, ModelRoute};

pub const FALLBACK_PROVIDER_BOUNDARY_SCHEMA: &str = "kiana.fallback-provider-boundary.v1";

pub fn validate_fallback_attempt(
    admission: &FallbackAttemptAdmission,
    route: &ModelRoute,
    credential_revision: Option<&str>,
    permit_digest: &str,
) -> Result<(), ModelError> {
    admission.validate().map_err(ModelError::invalid)?;
    if route.digest() != admission.route_digest
        || route.provider_id != admission.route.provider_id
        || route.model_id != admission.route.model_id
    {
        return Err(ModelError::invalid("fallback_route_admission_drift"));
    }
    if credential_revision != Some(admission.credential_revision.as_str()) {
        return Err(ModelError::invalid("fallback_credential_revision_drift"));
    }
    if permit_digest != admission.permit_digest {
        return Err(ModelError::invalid("fallback_permit_admission_drift"));
    }
    Ok(())
}
