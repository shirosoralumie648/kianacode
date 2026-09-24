//! INT-18 effect-time connector permit and epoch fencing.
//!
//! Admission snapshots are useful for policy, but they are not authority at the adapter edge.
//! `ConnectorEffectPermit` is the server-owned, short lived object consumed by the Broker.  It
//! binds the immutable invocation command to the binding scope and every revocable epoch.  The
//! adapter is reached only after `validate_for_effect` succeeds against a freshly read fence.

use crate::{json_digest, ConnectorBindingSnapshot, ConnectorInvocationReservation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CONNECTOR_EFFECT_PERMIT_SCHEMA: &str = "kiana.connector-effect-permit.v1";
pub const CONNECTOR_EFFECT_FENCE_SCHEMA: &str = "kiana.connector-effect-fence.v1";

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

/// The current server-owned values read again immediately before adapter dispatch.  A caller may
/// provide a stale copy, but it can never make a stale permit current because the Broker compares
/// every field to its own fence snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorEffectFence {
    pub schema: String,
    pub scope_digest: String,
    pub authority_epoch: u64,
    pub configuration_epoch: u64,
    pub policy_epoch: u64,
    /// Zero means that this binding intentionally has no credential. Credential-backed bindings
    /// must carry a non-zero generation.
    pub credential_epoch: u64,
    pub data_epoch: u64,
    pub fence_digest: String,
}

impl ConnectorEffectFence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scope_digest: impl Into<String>,
        authority_epoch: u64,
        configuration_epoch: u64,
        policy_epoch: u64,
        credential_epoch: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        let mut fence = Self {
            schema: CONNECTOR_EFFECT_FENCE_SCHEMA.to_owned(),
            scope_digest: scope_digest.into(),
            authority_epoch,
            configuration_epoch,
            policy_epoch,
            credential_epoch,
            data_epoch,
            fence_digest: String::new(),
        };
        fence.fence_digest = fence.digest();
        fence.validate()?;
        Ok(fence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_EFFECT_FENCE_SCHEMA
            || self.authority_epoch == 0
            || self.configuration_epoch == 0
            || self.policy_epoch == 0
            || self.data_epoch == 0
            || self.fence_digest != self.digest()
        {
            return Err("connector_effect_fence_invalid".to_owned());
        }
        digest(&self.scope_digest, "connector_effect_scope_digest")?;
        digest(&self.fence_digest, "connector_effect_fence_digest")?;
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "scope_digest": self.scope_digest,
            "authority_epoch": self.authority_epoch,
            "configuration_epoch": self.configuration_epoch,
            "policy_epoch": self.policy_epoch,
            "credential_epoch": self.credential_epoch,
            "data_epoch": self.data_epoch,
        }))
    }

    pub fn matches(&self, current: &Self) -> Result<(), String> {
        self.validate()?;
        current.validate()?;
        if self.scope_digest != current.scope_digest {
            return Err("connector_effect_scope_epoch_mismatch".to_owned());
        }
        if self.authority_epoch != current.authority_epoch {
            return Err("connector_effect_authority_epoch_stale".to_owned());
        }
        if self.configuration_epoch != current.configuration_epoch {
            return Err("connector_effect_configuration_epoch_stale".to_owned());
        }
        if self.policy_epoch != current.policy_epoch {
            return Err("connector_effect_policy_epoch_stale".to_owned());
        }
        if self.credential_epoch != current.credential_epoch {
            return Err("connector_effect_credential_epoch_stale".to_owned());
        }
        if self.data_epoch != current.data_epoch {
            return Err("connector_effect_data_epoch_stale".to_owned());
        }
        Ok(())
    }
}

/// Compute a stable scope identity from the immutable server-owned binding and operation. It
/// intentionally contains no credential bytes and changes whenever the binding/configuration or
/// granted operation scope changes.
pub fn connector_effect_scope_digest(
    binding: &ConnectorBindingSnapshot,
    operation: &str,
) -> Result<String, String> {
    binding.validate().map_err(str::to_owned)?;
    let contract = binding.operation(operation).map_err(str::to_owned)?;
    let read_scopes = binding.binding.read_scopes.clone();
    let write_scopes = binding.binding.write_scopes.clone();
    let data_classes: BTreeSet<_> = contract.data_classes.clone();
    Ok(json_digest(&json!({
        "connector_id": binding.definition.connector_id,
        "connector_version": binding.definition.version,
        "binding_id": binding.binding.binding_id,
        "account_id": binding.binding.account_id,
        "project_root": binding.project_root,
        "binding_revision": binding.revision,
        "operation": operation,
        "required_scope": contract.required_scope,
        "read_scopes": read_scopes,
        "write_scopes": write_scopes,
        "data_classes": data_classes,
        "status": binding.status,
    })))
}

