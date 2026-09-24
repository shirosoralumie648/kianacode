//! INT-16 connector invocation reservation, command digest and idempotent permit contracts.
//!
//! A connector command is admitted from one server-owned binding snapshot and carries every
//! revision that can change its meaning.  The reservation ledger below is deliberately a pure
//! CAS model: ControlPlane/EventStore own durable commits, Broker owns the consume boundary and
//! the adapter can only run after a committed reservation has consumed exactly one permit.

use crate::{
    json_digest, ConnectorBindingSnapshot, ConnectorOperation, InvocationId, ProviderOutcome,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const CONNECTOR_RESERVATION_SCHEMA: &str = "kiana.connector-invocation-reservation.v1";
pub const CONNECTOR_PERMIT_SCHEMA: &str = "kiana.connector-invocation-permit.v1";
pub const CONNECTOR_INVOCATION_RECEIPT_SCHEMA: &str = "kiana.connector-invocation-receipt.v1";
pub const CONNECTOR_RESERVATION_STREAM: &str = "connector_invocation_reservation";
pub const CONNECTOR_RESERVATION_EVENT_RESERVED: &str = "connector.invocation.reserved";
pub const CONNECTOR_RESERVATION_EVENT_COMMITTED: &str = "connector.invocation.committed";
pub const CONNECTOR_RESERVATION_EVENT_CONSUMED: &str = "connector.invocation.permit_consumed";

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

fn same(left: &str, right: &str, reason: &str) -> Result<(), String> {
    if left == right {
        Ok(())
    } else {
        Err(reason.to_owned())
    }
}

/// The immutable command identity used by both reservation and permit CAS.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorInvocationCommand {
    pub schema: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub connector_id: String,
    pub connector_version: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub binding_revision: u64,
    pub operation_revision: String,
    pub owner_revision: String,
    pub authority_revision: u64,
    pub policy_revision: String,
    pub configuration_revision: String,
    pub credential_generation: u64,
    pub data_revision: u64,
    pub payload_digest: String,
    pub idempotency_key_digest: String,
    pub command_digest: String,
}

