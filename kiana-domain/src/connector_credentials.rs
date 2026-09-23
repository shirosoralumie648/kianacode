//! Opaque CI-07 credential leases bound to one connector invocation.

use crate::{
    json_digest, ConnectorBindingSnapshot, CredentialLease, RequestId, SecretRef,
    CREDENTIAL_LEASE_DEFAULT_TTL_MS,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONNECTOR_CREDENTIAL_INVOCATION_SCHEMA: &str = "kiana.connector-credential-invocation.v1";
pub const CONNECTOR_CREDENTIAL_PURPOSE: &str = "connector.invoke";
pub const CONNECTOR_CREDENTIAL_MAX_TTL_MS: u64 = CREDENTIAL_LEASE_DEFAULT_TTL_MS;

/// Metadata-only credential authorization for one connector attempt. It contains no secret value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCredentialInvocation {
    pub schema: String,
    pub connector_id: String,
    pub connector_version: String,
    pub binding_id: String,
    pub binding_revision: u64,
    pub binding_digest: String,
    pub account_id: String,
    pub operation: String,
    pub invocation_id: RequestId,
    pub idempotency_key_digest: String,
    pub secret_ref_digest: String,
    pub credential_generation: u64,
    pub effect_target_digest: String,
    pub lease: CredentialLease,
}

/// Redacted evidence suitable for an event or receipt. It never includes the SecretRef key.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCredentialEvidence {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub binding_revision: u64,
    pub binding_digest: String,
    pub account_id: String,
    pub operation: String,
    pub invocation_id: RequestId,
    pub idempotency_key_digest: String,
    pub secret_ref_digest: String,
    pub credential_generation: u64,
    pub effect_target_digest: String,
    pub lease_digest: String,
}

