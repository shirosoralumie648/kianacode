//! Provider-side validation for the additive INT-17 connector quota envelope.
//!
//! The provider only rechecks server-issued facts together with the existing BQ-16 capacity lease.
//! It cannot reserve, settle, release or dispatch on its own and never accepts an alias or raw
//! credential override from a model request.

use kiana_domain::{
    connector_quota_effect_admission, ConnectorQuotaClaim, ConnectorQuotaPolicy,
    ConnectorQuotaReservation,
};

pub const CONNECTOR_QUOTA_PROVIDER_BOUNDARY_SCHEMA: &str =
    "kiana.provider-connector-quota-boundary.v1";

pub fn validate_connector_quota_lease(
    reservation: &ConnectorQuotaReservation,
    claim: &ConnectorQuotaClaim,
    policy: &ConnectorQuotaPolicy,
    capacity_lease_digest: Option<&str>,
    now_unix_ms: u64,
) -> Result<(), String> {
    connector_quota_effect_admission(reservation, claim, policy, now_unix_ms)?;
    if policy.capacity_lease_digest.as_deref() != capacity_lease_digest {
        return Err("connector_quota_capacity_lease_mismatch".to_owned());
    }
    if reservation.key.credential_generation != policy.key.credential_generation {
        return Err("connector_quota_credential_generation_mismatch".to_owned());
    }
    Ok(())
}
