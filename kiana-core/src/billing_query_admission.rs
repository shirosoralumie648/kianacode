//! BQ-23 ControlPlane-side admission for the read-only billing query.
//!
//! The query surface reads a BQ-20 projection and returns a summary. It must never become an
//! effect path: no reservation is reserved or released, no approval is staged or consumed, no
//! claim is taken and no billing fact is appended. That rule is enforced here rather than left to
//! reviewer discipline:
//!
//! ```text
//! request (domain BillingQueryRequest) + server-observed effect witness
//!        -> 【this file】BillingQueryAdmission::admit
//!        -> Ok = the query observed no effect; Err = stable refusal code
//! ```
//!
//! The witness is a plain counter set filled by the caller with what its query path actually did.
//! This module does not read the EventStore, the approval store, the broker or a reservation map,
//! and it never runs a query. A caller that lies about the witness is lying to itself: the point is
//! that the shipped path has somewhere to declare "zero effects", and a nonzero declaration is
//! refused with a stable code in the deny-first fixtures.

use kiana_domain::BillingQueryRequest;

pub const BILLING_QUERY_ADMISSION_SCHEMA: &str = "kiana.billing-query-admission.v1";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BillingQueryAdmissionError {
    #[error("billing_query_request_invalid:{0}")]
    RequestInvalid(&'static str),
    #[error("billing_query_consumed_claim")]
    ConsumedClaim,
    #[error("billing_query_consumed_approval")]
    ConsumedApproval,
    #[error("billing_query_staged_approval")]
    StagedApproval,
    #[error("billing_query_reserved_budget")]
    ReservedBudget,
    #[error("billing_query_released_reservation")]
    ReleasedReservation,
    #[error("billing_query_wrote_facts")]
    WroteFacts,
}

/// What a query path actually did, as observed by the composition root.
///
/// All counters start at zero and must stay there. `wrote_facts` counts EventLog appends; the
/// projection itself is a read model, so even a "harmless" query fact would be a second write path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BillingQueryEffectWitness {
    pub claims_staged: u64,
    pub approvals_staged: u64,
    pub approvals_consumed: u64,
    pub reservations_reserved: u64,
    pub reservations_released: u64,
    pub facts_appended: u64,
}

impl BillingQueryEffectWitness {
    pub fn none() -> Self {
        Self::default()
    }
}

/// Stateless admission facade for the query path.
pub struct BillingQueryAdmission;

impl BillingQueryAdmission {
    /// Validate the request is the read-only shape, then refuse any observed effect.
    ///
    /// Deny order is deliberate: a request that is not `read_only` is refused before anything is
    /// inspected, then the strongest authority effects (claim, approval) precede budget effects,
    /// and any appended fact is refused last so the code still names that the query wrote.
    pub fn admit(
        request: &BillingQueryRequest,
        witness: &BillingQueryEffectWitness,
    ) -> Result<(), BillingQueryAdmissionError> {
        // Read-only is checked before the generic contract so a mutating request is named as
        // such instead of disappearing into the shared `billing_query_request_invalid` code.
        if !request.read_only {
            return Err(BillingQueryAdmissionError::RequestInvalid(
                "billing_query_request_not_read_only",
            ));
        }
        request
            .validate()
            .map_err(BillingQueryAdmissionError::RequestInvalid)?;
        if witness.claims_staged != 0 {
            return Err(BillingQueryAdmissionError::ConsumedClaim);
        }
        if witness.approvals_consumed != 0 {
            return Err(BillingQueryAdmissionError::ConsumedApproval);
        }
        if witness.approvals_staged != 0 {
            return Err(BillingQueryAdmissionError::StagedApproval);
        }
        if witness.reservations_reserved != 0 {
            return Err(BillingQueryAdmissionError::ReservedBudget);
        }
        if witness.reservations_released != 0 {
            return Err(BillingQueryAdmissionError::ReleasedReservation);
        }
        if witness.facts_appended != 0 {
            return Err(BillingQueryAdmissionError::WroteFacts);
        }
        Ok(())
    }

    /// Answer whether a witness is the zero-effect shape the query path promises.
    pub fn is_read_only(witness: &BillingQueryEffectWitness) -> bool {
        *witness == BillingQueryEffectWitness::none()
    }
}

/// Marker for the source guard: this file owns no fact, approval or reservation write path.
pub const BILLING_QUERY_ADMISSION_NO_EFFECTS: bool = true;
