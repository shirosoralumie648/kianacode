//! INT-21 connector retry classification and bounded attempt policy.
//!
//! This is a pure decision contract. A `Retry` result never authorizes or dispatches a provider
//! call; the caller must create a fresh connector reservation/permit for `next_attempt`. Unknown
//! effects, non-idempotent writes and authorization/fence failures are deny-first.
//!
//! Every decision is derived from a server-owned [`ConnectorRetryBinding`] (invocation id, attempt,
//! command digest, idempotency key digest and the admitted retry/idempotency modes) instead of
//! caller-supplied modes, so a caller can neither upgrade the retry mode, borrow another
//! invocation's idempotency key nor replay a decision across attempts or invocations. The policy
//! bounds attempts, backoff, the per-attempt timeout window and the absolute deadline.

use crate::{
    json_digest, ConnectorIdempotencyMode, ConnectorInvocationCommand, ConnectorRetryMode,
    EffectObservationState, InvocationId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONNECTOR_RETRY_POLICY_SCHEMA: &str = "kiana.connector-retry-policy.v1";
pub const CONNECTOR_RETRY_OBSERVATION_SCHEMA: &str = "kiana.connector-retry-observation.v1";
pub const CONNECTOR_RETRY_DECISION_SCHEMA: &str = "kiana.connector-retry-decision.v1";
pub const CONNECTOR_RETRY_BINDING_SCHEMA: &str = "kiana.connector-retry-binding.v1";
pub const MAX_CONNECTOR_RETRY_ATTEMPTS: u32 = 16;
pub const MAX_CONNECTOR_RETRY_AFTER_MS: u64 = 86_400_000;
pub const MAX_CONNECTOR_RETRY_BACKOFF_MS: u64 = 86_400_000;
pub const MAX_CONNECTOR_RETRY_ATTEMPT_TIMEOUT_MS: u64 = 3_600_000;
/// First backoff step. Attempt `n` waits `base * 2^(n - 1)`, clamped to the policy bound.
pub const CONNECTOR_RETRY_BASE_BACKOFF_MS: u64 = 100;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn valid_digest_option(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(valid_digest)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRetryClass {
    KnownNoEffect,
    ProviderThrottled,
    Timeout,
    ProviderFailed,
    Unknown,
    ApprovalDenied,
    EpochStale,
    ScopeDenied,
    ValidationFailed,
    Cancelled,
}

impl ConnectorRetryClass {
    const fn deny_reason(self) -> Option<ConnectorRetryDenyReason> {
        match self {
            Self::Unknown => Some(ConnectorRetryDenyReason::UnknownEffect),
            Self::ApprovalDenied => Some(ConnectorRetryDenyReason::ApprovalDenied),
            Self::EpochStale => Some(ConnectorRetryDenyReason::EpochStale),
            Self::ScopeDenied => Some(ConnectorRetryDenyReason::ScopeDenied),
            Self::ValidationFailed => Some(ConnectorRetryDenyReason::ValidationFailed),
            Self::Cancelled => Some(ConnectorRetryDenyReason::Cancelled),
            Self::KnownNoEffect
            | Self::ProviderThrottled
            | Self::Timeout
            | Self::ProviderFailed => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRetryDenyReason {
    RetryModeDisabled,
    AttemptBudget,
    Deadline,
    RetryAfterTooLarge,
    UnknownEffect,
    NonIdempotent,
    RequestAlreadySent,
    RetryClassNotAllowed,
    ApprovalDenied,
    EpochStale,
    ScopeDenied,
    ValidationFailed,
    Cancelled,
}

impl ConnectorRetryDenyReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RetryModeDisabled => "retry_mode_disabled",
            Self::AttemptBudget => "attempt_budget",
            Self::Deadline => "deadline",
            Self::RetryAfterTooLarge => "retry_after_too_large",
            Self::UnknownEffect => "unknown_effect",
            Self::NonIdempotent => "non_idempotent",
            Self::RequestAlreadySent => "request_already_sent",
            Self::RetryClassNotAllowed => "retry_class_not_allowed",
            Self::ApprovalDenied => "approval_denied",
            Self::EpochStale => "epoch_stale",
            Self::ScopeDenied => "scope_denied",
            Self::ValidationFailed => "validation_failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Redacted adapter evidence. It contains no raw provider response or error body.
///
/// The observation is evidence only: the admitted retry mode, attempt and idempotency key belong
/// to the server-owned [`ConnectorRetryBinding`], never to this value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRetryObservation {
    pub schema: String,
    pub class: ConnectorRetryClass,
    pub request_sent: bool,
    pub effect_state: EffectObservationState,
    pub declared_idempotent: bool,
    pub idempotency_key_digest: Option<String>,
    pub retry_after_ms: Option<u64>,
    pub code: String,
    pub observation_digest: String,
}

impl ConnectorRetryObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        class: ConnectorRetryClass,
        request_sent: bool,
        effect_state: EffectObservationState,
        declared_idempotent: bool,
        idempotency_key_digest: Option<String>,
        retry_after_ms: Option<u64>,
        code: impl Into<String>,
    ) -> Result<Self, String> {
        let mut observation = Self {
            schema: CONNECTOR_RETRY_OBSERVATION_SCHEMA.to_owned(),
            class,
            request_sent,
            effect_state,
            declared_idempotent,
            idempotency_key_digest,
            retry_after_ms,
            code: code.into(),
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RETRY_OBSERVATION_SCHEMA
            || required(&self.code, "connector_retry_observation_code", 128).is_err()
            || !valid_digest_option(&self.idempotency_key_digest)
            || self
                .retry_after_ms
                .is_some_and(|value| value > MAX_CONNECTOR_RETRY_AFTER_MS)
            || !valid_digest(&self.observation_digest)
            || self.observation_digest != self.digest()
        {
            return Err("connector_retry_observation_invalid".to_owned());
        }
        if self.declared_idempotent && self.idempotency_key_digest.is_none() {
            return Err("connector_retry_idempotency_key_required".to_owned());
        }
        if self.class == ConnectorRetryClass::Unknown
            && self.effect_state != EffectObservationState::Unknown
        {
            return Err("connector_retry_unknown_state_mismatch".to_owned());
        }
        // A request that was never sent cannot have an effect.  A timed-out or failed attempt that
        // claims an effect without having sent anything is forged evidence, not a retry candidate.
        if !self.request_sent && self.effect_state != EffectObservationState::NoEffect {
            return Err("connector_retry_observation_effect_state_mismatch".to_owned());
        }
        // `retry_after_ms` is a provider throttle instruction; no other class may carry it.
        if self.retry_after_ms.is_some() && self.class != ConnectorRetryClass::ProviderThrottled {
            return Err("connector_retry_observation_retry_after_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "class": self.class,
            "request_sent": self.request_sent,
            "effect_state": self.effect_state,
            "declared_idempotent": self.declared_idempotent,
            "idempotency_key_digest": self.idempotency_key_digest,
            "retry_after_ms": self.retry_after_ms,
            "code": self.code,
        }))
    }
}

/// Server-owned retry identity derived from the admitted invocation command.
///
/// The retry mode and idempotency mode come from the admitted operation contract, the attempt and
/// the idempotency key digest come from the reservation command, and the command digest binds the
/// decision to exactly one admitted command.  Nothing here is caller-supplied on the wire.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRetryBinding {
    pub schema: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub command_digest: String,
    pub idempotency_key_digest: String,
    pub retry_mode: ConnectorRetryMode,
    pub idempotency: ConnectorIdempotencyMode,
    pub binding_digest: String,
}

impl ConnectorRetryBinding {
    pub fn from_command(
        command: &ConnectorInvocationCommand,
        retry_mode: ConnectorRetryMode,
        idempotency: ConnectorIdempotencyMode,
    ) -> Result<Self, String> {
        command.validate()?;
        let mut binding = Self {
            schema: CONNECTOR_RETRY_BINDING_SCHEMA.to_owned(),
            invocation_id: command.invocation_id,
            attempt: command.attempt,
            command_digest: command.command_digest.clone(),
            idempotency_key_digest: command.idempotency_key_digest.clone(),
            retry_mode,
            idempotency,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RETRY_BINDING_SCHEMA
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || !valid_digest(&self.command_digest)
            || !valid_digest(&self.idempotency_key_digest)
            || !valid_digest(&self.binding_digest)
            || self.binding_digest != self.digest()
        {
            return Err("connector_retry_binding_invalid".to_owned());
        }
        // Mirrors the operation contract rule: an operation that requires its own idempotency key
        // cannot also be admitted as provider-declared idempotent.
        if self.retry_mode == ConnectorRetryMode::DeclaredIdempotent
            && self.idempotency == ConnectorIdempotencyMode::Required
        {
            return Err("connector_retry_binding_idempotency_conflict".to_owned());
        }
        Ok(())
    }

    /// Evidence must reference the admitted invocation's idempotency key digest, if it references
    /// one at all.  A provider-declared idempotency key from another invocation is refused.
    pub fn validate_observation(
        &self,
        observation: &ConnectorRetryObservation,
    ) -> Result<(), String> {
        self.validate()?;
        observation.validate()?;
        if let Some(key) = observation.idempotency_key_digest.as_deref() {
            if key != self.idempotency_key_digest {
                return Err("connector_retry_idempotency_key_mismatch".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "command_digest": self.command_digest,
            "idempotency_key_digest": self.idempotency_key_digest,
            "retry_mode": self.retry_mode,
            "idempotency": self.idempotency,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRetryPolicy {
    pub schema: String,
    pub max_attempts: u32,
    pub max_backoff_ms: u64,
    pub attempt_timeout_ms: u64,
    pub deadline_unix_ms: u64,
    pub policy_digest: String,
}

impl ConnectorRetryPolicy {
    pub fn new(
        max_attempts: u32,
        max_backoff_ms: u64,
        deadline_unix_ms: u64,
        attempt_timeout_ms: u64,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: CONNECTOR_RETRY_POLICY_SCHEMA.to_owned(),
            max_attempts,
            max_backoff_ms,
            attempt_timeout_ms,
            deadline_unix_ms,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RETRY_POLICY_SCHEMA
            || self.max_attempts == 0
            || self.max_attempts > MAX_CONNECTOR_RETRY_ATTEMPTS
            || self.max_backoff_ms == 0
            || self.max_backoff_ms > MAX_CONNECTOR_RETRY_BACKOFF_MS
            || self.attempt_timeout_ms == 0
            || self.attempt_timeout_ms > MAX_CONNECTOR_RETRY_ATTEMPT_TIMEOUT_MS
            || self.deadline_unix_ms == 0
            || !valid_digest(&self.policy_digest)
            || self.policy_digest != self.digest()
        {
            return Err("connector_retry_policy_invalid".to_owned());
        }
        Ok(())
    }

    /// Decide whether the supplied evidence admits a new attempt for the bound invocation.
    ///
    /// Deny-first order: deadline, attempt budget, deny classes, unknown effect, disabled retry
    /// mode, oversized `retry_after`, then the mode-specific evidence rules.  A `Retry` decision
    /// is a bounded next-attempt instruction for a *fresh* reservation/permit; it is not an
    /// authorization and it never reuses the consumed attempt.
    pub fn classify(
        &self,
        binding: &ConnectorRetryBinding,
        now_unix_ms: u64,
        observation: &ConnectorRetryObservation,
    ) -> Result<ConnectorRetryDecision, String> {
        self.validate()?;
        binding.validate()?;
        observation.validate()?;
        binding.validate_observation(observation)?;
        let attempt = binding.attempt;
        if now_unix_ms >= self.deadline_unix_ms {
            return Ok(ConnectorRetryDecision::deny(
                self,
                binding,
                ConnectorRetryDenyReason::Deadline,
            ));
        }
        if attempt >= self.max_attempts {
            return Ok(ConnectorRetryDecision::deny(
                self,
                binding,
                ConnectorRetryDenyReason::AttemptBudget,
            ));
        }
        if let Some(reason) = observation.class.deny_reason() {
            return Ok(ConnectorRetryDecision::deny(self, binding, reason));
        }
        if observation.effect_state == EffectObservationState::Unknown {
            return Ok(ConnectorRetryDecision::deny(
                self,
                binding,
                ConnectorRetryDenyReason::UnknownEffect,
            ));
        }
        if binding.retry_mode == ConnectorRetryMode::Never {
            return Ok(ConnectorRetryDecision::deny(
                self,
                binding,
                ConnectorRetryDenyReason::RetryModeDisabled,
            ));
        }
        if observation
            .retry_after_ms
            .is_some_and(|value| value > self.max_backoff_ms)
        {
            return Ok(ConnectorRetryDecision::deny(
                self,
                binding,
                ConnectorRetryDenyReason::RetryAfterTooLarge,
            ));
        }
        match binding.retry_mode {
            ConnectorRetryMode::KnownNoEffect
                if observation.class != ConnectorRetryClass::KnownNoEffect
                    || observation.request_sent
                    || observation.effect_state != EffectObservationState::NoEffect =>
            {
                return Ok(ConnectorRetryDecision::deny(
                    self,
                    binding,
                    ConnectorRetryDenyReason::RetryClassNotAllowed,
                ));
            }
            ConnectorRetryMode::DeclaredIdempotent => {
                if binding.idempotency == ConnectorIdempotencyMode::Forbidden
                    || !observation.declared_idempotent
                    || observation.idempotency_key_digest.is_none()
                {
                    return Ok(ConnectorRetryDecision::deny(
                        self,
                        binding,
                        ConnectorRetryDenyReason::NonIdempotent,
                    ));
                }
                if observation.request_sent
                    && observation.effect_state != EffectObservationState::NoEffect
                {
                    return Ok(ConnectorRetryDecision::deny(
                        self,
                        binding,
                        ConnectorRetryDenyReason::RequestAlreadySent,
                    ));
                }
                if !matches!(
                    observation.class,
                    ConnectorRetryClass::KnownNoEffect
                        | ConnectorRetryClass::ProviderThrottled
                        | ConnectorRetryClass::Timeout
                        | ConnectorRetryClass::ProviderFailed
                ) || observation.effect_state != EffectObservationState::NoEffect
                {
                    return Ok(ConnectorRetryDecision::deny(
                        self,
                        binding,
                        ConnectorRetryDenyReason::RetryClassNotAllowed,
                    ));
                }
            }
            ConnectorRetryMode::KnownNoEffect | ConnectorRetryMode::Never => {}
        }
        let retry_after = observation.retry_after_ms.unwrap_or_default();
        let exponent = attempt.saturating_sub(1).min(10);
        let backoff = CONNECTOR_RETRY_BASE_BACKOFF_MS
            .saturating_mul(1u64 << exponent)
            .min(self.max_backoff_ms);
        let delay_ms = backoff.max(retry_after);
        let remaining = self
            .deadline_unix_ms
            .checked_sub(now_unix_ms)
            .ok_or_else(|| "connector_retry_deadline_underflow".to_owned())?;
        // The new attempt must start *and* fit its own timeout window before the deadline;
        // otherwise the deadline denies instead of the caller starting an attempt that cannot
        // complete.  Backoff alone is not a sufficient bound.
        let window_ms = delay_ms
            .checked_add(self.attempt_timeout_ms)
            .ok_or_else(|| "connector_retry_window_overflow".to_owned())?;
        if window_ms >= remaining {
            return Ok(ConnectorRetryDecision::deny(
                self,
                binding,
                ConnectorRetryDenyReason::Deadline,
            ));
        }
        let next_attempt = attempt
            .checked_add(1)
            .ok_or_else(|| "connector_retry_attempt_overflow".to_owned())?;
        ConnectorRetryDecision::retry(self, binding, next_attempt, delay_ms)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "max_attempts": self.max_attempts,
            "max_backoff_ms": self.max_backoff_ms,
            "attempt_timeout_ms": self.attempt_timeout_ms,
            "deadline_unix_ms": self.deadline_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRetryDecision {
    pub schema: String,
    pub retry: bool,
    pub next_attempt: u32,
    pub delay_ms: u64,
    pub deny_reason: Option<ConnectorRetryDenyReason>,
    pub policy_digest: String,
    pub binding_digest: String,
    pub decision_digest: String,
}

impl ConnectorRetryDecision {
    fn retry(
        policy: &ConnectorRetryPolicy,
        binding: &ConnectorRetryBinding,
        next_attempt: u32,
        delay_ms: u64,
    ) -> Result<Self, String> {
        let mut decision = Self {
            schema: CONNECTOR_RETRY_DECISION_SCHEMA.to_owned(),
            retry: true,
            next_attempt,
            delay_ms,
            deny_reason: None,
            policy_digest: policy.policy_digest.clone(),
            binding_digest: binding.binding_digest.clone(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    fn deny(
        policy: &ConnectorRetryPolicy,
        binding: &ConnectorRetryBinding,
        reason: ConnectorRetryDenyReason,
    ) -> Self {
        let mut decision = Self {
            schema: CONNECTOR_RETRY_DECISION_SCHEMA.to_owned(),
            retry: false,
            next_attempt: 0,
            delay_ms: 0,
            deny_reason: Some(reason),
            policy_digest: policy.policy_digest.clone(),
            binding_digest: binding.binding_digest.clone(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RETRY_DECISION_SCHEMA
            || !valid_digest(&self.policy_digest)
            || !valid_digest(&self.binding_digest)
            || !valid_digest(&self.decision_digest)
            || self.decision_digest != self.digest()
        {
            return Err("connector_retry_decision_invalid".to_owned());
        }
        if self.retry {
            if self.next_attempt == 0 || self.delay_ms == 0 || self.deny_reason.is_some() {
                return Err("connector_retry_decision_invalid".to_owned());
            }
        } else if self.next_attempt != 0 || self.delay_ms != 0 || self.deny_reason.is_none() {
            return Err("connector_retry_denial_invalid".to_owned());
        }
        Ok(())
    }

    pub fn validate_for_policy(&self, policy: &ConnectorRetryPolicy) -> Result<(), String> {
        policy.validate()?;
        self.validate()?;
        if self.policy_digest != policy.policy_digest {
            return Err("connector_retry_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The decision belongs to exactly one admitted attempt: a decision minted for attempt `n`
    /// cannot be applied to another invocation or reused for a different next attempt.
    pub fn validate_for_binding(&self, binding: &ConnectorRetryBinding) -> Result<(), String> {
        binding.validate()?;
        self.validate()?;
        if self.binding_digest != binding.binding_digest {
            return Err("connector_retry_decision_binding_mismatch".to_owned());
        }
        if self.retry {
            let expected = binding
                .attempt
                .checked_add(1)
                .ok_or_else(|| "connector_retry_attempt_overflow".to_owned())?;
            if self.next_attempt != expected {
                return Err("connector_retry_decision_attempt_mismatch".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "retry": self.retry,
            "next_attempt": self.next_attempt,
            "delay_ms": self.delay_ms,
            "deny_reason": self.deny_reason,
            "policy_digest": self.policy_digest,
            "binding_digest": self.binding_digest,
        }))
    }
}