/// Effect-time permit. Every digest and epoch is immutable for one attempt. A permit from an old
/// snapshot is never upgraded after revocation, rotation, policy/config changes or data deletion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorEffectPermit {
    pub schema: String,
    pub reservation_id: String,
    pub attempt: u32,
    pub connector_id: String,
    pub connector_version: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub scope_digest: String,
    pub command_digest: String,
    pub payload_digest: String,
    pub idempotency_key_digest: String,
    pub owner_revision: String,
    pub policy_revision: String,
    pub configuration_revision: String,
    pub authority_epoch: u64,
    pub configuration_epoch: u64,
    pub policy_epoch: u64,
    pub credential_epoch: u64,
    pub data_epoch: u64,
    pub binding_revision: u64,
    pub lease_id: String,
    pub fence_token: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub permit_digest: String,
}

impl ConnectorEffectPermit {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        reservation: &ConnectorInvocationReservation,
        binding: &ConnectorBindingSnapshot,
        fence: &ConnectorEffectFence,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        reservation.validate(None)?;
        if !reservation.state.can_consume() {
            return Err("connector_effect_permit_reservation_not_committed".to_owned());
        }
        fence.validate()?;
        if issued_at_unix_ms == 0 || expires_at_unix_ms <= issued_at_unix_ms {
            return Err("connector_effect_permit_expiry_invalid".to_owned());
        }
        let command = &reservation.command;
        command.validate_for_binding(
            binding,
            &command.owner_revision,
            command.authority_revision,
            &command.policy_revision,
            &command.configuration_revision,
            command.credential_generation,
            command.data_revision,
        )?;
        let scope_digest = connector_effect_scope_digest(binding, &command.operation)?;
        if scope_digest != fence.scope_digest
            || fence.authority_epoch != command.authority_revision
            || fence.credential_epoch != command.credential_generation
            || fence.data_epoch != command.data_revision
        {
            return Err("connector_effect_fence_command_mismatch".to_owned());
        }
        let mut permit = Self {
            schema: CONNECTOR_EFFECT_PERMIT_SCHEMA.to_owned(),
            reservation_id: reservation.reservation_id.clone(),
            attempt: command.attempt,
            connector_id: command.connector_id.clone(),
            connector_version: command.connector_version.clone(),
            binding_id: command.binding_id.clone(),
            account_id: command.account_id.clone(),
            operation: command.operation.clone(),
            scope_digest,
            command_digest: command.command_digest.clone(),
            payload_digest: command.payload_digest.clone(),
            idempotency_key_digest: command.idempotency_key_digest.clone(),
            owner_revision: command.owner_revision.clone(),
            policy_revision: command.policy_revision.clone(),
            configuration_revision: command.configuration_revision.clone(),
            authority_epoch: fence.authority_epoch,
            configuration_epoch: fence.configuration_epoch,
            policy_epoch: fence.policy_epoch,
            credential_epoch: fence.credential_epoch,
            data_epoch: fence.data_epoch,
            binding_revision: command.binding_revision,
            lease_id: reservation.lease.lease_id.clone(),
            fence_token: reservation.lease.fence_token.clone(),
            issued_at_unix_ms,
            expires_at_unix_ms,
            permit_digest: String::new(),
        };
        permit.permit_digest = permit.digest();
        permit.validate(None)?;
        Ok(permit)
    }

    pub fn validate(&self, now_unix_ms: Option<u64>) -> Result<(), String> {
        if self.schema != CONNECTOR_EFFECT_PERMIT_SCHEMA
            || required(
                &self.reservation_id,
                "connector_effect_permit_reservation_id",
                256,
            )
            .is_err()
            || required(
                &self.connector_id,
                "connector_effect_permit_connector_id",
                256,
            )
            .is_err()
            || required(
                &self.connector_version,
                "connector_effect_permit_version",
                128,
            )
            .is_err()
            || required(&self.binding_id, "connector_effect_permit_binding_id", 256).is_err()
            || required(&self.account_id, "connector_effect_permit_account_id", 256).is_err()
            || required(&self.operation, "connector_effect_permit_operation", 256).is_err()
            || required(&self.lease_id, "connector_effect_permit_lease_id", 256).is_err()
            || required(
                &self.fence_token,
                "connector_effect_permit_fence_token",
                256,
            )
            .is_err()
            || self.attempt == 0
            || self.authority_epoch == 0
            || self.configuration_epoch == 0
            || self.policy_epoch == 0
            || self.data_epoch == 0
            || self.binding_revision == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.permit_digest != self.digest()
        {
            return Err("connector_effect_permit_invalid".to_owned());
        }
        for (value, field) in [
            (&self.scope_digest, "connector_effect_permit_scope_digest"),
            (
                &self.command_digest,
                "connector_effect_permit_command_digest",
            ),
            (
                &self.payload_digest,
                "connector_effect_permit_payload_digest",
            ),
            (
                &self.idempotency_key_digest,
                "connector_effect_permit_idempotency_key_digest",
            ),
            (
                &self.owner_revision,
                "connector_effect_permit_owner_revision",
            ),
            (&self.permit_digest, "connector_effect_permit_digest"),
        ] {
            digest(value, field)?;
        }
        required(
            &self.policy_revision,
            "connector_effect_permit_policy_revision",
            256,
        )?;
        required(
            &self.configuration_revision,
            "connector_effect_permit_configuration_revision",
            256,
        )?;
        if now_unix_ms
            .is_some_and(|now| now < self.issued_at_unix_ms || now >= self.expires_at_unix_ms)
        {
            return Err("connector_effect_permit_expired".to_owned());
        }
        Ok(())
    }

    /// Recheck the reservation, binding snapshot and all current epochs before entering Broker or
    /// an adapter. `current` must be read after admission, not copied from the permit.
    pub fn validate_for_effect(
        &self,
        reservation: &ConnectorInvocationReservation,
        binding: &ConnectorBindingSnapshot,
        current: &ConnectorEffectFence,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate(Some(now_unix_ms))?;
        reservation.validate(Some(now_unix_ms))?;
        if reservation.state != crate::ConnectorReservationState::Committed
            || self.reservation_id != reservation.reservation_id
            || self.attempt != reservation.command.attempt
            || self.connector_id != reservation.command.connector_id
            || self.connector_version != reservation.command.connector_version
            || self.binding_id != reservation.command.binding_id
            || self.account_id != reservation.command.account_id
            || self.operation != reservation.command.operation
            || self.command_digest != reservation.command.command_digest
            || self.payload_digest != reservation.command.payload_digest
            || self.idempotency_key_digest != reservation.command.idempotency_key_digest
            || self.owner_revision != reservation.command.owner_revision
            || self.policy_revision != reservation.command.policy_revision
            || self.configuration_revision != reservation.command.configuration_revision
            || self.authority_epoch != reservation.command.authority_revision
            || self.credential_epoch != reservation.command.credential_generation
            || self.data_epoch != reservation.command.data_revision
            || self.binding_revision != reservation.command.binding_revision
            || self.lease_id != reservation.lease.lease_id
            || self.fence_token != reservation.lease.fence_token
            || self.expires_at_unix_ms > reservation.lease.expires_at_unix_ms
        {
            return Err("connector_effect_permit_reservation_mismatch".to_owned());
        }
        binding.validate().map_err(str::to_owned)?;
        if binding.status != "active" {
            return Err("connector_effect_binding_revoked".to_owned());
        }
        if binding.revision != self.binding_revision {
            return Err("connector_effect_binding_revision_stale".to_owned());
        }
        if self.connector_id != binding.definition.connector_id
            || self.connector_version != binding.definition.version
            || self.binding_id != binding.binding.binding_id
            || self.account_id != binding.binding.account_id
        {
            return Err("connector_effect_binding_snapshot_stale".to_owned());
        }
        let scope_digest = connector_effect_scope_digest(binding, &self.operation)?;
        if scope_digest != self.scope_digest {
            return Err("connector_effect_scope_digest_mismatch".to_owned());
        }
        let expected = ConnectorEffectFence::new(
            current.scope_digest.clone(),
            current.authority_epoch,
            current.configuration_epoch,
            current.policy_epoch,
            current.credential_epoch,
            current.data_epoch,
        )?;
        let permit_fence = ConnectorEffectFence::new(
            self.scope_digest.clone(),
            self.authority_epoch,
            self.configuration_epoch,
            self.policy_epoch,
            self.credential_epoch,
            self.data_epoch,
        )?;
        permit_fence.matches(&expected)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "reservation_id": self.reservation_id,
            "attempt": self.attempt,
            "connector_id": self.connector_id,
            "connector_version": self.connector_version,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "scope_digest": self.scope_digest,
            "command_digest": self.command_digest,
            "payload_digest": self.payload_digest,
            "idempotency_key_digest": self.idempotency_key_digest,
            "owner_revision": self.owner_revision,
            "policy_revision": self.policy_revision,
            "configuration_revision": self.configuration_revision,
            "authority_epoch": self.authority_epoch,
            "configuration_epoch": self.configuration_epoch,
            "policy_epoch": self.policy_epoch,
            "credential_epoch": self.credential_epoch,
            "data_epoch": self.data_epoch,
            "binding_revision": self.binding_revision,
            "lease_id": self.lease_id,
            "fence_token": self.fence_token,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}