impl ConnectorInvocationCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        invocation_id: InvocationId,
        attempt: u32,
        binding: &ConnectorBindingSnapshot,
        operation: &str,
        owner_revision: impl Into<String>,
        authority_revision: u64,
        policy_revision: impl Into<String>,
        configuration_revision: impl Into<String>,
        credential_generation: u64,
        data_revision: u64,
        payload_digest: impl Into<String>,
        idempotency_key: &str,
    ) -> Result<Self, String> {
        let operation_contract = binding.operation(operation).map_err(str::to_owned)?;
        let mut command = Self {
            schema: CONNECTOR_RESERVATION_SCHEMA.to_owned(),
            invocation_id,
            attempt,
            connector_id: binding.definition.connector_id.clone(),
            connector_version: binding.definition.version.clone(),
            binding_id: binding.binding.binding_id.clone(),
            account_id: binding.binding.account_id.clone(),
            operation: operation.to_owned(),
            binding_revision: binding.revision,
            operation_revision: json_digest(
                &serde_json::to_value(operation_contract)
                    .map_err(|_| "connector_operation_revision_encode_failed".to_owned())?,
            ),
            owner_revision: owner_revision.into(),
            authority_revision,
            policy_revision: policy_revision.into(),
            configuration_revision: configuration_revision.into(),
            credential_generation,
            data_revision,
            payload_digest: payload_digest.into(),
            idempotency_key_digest: json_digest(&json!({
                "idempotency_key": idempotency_key
            })),
            command_digest: String::new(),
        };
        command.command_digest = command.digest();
        command.validate()?;
        Ok(command)
    }

    /// Convenience constructor that derives the payload digest from canonical JSON.
    #[allow(clippy::too_many_arguments)]
    pub fn from_payload(
        invocation_id: InvocationId,
        attempt: u32,
        binding: &ConnectorBindingSnapshot,
        operation: &str,
        owner_revision: impl Into<String>,
        authority_revision: u64,
        policy_revision: impl Into<String>,
        configuration_revision: impl Into<String>,
        credential_generation: u64,
        data_revision: u64,
        payload: &serde_json::Value,
        idempotency_key: &str,
    ) -> Result<Self, String> {
        Self::new(
            invocation_id,
            attempt,
            binding,
            operation,
            owner_revision,
            authority_revision,
            policy_revision,
            configuration_revision,
            credential_generation,
            data_revision,
            json_digest(payload),
            idempotency_key,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RESERVATION_SCHEMA
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || self.binding_revision == 0
            || self.authority_revision == 0
            || self.data_revision == 0
            || self.command_digest != self.digest()
        {
            return Err("connector_invocation_command_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.connector_id, "connector_command_connector_id", 256),
            (&self.connector_version, "connector_command_version", 128),
            (&self.binding_id, "connector_command_binding_id", 256),
            (&self.account_id, "connector_command_account_id", 256),
            (&self.operation, "connector_command_operation", 256),
            (
                &self.owner_revision,
                "connector_command_owner_revision",
                256,
            ),
            (
                &self.policy_revision,
                "connector_command_policy_revision",
                256,
            ),
            (
                &self.configuration_revision,
                "connector_command_configuration_revision",
                256,
            ),
        ] {
            required(value, field, max)?;
        }
        for (value, field) in [
            (
                &self.operation_revision,
                "connector_command_operation_revision",
            ),
            (&self.owner_revision, "connector_command_owner_digest"),
            (&self.payload_digest, "connector_command_payload_digest"),
            (
                &self.idempotency_key_digest,
                "connector_command_idempotency_key_digest",
            ),
            (&self.command_digest, "connector_command_digest"),
        ] {
            digest(value, field)?;
        }
        Ok(())
    }

    /// Revalidate all server-owned revisions at the effect boundary.  No caller supplied value
    /// can lower a binding, operation, authority, policy, configuration, credential or data
    /// revision after admission.
    #[allow(clippy::too_many_arguments)]
    pub fn validate_for_binding(
        &self,
        binding: &ConnectorBindingSnapshot,
        owner_revision: &str,
        authority_revision: u64,
        policy_revision: &str,
        configuration_revision: &str,
        credential_generation: u64,
        data_revision: u64,
    ) -> Result<(), String> {
        self.validate()?;
        binding.validate().map_err(str::to_owned)?;
        let operation = binding.operation(&self.operation).map_err(str::to_owned)?;
        same(
            &self.connector_id,
            &binding.definition.connector_id,
            "connector_reservation_connector_mismatch",
        )?;
        same(
            &self.connector_version,
            &binding.definition.version,
            "connector_reservation_configuration_mismatch",
        )?;
        same(
            &self.binding_id,
            &binding.binding.binding_id,
            "connector_reservation_binding_mismatch",
        )?;
        same(
            &self.account_id,
            &binding.binding.account_id,
            "connector_reservation_account_mismatch",
        )?;
        if self.binding_revision != binding.revision {
            return Err("connector_reservation_binding_revision_mismatch".to_owned());
        }
        same(
            &self.operation_revision,
            &json_digest(
                &serde_json::to_value(operation)
                    .map_err(|_| "connector_operation_revision_encode_failed".to_owned())?,
            ),
            "connector_reservation_operation_revision_mismatch",
        )?;
        same(
            &self.owner_revision,
            owner_revision,
            "connector_reservation_owner_revision_mismatch",
        )?;
        if self.authority_revision != authority_revision {
            return Err("connector_reservation_authority_revision_mismatch".to_owned());
        }
        same(
            &self.policy_revision,
            policy_revision,
            "connector_reservation_policy_revision_mismatch",
        )?;
        same(
            &self.configuration_revision,
            configuration_revision,
            "connector_reservation_configuration_revision_mismatch",
        )?;
        if self.credential_generation != credential_generation {
            return Err("connector_reservation_credential_revision_mismatch".to_owned());
        }
        if self.data_revision != data_revision {
            return Err("connector_reservation_data_revision_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "connector_id": self.connector_id,
            "connector_version": self.connector_version,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "binding_revision": self.binding_revision,
            "operation_revision": self.operation_revision,
            "owner_revision": self.owner_revision,
            "authority_revision": self.authority_revision,
            "policy_revision": self.policy_revision,
            "configuration_revision": self.configuration_revision,
            "credential_generation": self.credential_generation,
            "data_revision": self.data_revision,
            "payload_digest": self.payload_digest,
            "idempotency_key_digest": self.idempotency_key_digest,
        }))
    }
}

/// Short-lived server-owned reservation lease and fencing token.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorReservationLease {
    pub lease_id: String,
    pub fence_token: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub lease_digest: String,
}