impl ConnectorCredentialInvocation {
    pub fn issue(
        binding: &ConnectorBindingSnapshot,
        operation: &str,
        invocation_id: RequestId,
        idempotency_key: &str,
        secret_ref: &SecretRef,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<Self, String> {
        binding.validate().map_err(str::to_owned)?;
        binding.operation(operation).map_err(str::to_owned)?;
        if invocation_id.as_uuid().is_nil()
            || idempotency_key.trim().is_empty()
            || idempotency_key.len() > 128
            || idempotency_key.chars().any(char::is_control)
        {
            return Err("connector_credential_invocation_invalid".to_owned());
        }
        if ttl_ms == 0 || ttl_ms > CONNECTOR_CREDENTIAL_MAX_TTL_MS {
            return Err("connector_credential_lease_ttl_invalid".to_owned());
        }
        secret_ref.validate()?;
        let audience = connector_credential_audience(
            &binding.definition.connector_id,
            &binding.definition.version,
        );
        if secret_ref.purpose != CONNECTOR_CREDENTIAL_PURPOSE || secret_ref.audience != audience {
            return Err("connector_credential_secret_ref_binding_mismatch".to_owned());
        }
        if binding.binding.credential_ref.as_ref() != Some(secret_ref) {
            return Err("connector_credential_reference_mismatch".to_owned());
        }
        let effect_target_digest = effect_target_digest(binding);
        let lease = CredentialLease::issue(
            secret_ref.clone(),
            binding.binding.account_id.clone(),
            CONNECTOR_CREDENTIAL_PURPOSE,
            audience,
            effect_target_digest.clone(),
            now_unix_ms,
            ttl_ms,
        )?;
        Ok(Self {
            schema: CONNECTOR_CREDENTIAL_INVOCATION_SCHEMA.to_owned(),
            connector_id: binding.definition.connector_id.clone(),
            connector_version: binding.definition.version.clone(),
            binding_id: binding.binding.binding_id.clone(),
            binding_revision: binding.revision,
            binding_digest: binding_digest(binding),
            account_id: binding.binding.account_id.clone(),
            operation: operation.to_owned(),
            invocation_id,
            idempotency_key_digest: json_digest(&json!({"idempotency_key": idempotency_key})),
            secret_ref_digest: secret_ref.reference_digest.clone(),
            credential_generation: secret_ref.generation,
            effect_target_digest,
            lease,
        })
    }

    pub fn consume_for(
        &mut self,
        binding: &ConnectorBindingSnapshot,
        operation: &str,
        invocation_id: RequestId,
        idempotency_key: &str,
        now_unix_ms: u64,
    ) -> Result<ConnectorCredentialEvidence, String> {
        binding.validate().map_err(str::to_owned)?;
        binding.operation(operation).map_err(str::to_owned)?;
        let target_digest = effect_target_digest(binding);
        let expected_idempotency_digest = json_digest(&json!({"idempotency_key": idempotency_key}));
        if self.schema != CONNECTOR_CREDENTIAL_INVOCATION_SCHEMA
            || self.connector_id != binding.definition.connector_id
            || self.connector_version != binding.definition.version
            || self.binding_id != binding.binding.binding_id
            || self.binding_revision != binding.revision
            || self.binding_digest != binding_digest(binding)
            || self.account_id != binding.binding.account_id
            || self.operation != operation
            || self.invocation_id != invocation_id
            || self.idempotency_key_digest != expected_idempotency_digest
            || self.effect_target_digest != target_digest
            || binding.binding.credential_ref.as_ref() != Some(&self.lease.secret_ref)
            || self.secret_ref_digest != self.lease.secret_ref.reference_digest
            || self.credential_generation != self.lease.secret_ref.generation
        {
            return Err("connector_credential_invocation_binding_mismatch".to_owned());
        }
        let audience = connector_credential_audience(
            &binding.definition.connector_id,
            &binding.definition.version,
        );
        if !self.lease.one_shot {
            return Err("connector_credential_one_shot_required".to_owned());
        }
        let lease_ttl_ms = self
            .lease
            .expires_at_unix_ms
            .checked_sub(self.lease.issued_at_unix_ms)
            .ok_or_else(|| "connector_credential_lease_ttl_invalid".to_owned())?;
        if lease_ttl_ms > CONNECTOR_CREDENTIAL_MAX_TTL_MS {
            return Err("connector_credential_lease_ttl_exceeded".to_owned());
        }
        self.lease.validate_for(
            now_unix_ms,
            &binding.binding.account_id,
            CONNECTOR_CREDENTIAL_PURPOSE,
            &audience,
            &target_digest,
        )?;
        self.lease.consume(now_unix_ms)?;
        Ok(ConnectorCredentialEvidence {
            schema: CONNECTOR_CREDENTIAL_INVOCATION_SCHEMA.to_owned(),
            connector_id: self.connector_id.clone(),
            binding_id: self.binding_id.clone(),
            binding_revision: self.binding_revision,
            binding_digest: self.binding_digest.clone(),
            account_id: self.account_id.clone(),
            operation: self.operation.clone(),
            invocation_id: self.invocation_id,
            idempotency_key_digest: self.idempotency_key_digest.clone(),
            secret_ref_digest: self.secret_ref_digest.clone(),
            credential_generation: self.credential_generation,
            effect_target_digest: self.effect_target_digest.clone(),
            lease_digest: self.lease.lease_digest.clone(),
        })
    }
}

/// Current local-fixture target identity; this is not a network endpoint or live-service claim.
pub fn effect_target_digest(binding: &ConnectorBindingSnapshot) -> String {
    json_digest(&json!({
        "connector_id": binding.definition.connector_id,
        "connector_version": binding.definition.version,
        "adapter": binding.definition.adapter,
        "binding_id": binding.binding.binding_id,
        "account_id": binding.binding.account_id,
        "project_root": binding.project_root,
        "fixture_sha256": binding.binding.fixture_sha256,
    }))
}

pub fn binding_digest(binding: &ConnectorBindingSnapshot) -> String {
    json_digest(&json!({"connector_binding_snapshot": binding}))
}

pub fn connector_credential_audience(connector_id: &str, version: &str) -> String {
    format!("connector:{connector_id}:{version}")
}
