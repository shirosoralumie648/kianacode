//! Quota reservation identity/state contract for durable CAS adapters.

use crate::{
    json_digest, AttemptId, QuotaGroupKey, QuotaReservationId, QuotaWindow, RunId, SchemaVersion,
};
use serde::{Deserialize, Serialize};

pub const QUOTA_RESERVATION_SCHEMA: &str = "kiana.quota-reservation.v1";
pub const QUOTA_RESERVATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaReservationState {
    Reserved,
    Settled,
    Released,
    Unknown,
    Expired,
}

impl QuotaReservationState {
    pub fn transition(self, next: Self) -> Result<Self, String> {
        if self == next {
            return Err("quota_reservation_state_duplicate".to_owned());
        }
        let allowed = matches!(
            (self, next),
            (
                Self::Reserved,
                Self::Settled | Self::Released | Self::Unknown | Self::Expired
            ) | (Self::Unknown, Self::Settled | Self::Released)
        );
        allowed
            .then_some(next)
            .ok_or_else(|| "quota_reservation_state_transition_invalid".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaReservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub reservation_id: QuotaReservationId,
    pub group: QuotaGroupKey,
    pub window: QuotaWindow,
    pub owner_run: RunId,
    pub owner_attempt: AttemptId,
    pub authority_epoch: u64,
    pub config_revision: String,
    pub requested_requests: u64,
    pub requested_tokens: u64,
    pub requested_concurrency: u32,
    pub state: QuotaReservationState,
    pub revision: u64,
    pub idempotency_key: String,
    pub expires_at_unix_ms: u64,
    pub reservation_digest: String,
}

impl QuotaReservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reservation_id: QuotaReservationId,
        group: QuotaGroupKey,
        window: QuotaWindow,
        owner_run: RunId,
        owner_attempt: AttemptId,
        authority_epoch: u64,
        config_revision: impl Into<String>,
        requested_requests: u64,
        requested_tokens: u64,
        requested_concurrency: u32,
        idempotency_key: impl Into<String>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut reservation = Self {
            schema: QUOTA_RESERVATION_SCHEMA.to_owned(),
            version: QUOTA_RESERVATION_VERSION,
            reservation_id,
            group,
            window,
            owner_run,
            owner_attempt,
            authority_epoch,
            config_revision: config_revision.into(),
            requested_requests,
            requested_tokens,
            requested_concurrency,
            state: QuotaReservationState::Reserved,
            revision: 1,
            idempotency_key: idempotency_key.into(),
            expires_at_unix_ms,
            reservation_digest: String::new(),
        };
        reservation.reservation_digest = reservation.digest();
        reservation.validate()?;
        Ok(reservation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUOTA_RESERVATION_SCHEMA
            || !self.version.is_compatible_with(&QUOTA_RESERVATION_VERSION)
            || self.reservation_id.as_uuid().is_nil()
            || self.owner_run.as_uuid().is_nil()
            || self.owner_attempt.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.config_revision.trim().is_empty()
            || self.config_revision.len() > 256
            || self.config_revision.contains(['\0', '\r', '\n'])
            || self.requested_requests == 0
            || self.requested_tokens == 0
            || self.requested_concurrency == 0
            || self.revision == 0
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.len() > 512
            || self.expires_at_unix_ms <= self.window.start_unix_ms
            || self.reservation_digest != self.digest()
            || !valid_digest(&self.reservation_digest)
        {
            return Err("quota_reservation_invalid".to_owned());
        }
        self.group.validate()?;
        self.window.validate()?;
        Ok(())
    }

    pub fn transition(&mut self, next: QuotaReservationState) -> Result<(), String> {
        self.validate()?;
        self.state.transition(next)?;
        self.state = next;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "quota_reservation_revision_overflow".to_owned())?;
        self.reservation_digest = self.digest();
        self.validate()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "reservation_id": self.reservation_id,
            "group": self.group,
            "window": self.window,
            "owner_run": self.owner_run,
            "owner_attempt": self.owner_attempt,
            "authority_epoch": self.authority_epoch,
            "config_revision": self.config_revision,
            "requested_requests": self.requested_requests,
            "requested_tokens": self.requested_tokens,
            "requested_concurrency": self.requested_concurrency,
            "state": self.state,
            "revision": self.revision,
            "idempotency_key": self.idempotency_key,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