impl ConnectorReservationLease {
    pub fn new(
        lease_id: impl Into<String>,
        fence_token: impl Into<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut lease = Self {
            lease_id: lease_id.into(),
            fence_token: fence_token.into(),
            issued_at_unix_ms,
            expires_at_unix_ms,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.digest();
        lease.validate(None)?;
        Ok(lease)
    }

    pub fn validate(&self, now_unix_ms: Option<u64>) -> Result<(), String> {
        if required(&self.lease_id, "connector_reservation_lease_id", 256).is_err()
            || required(&self.fence_token, "connector_reservation_fence_token", 256).is_err()
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.lease_digest != self.digest()
        {
            return Err("connector_reservation_lease_invalid".to_owned());
        }
        digest(&self.lease_digest, "connector_reservation_lease_digest")?;
        if now_unix_ms
            .is_some_and(|now| now < self.issued_at_unix_ms || now >= self.expires_at_unix_ms)
        {
            return Err("connector_reservation_lease_expired".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "lease_id": self.lease_id,
            "fence_token": self.fence_token,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorReservationState {
    Reserved,
    Committed,
    PermitConsumed,
    Completed,
    Unknown,
    Released,
}

impl ConnectorReservationState {
    pub const fn can_consume(self) -> bool {
        matches!(self, Self::Committed)
    }
}

/// Reservation facts are immutable except for the CAS state/revision.  The digest deliberately
/// excludes mutable state so replay and state transitions keep one stable command identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorInvocationReservation {
    pub schema: String,
    pub reservation_id: String,
    pub command: ConnectorInvocationCommand,
    pub lease: ConnectorReservationLease,
    pub state: ConnectorReservationState,
    pub state_revision: u64,
    pub reservation_digest: String,
}

impl ConnectorInvocationReservation {
    pub fn new(
        reservation_id: impl Into<String>,
        command: ConnectorInvocationCommand,
        lease: ConnectorReservationLease,
    ) -> Result<Self, String> {
        let mut reservation = Self {
            schema: CONNECTOR_RESERVATION_SCHEMA.to_owned(),
            reservation_id: reservation_id.into(),
            command,
            lease,
            state: ConnectorReservationState::Reserved,
            state_revision: 1,
            reservation_digest: String::new(),
        };
        reservation.reservation_digest = reservation.digest();
        reservation.validate(None)?;
        Ok(reservation)
    }

    pub fn validate(&self, now_unix_ms: Option<u64>) -> Result<(), String> {
        if self.schema != CONNECTOR_RESERVATION_SCHEMA
            || required(&self.reservation_id, "connector_reservation_id", 256).is_err()
            || self.state_revision == 0
            || self.reservation_digest != self.digest()
        {
            return Err("connector_invocation_reservation_invalid".to_owned());
        }
        self.command.validate()?;
        self.lease.validate(now_unix_ms)?;
        digest(&self.reservation_digest, "connector_reservation_digest")?;
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "reservation_id": self.reservation_id,
            "command": self.command,
            "lease": self.lease,
        }))
    }
}

/// A permit is issued only after the reservation CAS reaches Committed.  It carries enough
/// opaque binding material for Broker and the adapter to reject stale or cross-attempt reuse.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorInvocationPermit {
    pub schema: String,
    pub reservation_id: String,
    pub attempt: u32,
    pub command_digest: String,
    pub reservation_digest: String,
    pub lease_id: String,
    pub fence_token: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub permit_digest: String,
}

impl ConnectorInvocationPermit {
    pub fn issue(reservation: &ConnectorInvocationReservation) -> Result<Self, String> {
        reservation.validate(None)?;
        if !reservation.state.can_consume() {
            return Err("connector_reservation_not_committed".to_owned());
        }
        let mut permit = Self {
            schema: CONNECTOR_PERMIT_SCHEMA.to_owned(),
            reservation_id: reservation.reservation_id.clone(),
            attempt: reservation.command.attempt,
            command_digest: reservation.command.command_digest.clone(),
            reservation_digest: reservation.reservation_digest.clone(),
            lease_id: reservation.lease.lease_id.clone(),
            fence_token: reservation.lease.fence_token.clone(),
            issued_at_unix_ms: reservation.lease.issued_at_unix_ms,
            expires_at_unix_ms: reservation.lease.expires_at_unix_ms,
            permit_digest: String::new(),
        };
        permit.permit_digest = permit.digest();
        permit.validate(None)?;
        Ok(permit)
    }

