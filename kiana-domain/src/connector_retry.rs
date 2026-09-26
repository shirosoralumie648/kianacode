//! INT-21 connector retry classification and bounded attempt policy.
//!
//! This is a pure decision contract. A `Retry` result never authorizes or dispatches a provider
//! call; the caller must create a fresh connector reservation/permit for `next_attempt`. Unknown
//! effects, non-idempotent writes and authorization/fence failures are deny-first.

use crate::{json_digest, ConnectorIdempotencyMode, ConnectorRetryMode, EffectObservationState};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONNECTOR_RETRY_POLICY_SCHEMA: &str = "kiana.connector-retry-policy.v1";
pub const CONNECTOR_RETRY_OBSERVATION_SCHEMA: &str = "kiana.connector-retry-observation.v1";
pub const CONNECTOR_RETRY_DECISION_SCHEMA: &str = "kiana.connector-retry-decision.v1";
pub const MAX_CONNECTOR_RETRY_ATTEMPTS: u32 = 16;
pub const MAX_CONNECTOR_RETRY_AFTER_MS: u64 = 86_400_000;
pub const MAX_CONNECTOR_RETRY_BACKOFF_MS: u64 = 86_400_000;

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRetryPolicy {
    pub schema: String,
    pub max_attempts: u32,
    pub max_backoff_ms: u64,
    pub deadline_unix_ms: u64,
    pub policy_digest: String,
}

impl ConnectorRetryPolicy {
    pub fn new(
        max_attempts: u32,
        max_backoff_ms: u64,
        deadline_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: CONNECTOR_RETRY_POLICY_SCHEMA.to_owned(),
            max_attempts,
            max_backoff_ms,
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
            || self.deadline_unix_ms == 0
            || !valid_digest(&self.policy_digest)
            || self.policy_digest != self.digest()
        {
            return Err("connector_retry_policy_invalid".to_owned());
        }
        Ok(())
    }

    pub fn classify(
        &self,
        attempt: u32,
        now_unix_ms: u64,
        mode: ConnectorRetryMode,
        idempotency: ConnectorIdempotencyMode,
        observation: &ConnectorRetryObservation,
    ) -> Result<ConnectorRetryDecision, String> {
        self.validate()?;
        observation.validate()?;
        if now_unix_ms >= self.deadline_unix_ms {
            return Ok(ConnectorRetryDecision::deny(
                self,
                ConnectorRetryDenyReason::Deadline,
            ));
        }
        if attempt == 0 || attempt >= self.max_attempts {
            return Ok(ConnectorRetryDecision::deny(
                self,
                ConnectorRetryDenyReason::AttemptBudget,
            ));
        }
        if let Some(reason) = observation.class.deny_reason() {
            return Ok(ConnectorRetryDecision::deny(self, reason));
        }
        if observation.effect_state == EffectObservationState::Unknown {
            return Ok(ConnectorRetryDecision::deny(
                self,
                ConnectorRetryDenyReason::UnknownEffect,
            ));
        }
        if mode == ConnectorRetryMode::Never {
            return Ok(ConnectorRetryDecision::deny(
                self,
                ConnectorRetryDenyReason::RetryModeDisabled,
            ));
        }
        if observation
            .retry_after_ms
            .is_some_and(|value| value > self.max_backoff_ms)
        {
            return Ok(ConnectorRetryDecision::deny(
                self,
                ConnectorRetryDenyReason::RetryAfterTooLarge,
            ));
        }
        match mode {
            ConnectorRetryMode::KnownNoEffect
                if observation.class != ConnectorRetryClass::KnownNoEffect
                    || observation.request_sent
                    || observation.effect_state != EffectObservationState::NoEffect =>
            {
                return Ok(ConnectorRetryDecision::deny(
                    self,
                    ConnectorRetryDenyReason::RetryClassNotAllowed,
                ));
            }
            ConnectorRetryMode::DeclaredIdempotent => {
                if idempotency == ConnectorIdempotencyMode::Forbidden
                    || !observation.declared_idempotent
                    || observation.idempotency_key_digest.is_none()
                {
                    return Ok(ConnectorRetryDecision::deny(
                        self,
                        ConnectorRetryDenyReason::NonIdempotent,
                    ));
                }
                if observation.request_sent
                    && observation.effect_state != EffectObservationState::NoEffect
                {
                    return Ok(ConnectorRetryDecision::deny(
                        self,
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
                        ConnectorRetryDenyReason::RetryClassNotAllowed,
                    ));
                }
            }
            ConnectorRetryMode::KnownNoEffect | ConnectorRetryMode::Never => {}
        }
        let retry_after = observation.retry_after_ms.unwrap_or_default();
        let exponent = attempt.saturating_sub(1).min(10);
        let backoff = 100u64
            .saturating_mul(1u64 << exponent)
            .min(self.max_backoff_ms);
        let delay_ms = backoff.max(retry_after);
        let remaining = self
            .deadline_unix_ms
            .checked_sub(now_unix_ms)
            .ok_or_else(|| "connector_retry_deadline_underflow".to_owned())?;
        if delay_ms >= remaining {
            return Ok(ConnectorRetryDecision::deny(
                self,
                ConnectorRetryDenyReason::Deadline,
            ));
        }
        let next_attempt = attempt
            .checked_add(1)
            .ok_or_else(|| "connector_retry_attempt_overflow".to_owned())?;
        ConnectorRetryDecision::retry(self, next_attempt, delay_ms)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "max_attempts": self.max_attempts,
            "max_backoff_ms": self.max_backoff_ms,
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
    pub decision_digest: String,
}

impl ConnectorRetryDecision {
    fn retry(
        policy: &ConnectorRetryPolicy,
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
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    fn deny(policy: &ConnectorRetryPolicy, reason: ConnectorRetryDenyReason) -> Self {
        let mut decision = Self {
            schema: CONNECTOR_RETRY_DECISION_SCHEMA.to_owned(),
            retry: false,
            next_attempt: 0,
            delay_ms: 0,
            deny_reason: Some(reason),
            policy_digest: policy.policy_digest.clone(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RETRY_DECISION_SCHEMA
            || !valid_digest(&self.policy_digest)
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

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "retry": self.retry,
            "next_attempt": self.next_attempt,
            "delay_ms": self.delay_ms,
            "deny_reason": self.deny_reason,
            "policy_digest": self.policy_digest,
        }))
    }
}
