//! BQ-23 query-surface receipt view: runtime status and business outcome stay separate.
//!
//! The billing query is read-only, so it never produces a receipt of its own. What it *can*
//! report alongside a response is the outcome of the request that produced the projection. That
//! report must keep two different statements apart:
//!
//! - `runtime_status` — what the runtime did (`Completed`, `Failed`, `ResultUnknown`, ...);
//! - `business_outcome` — what happened in the world (invoice paid, credit issued), which only
//!   exists when separate evidence is attached.
//!
//! A `Completed` runtime is not a paid invoice. This module refuses the conflation instead of
//! documenting it: a business outcome is accepted only when the runtime actually completed and
//! at least one evidence reference is bound to it. The view is a plain value; it has no handle to
//! the EventStore, approvals, reservations or the ledger.

use kiana_domain::ExecutionStatus;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const BILLING_RECEIPT_VIEW_SCHEMA: &str = "kiana.billing-receipt-view.v1";
pub const MAX_BUSINESS_EVIDENCE_REFS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BillingReceiptViewError {
    #[error("billing_receipt_view_invalid:{0}")]
    Invalid(String),
    #[error("billing_receipt_business_outcome_empty")]
    BusinessOutcomeEmpty,
    #[error("billing_receipt_business_outcome_without_runtime")]
    BusinessOutcomeWithoutRuntime,
    #[error("billing_receipt_business_outcome_without_evidence")]
    BusinessOutcomeWithoutEvidence,
    #[error("billing_receipt_business_outcome_confirmation_mismatch")]
    BusinessOutcomeConfirmationMismatch,
}

/// Separated runtime/business report attached to a BQ-23 query answer.
///
/// `runtime_completed_is_not_business_outcome` is a constant marker, not a computed flag: the
/// query path never claims a business result merely because a request completed. A caller that
/// wants to assert a business outcome has to carry evidence references, and the constructor
/// rejects the assertion otherwise.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingQueryReceiptView {
    pub schema: String,
    pub runtime_status: ExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business_outcome: Option<String>,
    #[serde(default)]
    pub business_evidence_refs: Vec<String>,
    pub business_outcome_confirmed: bool,
    pub runtime_completed_is_not_business_outcome: bool,
}

impl BillingQueryReceiptView {
    /// Report the runtime status only. This is what the billing query always produces: no
    /// business outcome is inferred from a completed request.
    pub fn runtime_only(runtime_status: ExecutionStatus) -> Self {
        Self {
            schema: BILLING_RECEIPT_VIEW_SCHEMA.to_owned(),
            runtime_status,
            business_outcome: None,
            business_evidence_refs: Vec::new(),
            business_outcome_confirmed: false,
            runtime_completed_is_not_business_outcome: true,
        }
    }

    /// Bind an externally evidenced business outcome to a completed runtime.
    ///
    /// Deny first: a blank outcome, a non-terminal/non-completed runtime, or a missing evidence
    /// reference is refused; the confirmed flag is derived, never taken from the caller.
    pub fn with_business_outcome(
        runtime_status: ExecutionStatus,
        outcome: impl Into<String>,
        evidence_refs: Vec<String>,
    ) -> Result<Self, BillingReceiptViewError> {
        let outcome = outcome.into();
        if outcome.trim().is_empty() {
            return Err(BillingReceiptViewError::BusinessOutcomeEmpty);
        }
        if runtime_status != ExecutionStatus::Completed {
            return Err(BillingReceiptViewError::BusinessOutcomeWithoutRuntime);
        }
        if evidence_refs.is_empty() {
            return Err(BillingReceiptViewError::BusinessOutcomeWithoutEvidence);
        }
        if evidence_refs.len() > MAX_BUSINESS_EVIDENCE_REFS
            || evidence_refs
                .iter()
                .any(|reference| reference.trim().is_empty())
        {
            return Err(BillingReceiptViewError::Invalid(
                "billing_receipt_evidence_invalid".to_owned(),
            ));
        }
        let mut view = Self::runtime_only(runtime_status);
        view.business_outcome = Some(outcome);
        view.business_evidence_refs = evidence_refs;
        view.business_outcome_confirmed = true;
        view.validate()?;
        Ok(view)
    }

    pub fn validate(&self) -> Result<(), BillingReceiptViewError> {
        if self.schema != BILLING_RECEIPT_VIEW_SCHEMA {
            return Err(BillingReceiptViewError::Invalid(
                "billing_receipt_view_header_invalid".to_owned(),
            ));
        }
        if !self.runtime_completed_is_not_business_outcome {
            return Err(BillingReceiptViewError::Invalid(
                "billing_receipt_runtime_business_outcome_conflated".to_owned(),
            ));
        }
        let implied = self.business_outcome.is_some()
            && self.runtime_status == ExecutionStatus::Completed
            && !self.business_evidence_refs.is_empty();
        if self.business_outcome_confirmed != implied {
            return Err(BillingReceiptViewError::BusinessOutcomeConfirmationMismatch);
        }
        if let Some(outcome) = &self.business_outcome {
            if outcome.trim().is_empty() {
                return Err(BillingReceiptViewError::BusinessOutcomeEmpty);
            }
            if self.runtime_status != ExecutionStatus::Completed {
                return Err(BillingReceiptViewError::BusinessOutcomeWithoutRuntime);
            }
            if self.business_evidence_refs.is_empty() {
                return Err(BillingReceiptViewError::BusinessOutcomeWithoutEvidence);
            }
        } else if self.business_outcome_confirmed || !self.business_evidence_refs.is_empty() {
            return Err(BillingReceiptViewError::BusinessOutcomeConfirmationMismatch);
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        kiana_domain::json_digest(&json!({
            "schema": self.schema,
            "runtime_status": self.runtime_status,
            "business_outcome": self.business_outcome,
            "business_evidence_refs": self.business_evidence_refs,
            "business_outcome_confirmed": self.business_outcome_confirmed,
            "runtime_completed_is_not_business_outcome": self.runtime_completed_is_not_business_outcome,
        }))
    }
}

/// Marker for the source guard: the query receipt view owns no fact-writing path.
pub const BILLING_RECEIPT_VIEW_NO_FACT_WRITES: bool = true;
