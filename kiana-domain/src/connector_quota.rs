//! INT-17 connector/account/project quota admission and settlement contracts.
//!
//! Quota state is server owned and digest bound. The pure ledger below models the durable CAS
//! boundary used by ControlPlane/EventStore; it never authorizes a capability or invokes a
//! connector. Connector, account and project windows are checked together so a caller cannot
//! bypass a narrow dimension by changing an alias, credential generation or owner identity.

use crate::{
    json_digest, AttemptId, ConnectorInvocationCommand, QuotaReservationId, QuotaWindow,
    RateCardId, RunId, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const CONNECTOR_QUOTA_SCHEMA: &str = "kiana.connector-quota.v1";
pub const CONNECTOR_QUOTA_POLICY_SCHEMA: &str = "kiana.connector-quota-policy.v1";
pub const CONNECTOR_QUOTA_RESERVATION_SCHEMA: &str = "kiana.connector-quota-reservation.v1";
pub const CONNECTOR_QUOTA_SETTLEMENT_SCHEMA: &str = "kiana.connector-quota-settlement.v1";
pub const CONNECTOR_QUOTA_CLAIM_SCHEMA: &str = "kiana.connector-quota-claim.v1";
pub const CONNECTOR_QUOTA_EVENT_RESERVED: &str = "connector.quota_reserved";
pub const CONNECTOR_QUOTA_EVENT_CLAIMED: &str = "connector.quota_claimed";
pub const CONNECTOR_QUOTA_EVENT_SETTLED: &str = "connector.quota_settled";
pub const CONNECTOR_QUOTA_EVENT_RELEASED: &str = "connector.quota_released";
pub const CONNECTOR_QUOTA_STREAM: &str = "connector_quota";
pub const CONNECTOR_QUOTA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_CONNECTOR_QUOTA_ID: usize = 256;
pub const MAX_CONNECTOR_QUOTA_ALIAS: usize = 128;
pub const MAX_CONNECTOR_QUOTA_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

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

fn valid_digest(value: &str) -> bool {
    digest(value, "digest").is_ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorQuotaScope {
    Connector,
    Account,
    Project,
}

impl ConnectorQuotaScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connector => "connector",
            Self::Account => "account",
            Self::Project => "project",
        }
    }
}

/// The canonical server-owned identity shared by all quota dimensions. Aliases are normalized
/// once here and are never accepted as a free-form bypass at the Broker boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaKey {
    pub schema: String,
    pub version: SchemaVersion,
    pub connector_id: String,
    pub connector_version: String,
    pub account_id: String,
    pub project_id: String,
    pub alias: String,
    pub credential_generation: u64,
    pub key_digest: String,
}

