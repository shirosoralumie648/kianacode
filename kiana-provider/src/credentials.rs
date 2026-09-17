//! Provider-side SecretStore adapters.
//!
//! The provider connection keeps only a [`SecretRef`] and an adapter handle.  A raw value is
//! materialized into [`SecretMaterial`] for the final HTTP request, after which its `Drop`
//! implementation clears the owned buffer.  Domain, runner, EventLog and provider snapshots
//! never receive that value.

use kiana_domain::{CredentialLease, ModelError, SecretRef, CREDENTIAL_LEASE_DEFAULT_TTL_MS};
use std::sync::Arc;

const MAX_SECRET_VALUE_BYTES: usize = 64 * 1024;
const PROVIDER_CREDENTIAL_PURPOSE: &str = "provider.request";

/// The narrow backend set understood by the provider adapter.  Only `Env` and `Inline` are
/// usable in this local slice; the protected-storage backends return an explicit unsupported
/// error until their OS-specific implementations are admitted.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecretBackend {
    Env,
    Inline,
    Keyring,
    File,
    Os,
}

/// A secret store resolves an opaque reference into an effect-scoped lease.  It must never put
/// the value in an event, snapshot, error or public port result.
pub(crate) trait SecretStore: Send + Sync {
    #[allow(dead_code)]
    fn backend(&self) -> SecretBackend;

    /// Read only the current value digest for pre-admission revision fencing.  The implementation
    /// may touch its protected source, but it never returns the value itself.
    fn current_revision(&self, secret_ref: &SecretRef) -> Result<String, ModelError>;

    fn issue(
        &self,
        secret_ref: &SecretRef,
        provider_account: &str,
        audience: &str,
        endpoint_digest: &str,
        now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError>;
}

/// Raw material is deliberately private to this crate and exists only while constructing the
/// outbound request.  Clearing the `String` is best-effort memory hygiene; the HTTP client may
/// retain an internal header copy, so this is not represented as a durability or HSM claim.
pub(crate) struct SecretMaterial {
    pub(crate) lease: CredentialLease,
    pub(crate) credential_revision: String,
    pub(crate) value: String,
}

impl Drop for SecretMaterial {
    fn drop(&mut self) {
        self.value.clear();
    }
}

fn validate_value(value: &str) -> Result<(), ModelError> {
    if value.trim().is_empty()
        || value.len() > MAX_SECRET_VALUE_BYTES
        || value.contains('\0')
        || value.contains(['\r', '\n'])
    {
        return Err(ModelError::invalid("model_credential_header_invalid"));
    }
    reqwest::header::HeaderValue::from_str(value)
        .map(|_| ())
        .map_err(|_| ModelError::invalid("model_credential_header_invalid"))
}

fn env_value(secret_ref: &SecretRef) -> Result<String, ModelError> {
    if secret_ref.store != "env"
        || secret_ref.key.is_empty()
        || !secret_ref
            .key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(ModelError::invalid("credential_secret_ref_invalid"));
    }
    let value = std::env::var(&secret_ref.key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ModelError::invalid("model_credential_unavailable"))?;
    validate_value(&value)?;
    Ok(value)
}

fn materialize(
    secret_ref: &SecretRef,
    provider_account: &str,
    audience: &str,
    endpoint_digest: &str,
    now_unix_ms: u64,
    value: String,
) -> Result<SecretMaterial, ModelError> {
    secret_ref
        .validate()
        .map_err(|_| ModelError::invalid("credential_secret_ref_invalid"))?;
    validate_value(&value)?;
    let lease = CredentialLease::issue(
        secret_ref.clone(),
        provider_account,
        PROVIDER_CREDENTIAL_PURPOSE,
        audience,
        endpoint_digest,
        now_unix_ms,
        CREDENTIAL_LEASE_DEFAULT_TTL_MS,
    )
    .map_err(|_| ModelError::invalid("credential_lease_invalid"))?;
    Ok(SecretMaterial {
        lease,
        credential_revision: kiana_domain::json_digest(&serde_json::json!(&value)),
        value,
    })
}

/// Environment-backed adapter.  The environment name is an opaque `SecretRef.key`; the value
/// is looked up only when a request is about to be sent.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EnvSecretStore;

impl SecretStore for EnvSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Env
    }

    fn current_revision(&self, secret_ref: &SecretRef) -> Result<String, ModelError> {
        let value = env_value(secret_ref)?;
        Ok(kiana_domain::json_digest(&serde_json::json!(&value)))
    }

    fn issue(
        &self,
        secret_ref: &SecretRef,
        provider_account: &str,
        audience: &str,
        endpoint_digest: &str,
        now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        let value = env_value(secret_ref)?;
        materialize(
            secret_ref,
            provider_account,
            audience,
            endpoint_digest,
            now_unix_ms,
            value,
        )
    }
}

/// Explicit API-key compatibility adapter.  New callers should provide an env/keyring/file/OS
/// reference; this adapter keeps the existing `ProviderConfig.api_key` API working while the
/// value remains private to the provider connection and effect boundary.
#[derive(Clone, Debug)]
pub(crate) struct InlineSecretStore {
    value: Arc<String>,
    revision: String,
}

