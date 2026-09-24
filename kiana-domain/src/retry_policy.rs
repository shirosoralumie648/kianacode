//! BQ-17 retry classification and per-attempt reservation contracts.
//!
//! Retry is a decision about a *new* model attempt.  This module only validates the evidence
//! needed to make that decision and tracks one server-owned reservation; it never sends a
//! provider request, consumes a ControlPlane permit or releases a quota lease.  Adapters must
//! perform a fresh preparation/admission for every returned `RetryDecision::Retry`.

use crate::{
    json_digest, AttemptId, ModelError, ModelRetryClass, ModelSideEffectState, QuotaReservationId,
    RunId, SchemaVersion,
};
use serde::{Deserialize, Serialize};

pub const RETRY_POLICY_SCHEMA: &str = "kiana.retry-policy.v1";
pub const RETRY_ATTEMPT_RESERVATION_SCHEMA: &str = "kiana.retry-attempt-reservation.v1";
pub const RETRY_POLICY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The upper bound is deliberately smaller than the model wall-time maximum.  A larger
/// Retry-After remains visible in the source error but cannot silently schedule a retry.
pub const MAX_RETRY_AFTER_MS: u64 = 86_400_000;
pub const MAX_RETRY_ATTEMPTS: u32 = 16;
pub const MAX_RETRY_REQUESTS: u32 = 16;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// Why an otherwise bounded retry was refused.  These reasons are policy data, not permission
/// to retry; callers still need to allocate a new attempt and re-enter ControlPlane admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryDenyReason {
    RetryClassNotAllowed,
    RequestAlreadySent,
    SideEffectUnknown,
    ObservedDelta,
    NonIdempotent,
    Cancelled,
    AttemptBudget,
    RequestBudget,
    Deadline,
    RetryAfterTooLarge,
}

impl RetryDenyReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RetryClassNotAllowed => "retry_class_not_allowed",
            Self::RequestAlreadySent => "request_already_sent",
            Self::SideEffectUnknown => "side_effect_unknown",
            Self::ObservedDelta => "observed_delta",
            Self::NonIdempotent => "non_idempotent",
            Self::Cancelled => "cancelled",
            Self::AttemptBudget => "attempt_budget",
            Self::RequestBudget => "request_budget",
            Self::Deadline => "deadline",
            Self::RetryAfterTooLarge => "retry_after_too_large",
        }
    }
}

/// Typed evidence consumed by `RetryPolicy`.  The policy deliberately does not inspect error
/// messages: authentication, TLS and arbitrary provider failures must be represented as
/// `ModelRetryClass::Never` by the provider boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryObservation {
    pub class: ModelRetryClass,
    pub code: String,
    pub request_sent: bool,
    pub side_effect_state: ModelSideEffectState,
    pub observed_delta: bool,
    pub idempotent: bool,
    pub retry_after_ms: Option<u64>,
}

impl RetryObservation {
    pub fn from_model_error(error: &ModelError, observed_delta: bool, idempotent: bool) -> Self {
        Self {
            class: error.retry_class,
            code: error.code.clone(),
            request_sent: error.request_sent,
            side_effect_state: error.side_effect_state,
            observed_delta,
            idempotent,
            retry_after_ms: error.retry_after_ms,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        required(&self.code, "retry_observation_code", 128)?;
        Ok(())
    }

    fn safe_pre_send(&self) -> bool {
        self.class == ModelRetryClass::BeforeSend
            && !self.request_sent
            && self.side_effect_state == ModelSideEffectState::None
    }

    fn safe_rejection(&self) -> bool {
        self.class == ModelRetryClass::Rejected
            && self.request_sent
            && self.side_effect_state == ModelSideEffectState::None
            && matches!(
                self.code.as_str(),
                "provider_http_429" | "provider_http_408"
            )
    }
}

/// Bounded retry policy shared by Runner fixtures and provider adapters.  `deadline_unix_ms` is
/// the same absolute deadline used for provider admission and transport; no separate backoff
/// budget exists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicy {
    pub schema: String,
    pub version: SchemaVersion,
    pub max_attempts: u32,
    pub max_requests: u32,
    pub max_backoff_ms: u64,
    pub deadline_unix_ms: u64,
    pub policy_digest: String,
}