impl ConnectorQuotaKey {
    pub fn new(
        connector_id: impl Into<String>,
        connector_version: impl Into<String>,
        account_id: impl Into<String>,
        project_id: impl Into<String>,
        alias: impl Into<String>,
        credential_generation: u64,
    ) -> Result<Self, String> {
        let mut key = Self {
            schema: CONNECTOR_QUOTA_SCHEMA.to_owned(),
            version: CONNECTOR_QUOTA_VERSION,
            connector_id: connector_id.into().trim().to_ascii_lowercase(),
            connector_version: connector_version.into().trim().to_owned(),
            account_id: account_id.into().trim().to_ascii_lowercase(),
            project_id: project_id.into().trim().to_owned(),
            alias: alias.into().trim().to_ascii_lowercase(),
            credential_generation,
            key_digest: String::new(),
        };
        key.key_digest = key.digest();
        key.validate()?;
        Ok(key)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_QUOTA_SCHEMA
            || !self.version.is_compatible_with(&CONNECTOR_QUOTA_VERSION)
            || self.credential_generation == 0
            || self.alias != self.alias.trim().to_ascii_lowercase()
            || self.alias.contains(' ')
            || self.alias.len() > MAX_CONNECTOR_QUOTA_ALIAS
            || !valid_digest(&self.key_digest)
            || self.key_digest != self.digest()
        {
            return Err("connector_quota_key_invalid".to_owned());
        }
        for (value, field) in [
            (&self.connector_id, "connector_quota_connector_id"),
            (&self.connector_version, "connector_quota_connector_version"),
            (&self.account_id, "connector_quota_account_id"),
            (&self.project_id, "connector_quota_project_id"),
            (&self.alias, "connector_quota_alias"),
        ] {
            required(value, field, MAX_CONNECTOR_QUOTA_ID)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "connector_id": self.connector_id,
            "connector_version": self.connector_version,
            "account_id": self.account_id,
            "project_id": self.project_id,
            "alias": self.alias,
            "credential_generation": self.credential_generation,
        }))
    }

    pub fn scope_digest(&self, scope: ConnectorQuotaScope) -> String {
        json_digest(&json!({
            "scope": scope,
            "connector_id": self.connector_id,
            "connector_version": self.connector_version,
            "account_id": self.account_id,
            "project_id": self.project_id,
            "alias": self.alias,
            "credential_generation": self.credential_generation,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaDimensions {
    pub requests: u64,
    pub tokens: u64,
    pub budget_micros: u64,
    pub concurrency: u32,
}

impl ConnectorQuotaDimensions {
    pub fn invocation(tokens: u64, budget_micros: u64) -> Self {
        Self {
            requests: 1,
            tokens,
            budget_micros,
            concurrency: 1,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.requests == 0 || self.concurrency == 0 {
            return Err("connector_quota_dimensions_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaLimit {
    pub max_requests: u64,
    pub max_tokens: u64,
    pub max_budget_micros: u64,
    pub max_concurrency: u32,
}

impl ConnectorQuotaLimit {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_requests == 0
            || self.max_tokens == 0
            || self.max_budget_micros == 0
            || self.max_concurrency == 0
        {
            return Err("connector_quota_limit_invalid".to_owned());
        }
        Ok(())
    }

    fn admits(&self, used: &ConnectorQuotaUsage, requested: &ConnectorQuotaDimensions) -> bool {
        requested.requests <= self.max_requests.saturating_sub(used.requests)
            && requested.tokens <= self.max_tokens.saturating_sub(used.tokens)
            && requested.budget_micros <= self.max_budget_micros.saturating_sub(used.budget_micros)
            && requested.concurrency <= self.max_concurrency.saturating_sub(used.concurrency)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaPolicy {
    pub schema: String,
    pub version: SchemaVersion,
    pub key: ConnectorQuotaKey,
    pub window: QuotaWindow,
    pub connector: ConnectorQuotaLimit,
    pub account: ConnectorQuotaLimit,
    pub project: ConnectorQuotaLimit,
    pub authority_epoch: u64,
    pub configuration_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_card_id: Option<RateCardId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_lease_digest: Option<String>,
    pub policy_digest: String,
}

impl ConnectorQuotaPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: ConnectorQuotaKey,
        window: QuotaWindow,
        connector: ConnectorQuotaLimit,
        account: ConnectorQuotaLimit,
        project: ConnectorQuotaLimit,
        authority_epoch: u64,
        configuration_revision: impl Into<String>,
        rate_card_id: Option<RateCardId>,
        capacity_lease_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: CONNECTOR_QUOTA_POLICY_SCHEMA.to_owned(),
            version: CONNECTOR_QUOTA_VERSION,
            key,
            window,
            connector,
            account,
            project,
            authority_epoch,
            configuration_revision: configuration_revision.into(),
            rate_card_id,
            capacity_lease_digest,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_QUOTA_POLICY_SCHEMA
            || !self.version.is_compatible_with(&CONNECTOR_QUOTA_VERSION)
            || self.authority_epoch == 0
            || self.configuration_revision.trim().is_empty()
            || self.configuration_revision.len() > MAX_CONNECTOR_QUOTA_ID
            || self.configuration_revision.contains(['\0', '\r', '\n'])
            || !valid_digest(&self.policy_digest)
            || self.policy_digest != self.digest()
            || self
                .capacity_lease_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
        {
            return Err("connector_quota_policy_invalid".to_owned());
        }
        if self.rate_card_id.is_none() {
            return Err("connector_quota_rate_card_required".to_owned());
        }
        self.key.validate()?;
        self.window.validate()?;
        if self
            .window
            .end_unix_ms
            .saturating_sub(self.window.start_unix_ms)
            > MAX_CONNECTOR_QUOTA_WINDOW_MS
        {
            return Err("connector_quota_window_too_large".to_owned());
        }
        self.connector.validate()?;
        self.account.validate()?;
        self.project.validate()?;
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "key": self.key,
            "window": self.window,
            "connector": self.connector,
            "account": self.account,
            "project": self.project,
            "authority_epoch": self.authority_epoch,
            "configuration_revision": self.configuration_revision,
            "rate_card_id": self.rate_card_id,
            "capacity_lease_digest": self.capacity_lease_digest,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaUsage {
    pub requests: u64,
    pub tokens: u64,
    pub budget_micros: u64,
    pub concurrency: u32,
}

impl ConnectorQuotaUsage {
    fn reserve(&mut self, requested: &ConnectorQuotaDimensions) -> Result<(), String> {
        self.requests = self
            .requests
            .checked_add(requested.requests)
            .ok_or_else(|| "connector_quota_requests_overflow".to_owned())?;
        self.tokens = self
            .tokens
            .checked_add(requested.tokens)
            .ok_or_else(|| "connector_quota_tokens_overflow".to_owned())?;
        self.budget_micros = self
            .budget_micros
            .checked_add(requested.budget_micros)
            .ok_or_else(|| "connector_quota_budget_overflow".to_owned())?;
        self.concurrency = self
            .concurrency
            .checked_add(requested.concurrency)
            .ok_or_else(|| "connector_quota_concurrency_overflow".to_owned())?;
        Ok(())
    }

    fn release_concurrency(&mut self, requested: &ConnectorQuotaDimensions) -> Result<(), String> {
        if self.concurrency < requested.concurrency {
            return Err("connector_quota_concurrency_underflow".to_owned());
        }
        self.concurrency -= requested.concurrency;
        Ok(())
    }

    fn release_all(&mut self, requested: &ConnectorQuotaDimensions) -> Result<(), String> {
        if self.requests < requested.requests
            || self.tokens < requested.tokens
            || self.budget_micros < requested.budget_micros
            || self.concurrency < requested.concurrency
        {
            return Err("connector_quota_usage_underflow".to_owned());
        }
        self.requests -= requested.requests;
        self.tokens -= requested.tokens;
        self.budget_micros -= requested.budget_micros;
        self.concurrency -= requested.concurrency;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorQuotaReservationState {
    Reserved,
    Claimed,
    Settled,
    Released,
    Unknown,
    Expired,
}

impl ConnectorQuotaReservationState {
    pub fn transition(self, next: Self) -> Result<(), String> {
        if self == next {
            return Err("connector_quota_state_duplicate".to_owned());
        }
        let allowed = matches!(
            (self, next),
            (
                Self::Reserved,
                Self::Claimed | Self::Released | Self::Expired
            ) | (
                Self::Claimed,
                Self::Settled | Self::Released | Self::Unknown
            ) | (Self::Unknown, Self::Settled | Self::Released)
        );
        allowed
            .then_some(())
            .ok_or_else(|| "connector_quota_state_transition_invalid".to_owned())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaReservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub reservation_id: QuotaReservationId,
    pub invocation_id: crate::InvocationId,
    pub attempt: u32,
    pub owner_run: RunId,
    pub owner_attempt: AttemptId,
    pub owner_digest: String,
    pub command_digest: String,
    pub key: ConnectorQuotaKey,
    pub policy_digest: String,
    pub authority_epoch: u64,
    pub configuration_revision: String,
    pub credential_generation: u64,
    pub rate_card_id: Option<RateCardId>,
    pub capacity_lease_digest: Option<String>,
    pub requested: ConnectorQuotaDimensions,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub state: ConnectorQuotaReservationState,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    pub reservation_digest: String,
}

impl ConnectorQuotaReservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reservation_id: QuotaReservationId,
        invocation_id: crate::InvocationId,
        attempt: u32,
        owner_run: RunId,
        owner_attempt: AttemptId,
        owner_digest: impl Into<String>,
        command_digest: impl Into<String>,
        policy: &ConnectorQuotaPolicy,
        requested: ConnectorQuotaDimensions,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        policy.validate()?;
        requested.validate()?;
        let mut reservation = Self {
            schema: CONNECTOR_QUOTA_RESERVATION_SCHEMA.to_owned(),
            version: CONNECTOR_QUOTA_VERSION,
            reservation_id,
            invocation_id,
            attempt,
            owner_run,
            owner_attempt,
            owner_digest: owner_digest.into(),
            command_digest: command_digest.into(),
            key: policy.key.clone(),
            policy_digest: policy.policy_digest.clone(),
            authority_epoch: policy.authority_epoch,
            configuration_revision: policy.configuration_revision.clone(),
            credential_generation: policy.key.credential_generation,
            rate_card_id: policy.rate_card_id,
            capacity_lease_digest: policy.capacity_lease_digest.clone(),
            requested,
            issued_at_unix_ms,
            expires_at_unix_ms,
            state: ConnectorQuotaReservationState::Reserved,
            revision: 1,
            claimed_by: None,
            reservation_digest: String::new(),
        };
        reservation.reservation_digest = reservation.digest();
        reservation.validate_against(policy, None)?;
        Ok(reservation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_QUOTA_RESERVATION_SCHEMA
            || !self.version.is_compatible_with(&CONNECTOR_QUOTA_VERSION)
            || self.reservation_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || self.owner_run.as_uuid().is_nil()
            || self.owner_attempt.as_uuid().is_nil()
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.revision == 0
            || self.credential_generation == 0
            || !valid_digest(&self.owner_digest)
            || !valid_digest(&self.command_digest)
            || !valid_digest(&self.policy_digest)
            || self.configuration_revision.trim().is_empty()
            || self.configuration_revision.len() > MAX_CONNECTOR_QUOTA_ID
            || self.configuration_revision.contains(['\0', '\r', '\n'])
            || self.claimed_by.as_deref().is_some_and(|value| {
                value.trim().is_empty()
                    || value.len() > MAX_CONNECTOR_QUOTA_ID
                    || value.contains(['\0', '\r', '\n'])
            })
            || self
                .capacity_lease_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || !valid_digest(&self.reservation_digest)
            || self.reservation_digest != self.digest()
        {
            return Err("connector_quota_reservation_invalid".to_owned());
        }
        self.key.validate()?;
        self.requested.validate()?;
        if self.claimed_by.is_some()
            != matches!(self.state, ConnectorQuotaReservationState::Claimed)
        {
            return Err("connector_quota_claim_owner_invalid".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        policy: &ConnectorQuotaPolicy,
        now_unix_ms: Option<u64>,
    ) -> Result<(), String> {
        self.validate()?;
        policy.validate()?;
        if self.key != policy.key
            || self.policy_digest != policy.policy_digest
            || self.authority_epoch != policy.authority_epoch
            || self.configuration_revision != policy.configuration_revision
            || self.credential_generation != policy.key.credential_generation
            || self.rate_card_id != policy.rate_card_id
            || self.capacity_lease_digest != policy.capacity_lease_digest
        {
            return Err("connector_quota_policy_fence_stale".to_owned());
        }
        if now_unix_ms
            .is_some_and(|now| now < self.issued_at_unix_ms || now >= self.expires_at_unix_ms)
        {
            return Err("connector_quota_reservation_expired".to_owned());
        }
        Ok(())
    }

    pub fn transition(&mut self, next: ConnectorQuotaReservationState) -> Result<(), String> {
        self.validate()?;
        self.state.transition(next)?;
        self.state = next;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "connector_quota_revision_overflow".to_owned())?;
        if next != ConnectorQuotaReservationState::Claimed {
            self.claimed_by = None;
        }
        self.reservation_digest = self.digest();
        self.validate()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "reservation_id": self.reservation_id,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "owner_run": self.owner_run,
            "owner_attempt": self.owner_attempt,
            "owner_digest": self.owner_digest,
            "command_digest": self.command_digest,
            "key": self.key,
            "policy_digest": self.policy_digest,
            "authority_epoch": self.authority_epoch,
            "configuration_revision": self.configuration_revision,
            "credential_generation": self.credential_generation,
            "rate_card_id": self.rate_card_id,
            "capacity_lease_digest": self.capacity_lease_digest,
            "requested": self.requested,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "state": self.state,
            "revision": self.revision,
            "claimed_by": self.claimed_by,
        }))
    }
}

/// A server-owned worker claim. It is separate from the reservation digest so one CAS winner is
/// observable and a second worker cannot reuse the first worker's owner release path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaClaim {
    pub schema: String,
    pub reservation_id: QuotaReservationId,
    pub reservation_digest: String,
    pub worker_id: String,
    pub expected_revision: u64,
    pub claimed_at_unix_ms: u64,
    pub claim_digest: String,
}

impl ConnectorQuotaClaim {
    pub fn new(
        reservation: &ConnectorQuotaReservation,
        worker_id: impl Into<String>,
        claimed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        reservation.validate()?;
        let mut claim = Self {
            schema: CONNECTOR_QUOTA_CLAIM_SCHEMA.to_owned(),
            reservation_id: reservation.reservation_id,
            reservation_digest: reservation.reservation_digest.clone(),
            worker_id: worker_id.into(),
            expected_revision: reservation.revision,
            claimed_at_unix_ms,
            claim_digest: String::new(),
        };
        claim.claim_digest = claim.digest();
        claim.validate()?;
        Ok(claim)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_QUOTA_CLAIM_SCHEMA
            || self.reservation_id.as_uuid().is_nil()
            || !valid_digest(&self.reservation_digest)
            || self.worker_id.trim().is_empty()
            || self.worker_id.len() > MAX_CONNECTOR_QUOTA_ID
            || self.worker_id.contains(['\0', '\r', '\n'])
            || self.expected_revision == 0
            || self.claimed_at_unix_ms == 0
            || !valid_digest(&self.claim_digest)
            || self.claim_digest != self.digest()
        {
            return Err("connector_quota_claim_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "reservation_id": self.reservation_id,
            "reservation_digest": self.reservation_digest,
            "worker_id": self.worker_id,
            "expected_revision": self.expected_revision,
            "claimed_at_unix_ms": self.claimed_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaSettlement {
    pub schema: String,
    pub version: SchemaVersion,
    pub reservation_id: QuotaReservationId,
    pub reservation_digest: String,
    pub command_digest: String,
    pub worker_id: String,
    pub provider_receipt_id: Option<String>,
    pub charged_tokens: Option<u64>,
    pub charged_budget_micros: Option<u64>,
    pub usage_known: bool,
    pub settled_at_unix_ms: u64,
    pub settlement_digest: String,
}

impl ConnectorQuotaSettlement {
    pub fn new(
        reservation: &ConnectorQuotaReservation,
        worker_id: impl Into<String>,
        provider_receipt_id: Option<String>,
        charged_tokens: Option<u64>,
        charged_budget_micros: Option<u64>,
        settled_at_unix_ms: u64,
    ) -> Result<Self, String> {
        reservation.validate()?;
        let mut settlement = Self {
            schema: CONNECTOR_QUOTA_SETTLEMENT_SCHEMA.to_owned(),
            version: CONNECTOR_QUOTA_VERSION,
            reservation_id: reservation.reservation_id,
            reservation_digest: reservation.reservation_digest.clone(),
            command_digest: reservation.command_digest.clone(),
            worker_id: worker_id.into(),
            provider_receipt_id,
            charged_tokens,
            charged_budget_micros,
            usage_known: charged_tokens.is_some() && charged_budget_micros.is_some(),
            settled_at_unix_ms,
            settlement_digest: String::new(),
        };
        settlement.settlement_digest = settlement.digest();
        settlement.validate_against(reservation)?;
        Ok(settlement)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_QUOTA_SETTLEMENT_SCHEMA
            || !self.version.is_compatible_with(&CONNECTOR_QUOTA_VERSION)
            || self.reservation_id.as_uuid().is_nil()
            || !valid_digest(&self.reservation_digest)
            || !valid_digest(&self.command_digest)
            || self.worker_id.trim().is_empty()
            || self.worker_id.len() > MAX_CONNECTOR_QUOTA_ID
            || self.worker_id.contains(['\0', '\r', '\n'])
            || self.provider_receipt_id.as_deref().is_some_and(|value| {
                value.trim().is_empty()
                    || value.len() > MAX_CONNECTOR_QUOTA_ID
                    || value.contains(['\0', '\r', '\n'])
            })
            || self.usage_known
                != (self.charged_tokens.is_some() && self.charged_budget_micros.is_some())
            || self.settled_at_unix_ms == 0
            || !valid_digest(&self.settlement_digest)
            || self.settlement_digest != self.digest()
        {
            return Err("connector_quota_settlement_invalid".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, reservation: &ConnectorQuotaReservation) -> Result<(), String> {
        self.validate()?;
        if self.reservation_id != reservation.reservation_id
            || self.reservation_digest != reservation.reservation_digest
            || self.command_digest != reservation.command_digest
            || self.settled_at_unix_ms < reservation.issued_at_unix_ms
            || self
                .charged_tokens
                .is_some_and(|value| value > reservation.requested.tokens)
            || self
                .charged_budget_micros
                .is_some_and(|value| value > reservation.requested.budget_micros)
        {
            return Err("connector_quota_settlement_reservation_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "reservation_id": self.reservation_id,
            "reservation_digest": self.reservation_digest,
            "command_digest": self.command_digest,
            "worker_id": self.worker_id,
            "provider_receipt_id": self.provider_receipt_id,
            "charged_tokens": self.charged_tokens,
            "charged_budget_micros": self.charged_budget_micros,
            "usage_known": self.usage_known,
            "settled_at_unix_ms": self.settled_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectorQuotaReserveOutcome {
    Reserved(ConnectorQuotaReservation),
    Replayed(ConnectorQuotaReservation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorQuotaClaimOutcome {
    Claimed,
    Replayed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorQuotaSettlementOutcome {
    Settled,
    Replayed,
}

/// Reference ledger for a durable quota adapter. It explicitly models the single-winner claim,
/// owner-bound release and replayable settlement while remaining inert with respect to Broker.
#[derive(Clone, Debug)]
pub struct ConnectorQuotaLedger {
    server_epoch: u64,
    policy: ConnectorQuotaPolicy,
    usage: BTreeMap<ConnectorQuotaScope, ConnectorQuotaUsage>,
    reservations: BTreeMap<QuotaReservationId, ConnectorQuotaReservation>,
    settlements: BTreeMap<QuotaReservationId, ConnectorQuotaSettlement>,
    idempotency: BTreeMap<String, QuotaReservationId>,
}

impl ConnectorQuotaLedger {
    pub fn new(policy: ConnectorQuotaPolicy, server_epoch: u64) -> Result<Self, String> {
        policy.validate()?;
        if server_epoch == 0 || server_epoch != policy.authority_epoch {
            return Err("connector_quota_epoch_invalid".to_owned());
        }
        Ok(Self {
            server_epoch,
            policy,
            usage: BTreeMap::new(),
            reservations: BTreeMap::new(),
            settlements: BTreeMap::new(),
            idempotency: BTreeMap::new(),
        })
    }

    pub fn policy(&self) -> &ConnectorQuotaPolicy {
        &self.policy
    }

    pub fn usage(&self, scope: ConnectorQuotaScope) -> ConnectorQuotaUsage {
        self.usage.get(&scope).cloned().unwrap_or_default()
    }

    pub fn reserve(
        &mut self,
        reservation: ConnectorQuotaReservation,
        idempotency_key_digest: &str,
        now_unix_ms: u64,
    ) -> Result<ConnectorQuotaReserveOutcome, String> {
        reservation.validate_against(&self.policy, Some(now_unix_ms))?;
        if now_unix_ms < self.policy.window.start_unix_ms
            || now_unix_ms >= self.policy.window.end_unix_ms
        {
            return Err("connector_quota_window_expired".to_owned());
        }
        digest(
            idempotency_key_digest,
            "connector_quota_idempotency_key_digest",
        )?;
        if let Some(existing_id) = self.idempotency.get(idempotency_key_digest) {
            let existing = self
                .reservations
                .get(existing_id)
                .cloned()
                .ok_or_else(|| "connector_quota_reservation_missing".to_owned())?;
            if existing.command_digest == reservation.command_digest {
                return Ok(ConnectorQuotaReserveOutcome::Replayed(existing));
            }
            return Err("connector_quota_idempotency_digest_conflict".to_owned());
        }
        if self.reservations.contains_key(&reservation.reservation_id) {
            return Err("connector_quota_reservation_id_conflict".to_owned());
        }
        self.check_limits(&reservation.requested)?;
        self.reserve_usage(&reservation.requested)?;
        self.idempotency.insert(
            idempotency_key_digest.to_owned(),
            reservation.reservation_id,
        );
        self.reservations
            .insert(reservation.reservation_id, reservation.clone());
        Ok(ConnectorQuotaReserveOutcome::Reserved(reservation))
    }

    fn check_limits(&self, requested: &ConnectorQuotaDimensions) -> Result<(), String> {
        requested.validate()?;
        for (scope, limit) in [
            (ConnectorQuotaScope::Connector, &self.policy.connector),
            (ConnectorQuotaScope::Account, &self.policy.account),
            (ConnectorQuotaScope::Project, &self.policy.project),
        ] {
            if !limit.admits(&self.usage(scope), requested) {
                return Err(format!("connector_quota_{}_limit_exceeded", scope.as_str()));
            }
        }
        Ok(())
    }

    fn reserve_usage(&mut self, requested: &ConnectorQuotaDimensions) -> Result<(), String> {
        for scope in [
            ConnectorQuotaScope::Connector,
            ConnectorQuotaScope::Account,
            ConnectorQuotaScope::Project,
        ] {
            self.usage.entry(scope).or_default().reserve(requested)?;
        }
        Ok(())
    }

    fn release_all_usage(&mut self, requested: &ConnectorQuotaDimensions) -> Result<(), String> {
        for scope in [
            ConnectorQuotaScope::Connector,
            ConnectorQuotaScope::Account,
            ConnectorQuotaScope::Project,
        ] {
            self.usage
                .entry(scope)
                .or_default()
                .release_all(requested)?;
        }
        Ok(())
    }

    fn release_concurrency_usage(
        &mut self,
        requested: &ConnectorQuotaDimensions,
    ) -> Result<(), String> {
        for scope in [
            ConnectorQuotaScope::Connector,
            ConnectorQuotaScope::Account,
            ConnectorQuotaScope::Project,
        ] {
            self.usage
                .entry(scope)
                .or_default()
                .release_concurrency(requested)?;
        }
        Ok(())
    }

    /// CAS claim: exactly one worker can move Reserved to Claimed for a revision.
    pub fn claim(
        &mut self,
        reservation_id: QuotaReservationId,
        expected_revision: u64,
        worker_id: &str,
        now_unix_ms: u64,
    ) -> Result<(ConnectorQuotaClaim, ConnectorQuotaClaimOutcome), String> {
        let mut reservation = self
            .reservations
            .get(&reservation_id)
            .cloned()
            .ok_or_else(|| "connector_quota_reservation_missing".to_owned())?;
        reservation.validate_against(&self.policy, Some(now_unix_ms))?;
        required(
            worker_id,
            "connector_quota_worker_id",
            MAX_CONNECTOR_QUOTA_ID,
        )?;
        if reservation.revision != expected_revision {
            return Err("connector_quota_claim_revision_stale".to_owned());
        }
        if reservation.state == ConnectorQuotaReservationState::Claimed
            && reservation.claimed_by.as_deref() == Some(worker_id)
        {
            let claim = ConnectorQuotaClaim::new(&reservation, worker_id.to_owned(), now_unix_ms)?;
            return Ok((claim, ConnectorQuotaClaimOutcome::Replayed));
        }
        if reservation.state != ConnectorQuotaReservationState::Reserved {
            return Err("connector_quota_claim_already_won".to_owned());
        }
        reservation.state = ConnectorQuotaReservationState::Claimed;
        reservation.claimed_by = Some(worker_id.to_owned());
        reservation.revision = reservation
            .revision
            .checked_add(1)
            .ok_or_else(|| "connector_quota_revision_overflow".to_owned())?;
        reservation.reservation_digest = reservation.digest();
        reservation.validate()?;
        let claim = ConnectorQuotaClaim::new(&reservation, worker_id.to_owned(), now_unix_ms)?;
        self.reservations.insert(reservation_id, reservation);
        Ok((claim, ConnectorQuotaClaimOutcome::Claimed))
    }

    pub fn settle(
        &mut self,
        settlement: ConnectorQuotaSettlement,
    ) -> Result<ConnectorQuotaSettlementOutcome, String> {
        if let Some(existing) = self.settlements.get(&settlement.reservation_id) {
            if existing.settlement_digest == settlement.settlement_digest {
                return Ok(ConnectorQuotaSettlementOutcome::Replayed);
            }
            return Err("connector_quota_settlement_conflict".to_owned());
        }
        let mut reservation = self
            .reservations
            .get(&settlement.reservation_id)
            .cloned()
            .ok_or_else(|| "connector_quota_reservation_missing".to_owned())?;
        settlement.validate_against(&reservation)?;
        if reservation.state != ConnectorQuotaReservationState::Claimed
            || reservation.claimed_by.as_deref() != Some(settlement.worker_id.as_str())
        {
            return Err("connector_quota_settlement_owner_mismatch".to_owned());
        }
        self.release_concurrency_usage(&reservation.requested)?;
        reservation.state = if settlement.usage_known {
            ConnectorQuotaReservationState::Settled
        } else {
            ConnectorQuotaReservationState::Unknown
        };
        reservation.claimed_by = None;
        reservation.revision = reservation.revision.saturating_add(1);
        reservation.reservation_digest = reservation.digest();
        self.reservations
            .insert(settlement.reservation_id, reservation);
        self.settlements
            .insert(settlement.reservation_id, settlement);
        Ok(ConnectorQuotaSettlementOutcome::Settled)
    }

    pub fn release(
        &mut self,
        reservation_id: QuotaReservationId,
        owner_digest: &str,
        worker_id: Option<&str>,
    ) -> Result<(), String> {
        let mut reservation = self
            .reservations
            .get(&reservation_id)
            .cloned()
            .ok_or_else(|| "connector_quota_reservation_missing".to_owned())?;
        reservation.validate_against(&self.policy, None)?;
        if reservation.owner_digest != owner_digest {
            return Err("connector_quota_owner_mismatch".to_owned());
        }
        if let Some(worker) = worker_id {
            if reservation.claimed_by.as_deref() != Some(worker) {
                return Err("connector_quota_release_worker_mismatch".to_owned());
            }
        } else if reservation.state == ConnectorQuotaReservationState::Claimed {
            return Err("connector_quota_release_claim_owner_required".to_owned());
        }
        if !matches!(
            reservation.state,
            ConnectorQuotaReservationState::Reserved
                | ConnectorQuotaReservationState::Claimed
                | ConnectorQuotaReservationState::Unknown
        ) {
            return Err("connector_quota_release_not_pending".to_owned());
        }
        self.release_all_usage(&reservation.requested)?;
        reservation.state = ConnectorQuotaReservationState::Released;
        reservation.claimed_by = None;
        reservation.revision = reservation.revision.saturating_add(1);
        reservation.reservation_digest = reservation.digest();
        self.reservations.insert(reservation_id, reservation);
        Ok(())
    }

    pub fn reservation(
        &self,
        reservation_id: QuotaReservationId,
    ) -> Option<&ConnectorQuotaReservation> {
        self.reservations.get(&reservation_id)
    }

    pub fn settlement(
        &self,
        reservation_id: QuotaReservationId,
    ) -> Option<&ConnectorQuotaSettlement> {
        self.settlements.get(&reservation_id)
    }

    /// Reopen requires the durable store to present the same server epoch. A restarted worker
    /// with an old epoch must stop and obtain a fresh reservation rather than dispatching.
    pub fn validate_reopen(&self, server_epoch: u64) -> Result<(), String> {
        if server_epoch == 0 || server_epoch != self.server_epoch {
            return Err("connector_quota_old_epoch".to_owned());
        }
        self.policy.validate()
    }

    /// A process restart may only reopen a persisted ledger with the same server epoch. Starting
    /// a fresh in-memory counter without a snapshot is a fail-closed error and cannot bypass a
    /// window's existing connector/account/project usage.
    pub fn reopen(server_epoch: u64, persisted: Option<Self>) -> Result<Self, String> {
        let ledger = persisted.ok_or_else(|| "connector_quota_restart_state_missing".to_owned())?;
        ledger.validate_reopen(server_epoch)?;
        Ok(ledger)
    }
}

/// Effect boundary helper used by Broker/provider adapters. Quota claim and INT-16 permit are
/// both required when a server emits the additive quota envelope; neither object mints authority.
pub fn connector_quota_effect_admission(
    reservation: &ConnectorQuotaReservation,
    claim: &ConnectorQuotaClaim,
    policy: &ConnectorQuotaPolicy,
    now_unix_ms: u64,
) -> Result<(), String> {
    reservation.validate_against(policy, Some(now_unix_ms))?;
    claim.validate()?;
    if reservation.state != ConnectorQuotaReservationState::Claimed
        || reservation.claimed_by.as_deref() != Some(claim.worker_id.as_str())
        || reservation.revision != claim.expected_revision
        || claim.reservation_id != reservation.reservation_id
        || claim.reservation_digest != reservation.reservation_digest
    {
        return Err("connector_quota_claim_fence_mismatch".to_owned());
    }
    Ok(())
}

/// Bind quota identity to the immutable INT-16 command. Callers cannot replace connector,
/// account, project, alias or credential generation after the invocation reservation is made.
pub fn connector_quota_command_digest(command: &ConnectorInvocationCommand) -> String {
    command.digest()
}