impl InlineSecretStore {
    pub(crate) fn new(value: String) -> Result<Self, ModelError> {
        validate_value(&value)?;
        Ok(Self {
            revision: kiana_domain::json_digest(&serde_json::json!(&value)),
            value: Arc::new(value),
        })
    }
}

impl SecretStore for InlineSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Inline
    }

    fn current_revision(&self, secret_ref: &SecretRef) -> Result<String, ModelError> {
        if secret_ref.store != "inline" {
            return Err(ModelError::invalid("credential_secret_ref_invalid"));
        }
        Ok(self.revision.clone())
    }

    fn issue(
        &self,
        secret_ref: &SecretRef,
        provider_account: &str,
        audience: &str,
        endpoint_digest: &str,
        now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        if secret_ref.store != "inline" {
            return Err(ModelError::invalid("credential_secret_ref_invalid"));
        }
        materialize(
            secret_ref,
            provider_account,
            audience,
            endpoint_digest,
            now_unix_ms,
            self.value.as_str().to_owned(),
        )
    }
}

/// Keyring adapter placeholder.  It is intentionally explicit rather than silently falling
/// back to an environment variable or inline value.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Keyring
    }

    fn current_revision(&self, _secret_ref: &SecretRef) -> Result<String, ModelError> {
        Err(ModelError::invalid(
            "credential_backend_unsupported:keyring",
        ))
    }

    fn issue(
        &self,
        _secret_ref: &SecretRef,
        _provider_account: &str,
        _audience: &str,
        _endpoint_digest: &str,
        _now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        Err(ModelError::invalid(
            "credential_backend_unsupported:keyring",
        ))
    }
}

/// File-backed adapter placeholder.  A future implementation must enforce ownership, mode and
/// symlink checks before it can be enabled.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FileSecretStore;

impl SecretStore for FileSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::File
    }

    fn current_revision(&self, _secret_ref: &SecretRef) -> Result<String, ModelError> {
        Err(ModelError::invalid("credential_backend_unsupported:file"))
    }

    fn issue(
        &self,
        _secret_ref: &SecretRef,
        _provider_account: &str,
        _audience: &str,
        _endpoint_digest: &str,
        _now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        Err(ModelError::invalid("credential_backend_unsupported:file"))
    }
}

/// OS credential adapter placeholder.  This prevents an unreviewed platform lookup from being
/// inferred from a `SecretRef.store` string.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OsSecretStore;

impl SecretStore for OsSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Os
    }

    fn current_revision(&self, _secret_ref: &SecretRef) -> Result<String, ModelError> {
        Err(ModelError::invalid("credential_backend_unsupported:os"))
    }

    fn issue(
        &self,
        _secret_ref: &SecretRef,
        _provider_account: &str,
        _audience: &str,
        _endpoint_digest: &str,
        _now_unix_ms: u64,
    ) -> Result<SecretMaterial, ModelError> {
        Err(ModelError::invalid("credential_backend_unsupported:os"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_store_issues_and_consumes_a_bound_one_shot_lease() {
        let secret_ref = SecretRef::new(
            "inline",
            "fixture",
            PROVIDER_CREDENTIAL_PURPOSE,
            "fake-provider",
            1,
        )
        .expect("fixture ref");
        let store = InlineSecretStore::new("sentinel-ci07".to_owned()).expect("fixture value");
        assert_eq!(store.backend(), SecretBackend::Inline);
        let endpoint_digest =
            kiana_domain::json_digest(&serde_json::json!("https://provider.invalid/v1"));
        let mut material = store
            .issue(
                &secret_ref,
                "fake-account",
                "fake-provider",
                &endpoint_digest,
                1_000,
            )
            .expect("lease");
        material
            .lease
            .validate_for(
                1_001,
                "fake-account",
                PROVIDER_CREDENTIAL_PURPOSE,
                "fake-provider",
                &endpoint_digest,
            )
            .expect("binding");
        material.lease.consume(1_001).expect("consume");
        assert_eq!(
            material.lease.consume(1_002).unwrap_err(),
            "credential_lease_replayed"
        );
        assert_eq!(material.value, "sentinel-ci07");
    }

    #[test]
    fn protected_backend_placeholders_fail_closed() {
        let reference =
            SecretRef::new("keyring", "fixture", "purpose", "audience", 1).expect("fixture ref");
        let endpoint_digest =
            kiana_domain::json_digest(&serde_json::json!("https://provider.invalid/v1"));
        assert_eq!(
            KeyringSecretStore
                .issue(&reference, "account", "audience", &endpoint_digest, 1_000)
                .err()
                .expect("keyring denial")
                .code,
            "credential_backend_unsupported:keyring"
        );
        assert_eq!(
            FileSecretStore
                .issue(&reference, "account", "audience", &endpoint_digest, 1_000)
                .err()
                .expect("file denial")
                .code,
            "credential_backend_unsupported:file"
        );
        assert_eq!(
            OsSecretStore
                .issue(&reference, "account", "audience", &endpoint_digest, 1_000)
                .err()
                .expect("os denial")
                .code,
            "credential_backend_unsupported:os"
        );
    }
}