    pub fn validate(&self, now_unix_ms: Option<u64>) -> Result<(), String> {
        if self.schema != CONNECTOR_PERMIT_SCHEMA
            || required(&self.reservation_id, "connector_permit_reservation_id", 256).is_err()
            || self.attempt == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.permit_digest != self.digest()
        {
            return Err("connector_invocation_permit_invalid".to_owned());
        }
        for (value, field) in [
            (&self.command_digest, "connector_permit_command_digest"),
            (
                &self.reservation_digest,
                "connector_permit_reservation_digest",
            ),
            (&self.permit_digest, "connector_permit_digest"),
        ] {
            digest(value, field)?;
        }
        if now_unix_ms
            .is_some_and(|now| now < self.issued_at_unix_ms || now >= self.expires_at_unix_ms)
        {
            return Err("connector_permit_expired".to_owned());
        }
        Ok(())
    }

    pub fn validate_for_reservation(
        &self,
        reservation: &ConnectorInvocationReservation,
        now_unix_ms: Option<u64>,
    ) -> Result<(), String> {
        self.validate(now_unix_ms)?;
        reservation.validate(now_unix_ms)?;
        if reservation.state != ConnectorReservationState::Committed
            || self.reservation_id != reservation.reservation_id
            || self.attempt != reservation.command.attempt
            || self.command_digest != reservation.command.command_digest
            || self.reservation_digest != reservation.reservation_digest
            || self.lease_id != reservation.lease.lease_id
            || self.fence_token != reservation.lease.fence_token
            || self.issued_at_unix_ms != reservation.lease.issued_at_unix_ms
            || self.expires_at_unix_ms != reservation.lease.expires_at_unix_ms
        {
            return Err("connector_permit_reservation_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "reservation_id": self.reservation_id,
            "attempt": self.attempt,
            "command_digest": self.command_digest,
            "reservation_digest": self.reservation_digest,
            "lease_id": self.lease_id,
            "fence_token": self.fence_token,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// Digest-only terminal receipt. Provider payloads remain behind the existing adapter receipt
/// boundary; this object carries only the replay identity and bounded evidence references.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorInvocationReceipt {
    pub schema: String,
    pub reservation_id: String,
    pub attempt: u32,
    pub command_digest: String,
    pub permit_digest: String,
    pub provider_receipt_id: Option<String>,
    pub outcome: ProviderOutcome,
    pub effect_known: bool,
    pub result_digest: String,
    pub receipt_digest: String,
}

impl ConnectorInvocationReceipt {
    pub fn new(
        permit: &ConnectorInvocationPermit,
        provider_receipt_id: Option<String>,
        outcome: ProviderOutcome,
        effect_known: bool,
        result_digest: impl Into<String>,
    ) -> Result<Self, String> {
        permit.validate(None)?;
        if provider_receipt_id
            .as_deref()
            .is_some_and(|value| required(value, "connector_provider_receipt_id", 256).is_err())
        {
            return Err("connector_provider_receipt_id_invalid".to_owned());
        }
        let mut receipt = Self {
            schema: CONNECTOR_INVOCATION_RECEIPT_SCHEMA.to_owned(),
            reservation_id: permit.reservation_id.clone(),
            attempt: permit.attempt,
            command_digest: permit.command_digest.clone(),
            permit_digest: permit.permit_digest.clone(),
            provider_receipt_id,
            outcome,
            effect_known,
            result_digest: result_digest.into(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate_for_permit(permit)?;
        Ok(receipt)
    }

    pub fn validate_for_permit(&self, permit: &ConnectorInvocationPermit) -> Result<(), String> {
        if self.schema != CONNECTOR_INVOCATION_RECEIPT_SCHEMA
            || self.reservation_id != permit.reservation_id
            || self.attempt != permit.attempt
            || self.command_digest != permit.command_digest
            || self.permit_digest != permit.permit_digest
            || self.receipt_digest != self.digest()
        {
            return Err("connector_invocation_receipt_binding_mismatch".to_owned());
        }
        digest(&self.command_digest, "connector_receipt_command_digest")?;
        digest(&self.permit_digest, "connector_receipt_permit_digest")?;
        digest(&self.result_digest, "connector_receipt_result_digest")?;
        digest(&self.receipt_digest, "connector_receipt_digest")?;
        if self.outcome == ProviderOutcome::Succeeded && !self.effect_known {
            return Err("connector_receipt_success_effect_unknown".to_owned());
        }
        if self.outcome == ProviderOutcome::Unknown && self.effect_known {
            return Err("connector_receipt_unknown_effect_known".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "reservation_id": self.reservation_id,
            "attempt": self.attempt,
            "command_digest": self.command_digest,
            "permit_digest": self.permit_digest,
            "provider_receipt_id": self.provider_receipt_id,
            "outcome": self.outcome,
            "effect_known": self.effect_known,
            "result_digest": self.result_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectorReservationOutcome {
    Reserved(ConnectorInvocationReservation),
    Replayed(ConnectorInvocationReservation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorReservationCommitOutcome {
    Committed,
    Replayed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorPermitConsumeOutcome {
    Consumed,
    Replayed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorReceiptApplyOutcome {
    Applied,
    Replayed,
}

/// A process-local reference model for the EventStore CAS transaction.  The durable integration
/// uses the same key/digest/state rules; this ledger is useful for deny-first fixtures and makes
/// replay behavior explicit without creating a second execution loop.
#[derive(Default)]
pub struct ConnectorInvocationLedger {
    reservations: BTreeMap<String, ConnectorInvocationReservation>,
    idempotency: BTreeMap<String, (String, String)>,
    permits: BTreeMap<String, ConnectorInvocationPermit>,
    consumed_attempts: BTreeMap<(String, u32), String>,
    receipts: BTreeMap<String, ConnectorInvocationReceipt>,
}

impl ConnectorInvocationLedger {
    pub fn reserve(
        &mut self,
        command: ConnectorInvocationCommand,
        lease: ConnectorReservationLease,
        reservation_id: impl Into<String>,
    ) -> Result<ConnectorReservationOutcome, String> {
        command.validate()?;
        lease.validate(None)?;
        let reservation_id = reservation_id.into();
        let reservation =
            ConnectorInvocationReservation::new(reservation_id.clone(), command.clone(), lease)?;
        if let Some((existing_digest, existing_id)) =
            self.idempotency.get(&command.idempotency_key_digest)
        {
            if existing_digest != &command.command_digest {
                return Err("connector_idempotency_digest_conflict".to_owned());
            }
            let existing = self
                .reservations
                .get(existing_id)
                .cloned()
                .ok_or_else(|| "connector_reservation_missing".to_owned())?;
            return Ok(ConnectorReservationOutcome::Replayed(existing));
        }
        if self.reservations.contains_key(&reservation_id) {
            return Err("connector_reservation_id_conflict".to_owned());
        }
        self.idempotency.insert(
            command.idempotency_key_digest.clone(),
            (command.command_digest.clone(), reservation_id.clone()),
        );
        self.reservations
            .insert(reservation_id, reservation.clone());
        Ok(ConnectorReservationOutcome::Reserved(reservation))
    }

    pub fn commit(
        &mut self,
        reservation_id: &str,
        expected_state_revision: u64,
    ) -> Result<ConnectorReservationCommitOutcome, String> {
        let reservation = self
            .reservations
            .get_mut(reservation_id)
            .ok_or_else(|| "connector_reservation_missing".to_owned())?;
        if reservation.state_revision != expected_state_revision {
            return Err("connector_reservation_cas_conflict".to_owned());
        }
        match reservation.state {
            ConnectorReservationState::Reserved => {
                reservation.state = ConnectorReservationState::Committed;
                reservation.state_revision = reservation.state_revision.saturating_add(1);
                Ok(ConnectorReservationCommitOutcome::Committed)
            }
            ConnectorReservationState::Committed => Ok(ConnectorReservationCommitOutcome::Replayed),
            _ => Err("connector_reservation_not_committable".to_owned()),
        }
    }

    pub fn issue_permit(
        &mut self,
        reservation_id: &str,
    ) -> Result<ConnectorInvocationPermit, String> {
        let reservation = self
            .reservations
            .get(reservation_id)
            .ok_or_else(|| "connector_reservation_missing".to_owned())?;
        if !reservation.state.can_consume() {
            return Err("connector_reservation_not_committed".to_owned());
        }
        let key = (
            reservation.command.invocation_id.to_string(),
            reservation.command.attempt,
        );
        if self.consumed_attempts.contains_key(&key) || self.permits.contains_key(reservation_id) {
            return Err("connector_attempt_permit_already_issued".to_owned());
        }
        let permit = ConnectorInvocationPermit::issue(reservation)?;
        self.permits
            .insert(reservation_id.to_owned(), permit.clone());
        self.consumed_attempts
            .insert(key, permit.permit_digest.clone());
        Ok(permit)
    }

    pub fn consume_permit(
        &mut self,
        permit: &ConnectorInvocationPermit,
        now_unix_ms: u64,
    ) -> Result<ConnectorPermitConsumeOutcome, String> {
        let reservation = self
            .reservations
            .get_mut(&permit.reservation_id)
            .ok_or_else(|| "connector_reservation_missing".to_owned())?;
        permit.validate_for_reservation(reservation, Some(now_unix_ms))?;
        match reservation.state {
            ConnectorReservationState::Committed => {
                reservation.state = ConnectorReservationState::PermitConsumed;
                reservation.state_revision = reservation.state_revision.saturating_add(1);
                Ok(ConnectorPermitConsumeOutcome::Consumed)
            }
            ConnectorReservationState::PermitConsumed => {
                Err("connector_permit_already_consumed".to_owned())
            }
            _ => Err("connector_permit_not_consumable".to_owned()),
        }
    }

    pub fn apply_receipt(
        &mut self,
        receipt: ConnectorInvocationReceipt,
    ) -> Result<ConnectorReceiptApplyOutcome, String> {
        let permit = self
            .permits
            .get(&receipt.reservation_id)
            .ok_or_else(|| "connector_permit_missing".to_owned())?;
        receipt.validate_for_permit(permit)?;
        let reservation = self
            .reservations
            .get_mut(&receipt.reservation_id)
            .ok_or_else(|| "connector_reservation_missing".to_owned())?;
        if reservation.state != ConnectorReservationState::PermitConsumed
            && reservation.state != ConnectorReservationState::Completed
        {
            return Err("connector_reservation_effect_not_admitted".to_owned());
        }
        if let Some(existing) = self.receipts.get(&receipt.reservation_id) {
            if existing.receipt_digest == receipt.receipt_digest {
                return Ok(ConnectorReceiptApplyOutcome::Replayed);
            }
            return Err("connector_idempotency_receipt_conflict".to_owned());
        }
        reservation.state = ConnectorReservationState::Completed;
        reservation.state_revision = reservation.state_revision.saturating_add(1);
        self.receipts
            .insert(receipt.reservation_id.clone(), receipt);
        Ok(ConnectorReceiptApplyOutcome::Applied)
    }

    /// Returns the original terminal receipt for an identical command. A different command
    /// digest under the same key is always a conflict and must never reach an adapter.
    pub fn replay_receipt(
        &self,
        idempotency_key_digest: &str,
        command_digest: &str,
    ) -> Result<Option<ConnectorInvocationReceipt>, String> {
        let Some((existing_digest, reservation_id)) = self.idempotency.get(idempotency_key_digest)
        else {
            return Ok(None);
        };
        if existing_digest != command_digest {
            return Err("connector_idempotency_digest_conflict".to_owned());
        }
        Ok(self.receipts.get(reservation_id).cloned())
    }

    pub fn reservation(&self, reservation_id: &str) -> Option<&ConnectorInvocationReservation> {
        self.reservations.get(reservation_id)
    }

    pub fn permit(&self, reservation_id: &str) -> Option<&ConnectorInvocationPermit> {
        self.permits.get(reservation_id)
    }
}

/// A small server-side helper used by Core/Broker source guards. It proves that an adapter effect
/// can only follow a committed reservation and a current one-shot permit.
pub fn connector_effect_admission(
    reservation: &ConnectorInvocationReservation,
    permit: &ConnectorInvocationPermit,
    now_unix_ms: u64,
) -> Result<(), String> {
    if reservation.state != ConnectorReservationState::Committed {
        return Err("connector_reservation_not_committed".to_owned());
    }
    permit.validate_for_reservation(reservation, Some(now_unix_ms))
}

/// Public alias for callers that describe the command identity as a digest operation.
pub fn connector_command_digest(command: &ConnectorInvocationCommand) -> String {
    command.digest()
}

/// Server-owned operation revision used by Core when compiling a reservation.
pub fn connector_operation_revision(operation: &ConnectorOperation) -> String {
    json_digest(&serde_json::to_value(operation).unwrap_or_default())
}
