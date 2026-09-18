//! Stable billing/quota/cost identities and state/error contracts.
//!
//! These are inert domain values. They do not authorize a capability, reserve a provider slot or
//! claim that an external invoice was paid; those decisions remain in ControlPlane/EventLog
//! adapters introduced by later BQ steps.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const BILLING_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const BILLING_CONTRACT_SCHEMA: &str = "kiana.billing-contract.v1";
pub const BILLING_UNKNOWN_REASON_SCHEMA: &str = "kiana.billing-unknown-reason.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingUnknownReason {
    Absent,
    Partial,
    ProviderUnreported,
    Malformed,
    RateCardMissing,
    ReceiptMissing,
    ClockUntrusted,
    ResultUnknown,
    ReconciliationRequired,
    Unsupported,
}

impl BillingUnknownReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Partial => "partial",
            Self::ProviderUnreported => "provider_unreported",
            Self::Malformed => "malformed",
            Self::RateCardMissing => "rate_card_missing",
            Self::ReceiptMissing => "receipt_missing",
            Self::ClockUntrusted => "clock_untrusted",
            Self::ResultUnknown => "result_unknown",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::Unsupported => "unsupported",
        }
    }

    pub const fn requires_reconciliation(self) -> bool {
        matches!(
            self,
            Self::ProviderUnreported
                | Self::ReceiptMissing
                | Self::ResultUnknown
                | Self::ReconciliationRequired
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingState {
    Reserved,
    Observed,
    Settled,
    Released,
    Unknown,
    Expired,
    Corrected,
}

impl BillingState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Settled | Self::Released | Self::Unknown | Self::Expired
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, &'static str> {
        if self == next {
            return Err("billing_state_duplicate");
        }
        let allowed = matches!(
            (self, next),
            (
                Self::Reserved,
                Self::Observed | Self::Released | Self::Unknown | Self::Expired
            ) | (
                Self::Observed,
                Self::Settled | Self::Released | Self::Unknown
            ) | (Self::Settled, Self::Corrected)
                | (
                    Self::Unknown,
                    Self::Settled | Self::Released | Self::Corrected
                )
        );
        if allowed {
            Ok(next)
        } else {
            Err("billing_state_transition_invalid")
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingErrorCode {
    SchemaUnsupported,
    IdentityInvalid,
    UnknownUsage,
    InvalidUsage,
    StateConflict,
    ReservationConflict,
    BudgetExceeded,
    RateCardMissing,
    ReceiptMissing,
    CurrencyMismatch,
    ArithmeticOverflow,
    ClockUntrusted,
    AuthorityStale,
    ReconciliationRequired,
    Unsupported,
}

impl BillingErrorCode {
    pub const ALL: &'static [Self] = &[
        Self::SchemaUnsupported,
        Self::IdentityInvalid,
        Self::UnknownUsage,
        Self::InvalidUsage,
        Self::StateConflict,
        Self::ReservationConflict,
        Self::BudgetExceeded,
        Self::RateCardMissing,
        Self::ReceiptMissing,
        Self::CurrencyMismatch,
        Self::ArithmeticOverflow,
        Self::ClockUntrusted,
        Self::AuthorityStale,
        Self::ReconciliationRequired,
        Self::Unsupported,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SchemaUnsupported => "schema_unsupported",
            Self::IdentityInvalid => "identity_invalid",
            Self::UnknownUsage => "unknown_usage",
            Self::InvalidUsage => "invalid_usage",
            Self::StateConflict => "state_conflict",
            Self::ReservationConflict => "reservation_conflict",
            Self::BudgetExceeded => "budget_exceeded",
            Self::RateCardMissing => "rate_card_missing",
            Self::ReceiptMissing => "receipt_missing",
            Self::CurrencyMismatch => "currency_mismatch",
            Self::ArithmeticOverflow => "arithmetic_overflow",
            Self::ClockUntrusted => "clock_untrusted",
            Self::AuthorityStale => "authority_stale",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::Unsupported => "unsupported",
        }
    }

    pub fn parse(value: &str) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.as_str() == value.trim())
            .unwrap_or(Self::Unsupported)
    }
}

impl std::fmt::Display for BillingErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingContractHeader {
    pub schema: String,
    pub version: SchemaVersion,
    pub state: BillingState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<BillingUnknownReason>,
    pub contract_digest: String,
}

impl BillingContractHeader {
    pub fn new(state: BillingState, unknown_reason: Option<BillingUnknownReason>) -> Self {
        let mut header = Self {
            schema: BILLING_CONTRACT_SCHEMA.to_owned(),
            version: BILLING_SCHEMA_VERSION,
            state,
            unknown_reason,
            contract_digest: String::new(),
        };
        header.contract_digest = header.digest();
        header
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_CONTRACT_SCHEMA
            || !self.version.is_compatible_with(&BILLING_SCHEMA_VERSION)
            || self.contract_digest != self.digest()
            || !self.contract_digest.starts_with("sha256:")
            || (self.state == BillingState::Unknown && self.unknown_reason.is_none())
            || (self.state != BillingState::Unknown && self.unknown_reason.is_some())
        {
            return Err("billing_contract_header_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "state": self.state,
            "unknown_reason": self.unknown_reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderReceiptRef(String);

impl ProviderReceiptRef {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty()
            || value.len() > 512
            || value.contains(['\0', '\r', '\n'])
            || value.to_ascii_lowercase().contains("secret")
        {
            return Err("provider_receipt_ref_invalid".to_owned());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
