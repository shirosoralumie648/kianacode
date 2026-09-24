//! ControlPlane-only BQ-18 fallback re-admission boundary.
//!
//! The core owns the order in which authority, data, budget, credential, route and price facts
//! are rechecked.  This module is intentionally pure and does not call a provider, Broker,
//! transport, EventLog writer or hidden retry loop; the caller must commit the normal permit facts
//! before the existing ProviderGateway path can dispatch.

use super::ControlPlane;
use kiana_domain::{
    admit_fallback_attempt, FallbackAdmissionCandidate, FallbackAdmissionRequest,
    FallbackAttemptAdmission, FallbackRouteAllowlist,
};

/// Typed result of a ControlPlane fallback admission.  Keeping this wrapper in core makes the
/// authority boundary explicit to source guards while the domain function remains reusable for
/// deny-first fixtures.
pub struct ControlPlaneFallbackAdmission;

impl ControlPlaneFallbackAdmission {
    pub fn re_admit(
        request: &FallbackAdmissionRequest,
        candidate: &FallbackAdmissionCandidate,
        allowlist: &FallbackRouteAllowlist,
        attempt_id: kiana_domain::AttemptId,
    ) -> Result<FallbackAttemptAdmission, String> {
        // This is the only admission call.  It produces a fresh attempt; no adapter is invoked.
        admit_fallback_attempt(request, candidate, allowlist, attempt_id)
    }
}

impl ControlPlane {
    /// Recheck one allow-listed fallback through the same ControlPlane authority boundary as the
    /// primary attempt.  The returned admission is only a prepared value; Broker/Provider remain
    /// responsible for the existing opaque permit and effect-time checks.
    pub fn re_admit_model_fallback(
        &self,
        request: &FallbackAdmissionRequest,
        candidate: &FallbackAdmissionCandidate,
        allowlist: &FallbackRouteAllowlist,
        attempt_id: kiana_domain::AttemptId,
    ) -> Result<FallbackAttemptAdmission, String> {
        let _ = self;
        ControlPlaneFallbackAdmission::re_admit(request, candidate, allowlist, attempt_id)
    }
}