impl RetryPolicy {
    pub fn new(
        max_attempts: u32,
        max_requests: u32,
        max_backoff_ms: u64,
        deadline_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: RETRY_POLICY_SCHEMA.to_owned(),
            version: RETRY_POLICY_VERSION,
            max_attempts,
            max_requests,
            max_backoff_ms,
            deadline_unix_ms,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRY_POLICY_SCHEMA
            || !self.version.is_compatible_with(&RETRY_POLICY_VERSION)
            || self.max_attempts == 0
            || self.max_attempts > MAX_RETRY_ATTEMPTS
            || self.max_requests == 0
            || self.max_requests > MAX_RETRY_REQUESTS
            || self.max_requests < self.max_attempts
            || self.max_backoff_ms == 0
            || self.max_backoff_ms > MAX_RETRY_AFTER_MS
            || self.deadline_unix_ms == 0
            || self.policy_digest != self.digest()
        {
            return Err("retry_policy_invalid".to_owned());
        }
        digest(&self.policy_digest, "retry_policy_digest")
    }

    /// Classify one failed attempt.  `now_unix_ms` and `request_count` are supplied by the
    /// authority so a retry cannot reset a task budget by constructing another policy locally.
    pub fn classify(
        &self,
        attempt: u32,
        request_count: u32,
        now_unix_ms: u64,
        observation: &RetryObservation,
    ) -> Result<RetryDecision, String> {
        self.validate()?;
        observation.validate()?;
        if now_unix_ms >= self.deadline_unix_ms {
            return Ok(RetryDecision::deny(RetryDenyReason::Deadline));
        }
        if attempt == 0 || attempt >= self.max_attempts {
            return Ok(RetryDecision::deny(RetryDenyReason::AttemptBudget));
        }
        if request_count >= self.max_requests {
            return Ok(RetryDecision::deny(RetryDenyReason::RequestBudget));
        }
        if observation.observed_delta {
            return Ok(RetryDecision::deny(RetryDenyReason::ObservedDelta));
        }
        if !observation.idempotent {
            return Ok(RetryDecision::deny(RetryDenyReason::NonIdempotent));
        }
        if observation.side_effect_state == ModelSideEffectState::Unknown {
            return Ok(RetryDecision::deny(RetryDenyReason::SideEffectUnknown));
        }
        if !(observation.safe_pre_send() || observation.safe_rejection()) {
            return Ok(RetryDecision::deny(if observation.request_sent {
                RetryDenyReason::RequestAlreadySent
            } else {
                RetryDenyReason::RetryClassNotAllowed
            }));
        }
        let retry_after = observation.retry_after_ms.unwrap_or_default();
        if retry_after > MAX_RETRY_AFTER_MS {
            return Ok(RetryDecision::deny(RetryDenyReason::RetryAfterTooLarge));
        }
        let exponent = attempt.saturating_sub(1).min(5);
        let backoff = 100u64
            .saturating_mul(1u64 << exponent)
            .min(self.max_backoff_ms);
        let delay_ms = backoff.max(retry_after);
        let remaining = self
            .deadline_unix_ms
            .checked_sub(now_unix_ms)
            .ok_or_else(|| "retry_deadline_underflow".to_owned())?;
        if delay_ms >= remaining {
            return Ok(RetryDecision::deny(RetryDenyReason::Deadline));
        }
        Ok(RetryDecision::retry(attempt + 1, delay_ms))
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "max_attempts": self.max_attempts,
            "max_requests": self.max_requests,
            "max_backoff_ms": self.max_backoff_ms,
            "deadline_unix_ms": self.deadline_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryDecision {
    pub retry: bool,
    pub next_attempt: u32,
    pub delay_ms: u64,
    pub deny_reason: Option<RetryDenyReason>,
}

impl RetryDecision {
    fn retry(next_attempt: u32, delay_ms: u64) -> Self {
        Self {
            retry: true,
            next_attempt,
            delay_ms,
            deny_reason: None,
        }
    }

    fn deny(reason: RetryDenyReason) -> Self {
        Self {
            retry: false,
            next_attempt: 0,
            delay_ms: 0,
            deny_reason: Some(reason),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.retry {
            if self.next_attempt == 0
                || self.delay_ms == 0
                || self.delay_ms > MAX_RETRY_AFTER_MS
                || self.deny_reason.is_some()
            {
                return Err("retry_decision_invalid".to_owned());
            }
        } else if self.next_attempt != 0 || self.delay_ms != 0 || self.deny_reason.is_none() {
            return Err("retry_denial_invalid".to_owned());
        }
        Ok(())
    }
}

/// Terminal state for one retry attempt reservation.  A reservation cannot become both
/// cancelled and settled, which closes the cancellation/response race at the value boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryAttemptState {
    Reserved,
    Sent,
    Settled,
    Unknown,
    Cancelled,
}

impl RetryAttemptState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Settled | Self::Unknown | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryAttemptReservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub reservation_id: QuotaReservationId,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub retry_ordinal: u32,
    pub requested_tokens: u64,
    pub deadline_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_lease_digest: Option<String>,
    pub state: RetryAttemptState,
    pub request_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_tokens: Option<u64>,
    pub revision: u64,
    pub reservation_digest: String,
}

impl RetryAttemptReservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reservation_id: QuotaReservationId,
        run_id: RunId,
        attempt_id: AttemptId,
        retry_ordinal: u32,
        requested_tokens: u64,
        deadline_unix_ms: u64,
        capacity_lease_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut reservation = Self {
            schema: RETRY_ATTEMPT_RESERVATION_SCHEMA.to_owned(),
            version: RETRY_POLICY_VERSION,
            reservation_id,
            run_id,
            attempt_id,
            retry_ordinal,
            requested_tokens,
            deadline_unix_ms,
            capacity_lease_digest,
            state: RetryAttemptState::Reserved,
            request_count: 0,
            observed_tokens: None,
            revision: 1,
            reservation_digest: String::new(),
        };
        reservation.reservation_digest = reservation.digest();
        reservation.validate()?;
        Ok(reservation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRY_ATTEMPT_RESERVATION_SCHEMA
            || !self.version.is_compatible_with(&RETRY_POLICY_VERSION)
            || self.reservation_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.retry_ordinal == 0
            || self.requested_tokens == 0
            || self.deadline_unix_ms == 0
            || self.request_count > 1
            || self.revision == 0
            || matches!(
                self.state,
                RetryAttemptState::Reserved | RetryAttemptState::Cancelled
            ) && self.request_count != 0
            || matches!(
                self.state,
                RetryAttemptState::Sent | RetryAttemptState::Settled | RetryAttemptState::Unknown
            ) && self.request_count != 1
            || self
                .observed_tokens
                .is_some_and(|value| value > self.requested_tokens)
            || self.reservation_digest != self.digest()
        {
            return Err("retry_attempt_reservation_invalid".to_owned());
        }
        if let Some(lease) = self.capacity_lease_digest.as_deref() {
            digest(lease, "retry_capacity_lease_digest")?;
        }
        digest(&self.reservation_digest, "retry_attempt_reservation_digest")
    }

