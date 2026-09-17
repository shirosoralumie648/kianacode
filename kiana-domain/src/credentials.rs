//! Opaque credential leases. Raw secret material is intentionally absent from this contract.

use crate::{json_digest, RequestId, SecretRef};
use serde::{Deserialize, Serialize};

pub const CREDENTIAL_LEASE_SCHEMA: &str = "kiana.credential-lease.v1";
pub const CREDENTIAL_LEASE_DEFAULT_TTL_MS: u64 = 60_000;

/// Safe status vocabulary for provider probes and user-facing diagnostics.  None of these
/// variants carry the credential value or a transport error body.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialDisplayStatus {
    Configured,
    Missing,
    Expired,
    ReauthRequired,
    ScopeInsufficient,
    Revoked,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialLease {
    pub schema: String,
    pub lease_id: RequestId,
    pub secret_ref: SecretRef,
    pub provider_account: String,
    pub purpose: String,
    pub audience: String,
    pub endpoint_digest: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub one_shot: bool,
    pub consumed: bool,
    pub lease_digest: String,
}

impl CredentialLease {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        secret_ref: SecretRef,
        provider_account: impl Into<String>,
        purpose: impl Into<String>,
        audience: impl Into<String>,
        endpoint_digest: impl Into<String>,
        issued_at_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<Self, String> {
        let expires_at_unix_ms = issued_at_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| "credential_lease_expiry_invalid".to_owned())?;
        let mut lease = Self {
            schema: CREDENTIAL_LEASE_SCHEMA.to_owned(),
            lease_id: RequestId::new(),
            secret_ref,
            provider_account: provider_account.into(),
            purpose: purpose.into(),
            audience: audience.into(),
            endpoint_digest: endpoint_digest.into(),
            issued_at_unix_ms,
            expires_at_unix_ms,
            one_shot: true,
            consumed: false,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.digest();
        lease.validate_at(issued_at_unix_ms)?;
        Ok(lease)
    }

    pub fn validate_at(&self, now_unix_ms: u64) -> Result<(), String> {
        if self.schema != CREDENTIAL_LEASE_SCHEMA
            || self.lease_id.as_uuid().is_nil()
            || self.provider_account.trim().is_empty()
            || self.provider_account.len() > 256
            || self.purpose.trim().is_empty()
            || self.purpose.len() > 256
            || self.audience.trim().is_empty()
            || self.audience.len() > 256
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || now_unix_ms < self.issued_at_unix_ms
            || now_unix_ms >= self.expires_at_unix_ms
            || self.consumed
            || !self.endpoint_digest.starts_with("sha256:")
            || self.endpoint_digest.len() != 71
            || self.lease_digest != self.digest()
        {
            return Err(if now_unix_ms >= self.expires_at_unix_ms {
                "credential_lease_expired"
            } else {
                "credential_lease_invalid"
            }
            .to_owned());
        }
        self.secret_ref.validate()?;
        if self.secret_ref.purpose != self.purpose || self.secret_ref.audience != self.audience {
            return Err("credential_lease_binding_mismatch".to_owned());
        }
        Ok(())
    }

    /// Revalidate the lease against the exact effect binding.  A lease is not transferable
    /// between provider accounts, purposes, audiences or endpoints even when its timestamp and
    /// digest are otherwise valid.
    pub fn validate_for(
        &self,
        now_unix_ms: u64,
        provider_account: &str,
        purpose: &str,
        audience: &str,
        endpoint_digest: &str,
    ) -> Result<(), String> {
        if self.one_shot && self.consumed {
            return Err("credential_lease_replayed".to_owned());
        }
        self.validate_at(now_unix_ms)?;
        if self.provider_account != provider_account {
            return Err("credential_lease_provider_mismatch".to_owned());
        }
        if self.purpose != purpose || self.secret_ref.purpose != purpose {
            return Err("credential_lease_purpose_mismatch".to_owned());
        }
        if self.audience != audience || self.secret_ref.audience != audience {
            return Err("credential_lease_audience_mismatch".to_owned());
        }
        if self.endpoint_digest != endpoint_digest {
            return Err("credential_lease_endpoint_mismatch".to_owned());
        }
        Ok(())
    }

    /// Consume a one-shot lease immediately before effect-time secret injection.
    pub fn consume(&mut self, now_unix_ms: u64) -> Result<(), String> {
        if self.one_shot && self.consumed {
            return Err("credential_lease_replayed".to_owned());
        }
        self.validate_at(now_unix_ms)?;
        self.consumed = true;
        self.lease_digest = self.digest();
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "lease_id": self.lease_id,
            "secret_ref": self.secret_ref,
            "provider_account": self.provider_account,
            "purpose": self.purpose,
            "audience": self.audience,
            "endpoint_digest": self.endpoint_digest,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "one_shot": self.one_shot,
            "consumed": self.consumed,
        }))
    }
}