    pub fn dispatch(&mut self) -> Result<(), String> {
        self.validate()?;
        if self.state != RetryAttemptState::Reserved {
            return Err("retry_attempt_dispatch_not_reserved".to_owned());
        }
        self.state = RetryAttemptState::Sent;
        self.request_count = 1;
        self.bump()
    }

    pub fn settle(&mut self, observed_tokens: Option<u64>) -> Result<(), String> {
        self.validate()?;
        if self.state != RetryAttemptState::Sent {
            return Err("retry_attempt_settle_state_invalid".to_owned());
        }
        if observed_tokens.is_some_and(|value| value > self.requested_tokens) {
            return Err("retry_attempt_usage_exceeds_reservation".to_owned());
        }
        self.observed_tokens = observed_tokens;
        self.state = RetryAttemptState::Settled;
        self.bump()
    }

    pub fn mark_unknown(&mut self, observed_tokens: Option<u64>) -> Result<(), String> {
        self.validate()?;
        if self.state != RetryAttemptState::Sent {
            return Err("retry_attempt_unknown_state_invalid".to_owned());
        }
        if observed_tokens.is_some_and(|value| value > self.requested_tokens) {
            return Err("retry_attempt_usage_exceeds_reservation".to_owned());
        }
        self.observed_tokens = observed_tokens;
        self.state = RetryAttemptState::Unknown;
        self.bump()
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        self.validate()?;
        if self.state != RetryAttemptState::Reserved {
            return Err("retry_attempt_cancel_race_terminal".to_owned());
        }
        self.state = RetryAttemptState::Cancelled;
        self.bump()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "reservation_id": self.reservation_id,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "retry_ordinal": self.retry_ordinal,
            "requested_tokens": self.requested_tokens,
            "deadline_unix_ms": self.deadline_unix_ms,
            "capacity_lease_digest": self.capacity_lease_digest,
            "state": self.state,
            "request_count": self.request_count,
            "observed_tokens": self.observed_tokens,
            "revision": self.revision,
        }))
    }

    fn bump(&mut self) -> Result<(), String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "retry_attempt_revision_overflow".to_owned())?;
        self.reservation_digest = self.digest();
        self.validate()
    }
}
