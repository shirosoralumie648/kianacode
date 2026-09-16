use async_trait::async_trait;
use kiana_domain::{AuthenticatedPrincipalRef, SecretRef};
use kiana_ports::{
    ConfigSnapshotStore, CredentialResolution, CredentialResolver, CredentialRotationPort,
    CredentialState, IdentityResolver,
};

struct MissingCredential;

#[async_trait]
impl CredentialResolver for MissingCredential {
    async fn resolve_credential(
        &self,
        secret_ref: &SecretRef,
        _now_unix_ms: u64,
    ) -> Result<CredentialResolution, kiana_ports::PortError> {
        Ok(CredentialResolution {
            secret_ref: secret_ref.clone(),
            state: CredentialState::Missing,
            expires_at_unix_ms: None,
            resolved_digest: None,
        })
    }
}

struct UnsupportedIdentity;

#[async_trait]
impl IdentityResolver for UnsupportedIdentity {
    async fn resolve_principal(
        &self,
        _authenticated: &AuthenticatedPrincipalRef,
    ) -> Result<kiana_domain::Principal, kiana_ports::PortError> {
        Err(kiana_ports::PortError::Unavailable(
            "identity_fixture_unavailable".to_owned(),
        ))
    }

    async fn resolve_authority(
        &self,
        _principal: &kiana_domain::Principal,
        _project: &kiana_domain::ProjectIdentity,
        _session_owner: &str,
        _requested_role: &str,
        _now_unix_ms: u64,
    ) -> Result<kiana_domain::AuthoritySnapshot, kiana_ports::PortError> {
        Err(kiana_ports::PortError::Unavailable(
            "authority_fixture_unavailable".to_owned(),
        ))
    }
}

struct UnsupportedConfig;

#[async_trait]
impl ConfigSnapshotStore for UnsupportedConfig {
    async fn read_snapshot(
        &self,
        _project: &kiana_domain::ProjectIdentity,
    ) -> Result<kiana_domain::ConfigSnapshot, kiana_ports::PortError> {
        Err(kiana_ports::PortError::Unavailable(
            "config_fixture_unavailable".to_owned(),
        ))
    }

    async fn publish_snapshot(
        &self,
        _snapshot: kiana_domain::ConfigSnapshot,
        _expected_revision: Option<&str>,
    ) -> Result<kiana_domain::ConfigSnapshot, kiana_ports::PortError> {
        Err(kiana_ports::PortError::Unavailable(
            "config_fixture_unavailable".to_owned(),
        ))
    }
}

struct UnsupportedRotation;

#[async_trait]
impl CredentialRotationPort for UnsupportedRotation {
    async fn rotate_credential(
        &self,
        _secret_ref: &SecretRef,
        _observed_generation: u64,
    ) -> Result<SecretRef, kiana_ports::PortError> {
        Err(kiana_ports::PortError::Unavailable(
            "rotation_fixture_unavailable".to_owned(),
        ))
    }

    async fn revoke_credential(
        &self,
        _secret_ref: &SecretRef,
        _observed_generation: u64,
    ) -> Result<SecretRef, kiana_ports::PortError> {
        Err(kiana_ports::PortError::Unavailable(
            "revoke_fixture_unavailable".to_owned(),
        ))
    }
}

#[test]
fn ports_never_return_raw_secret_to_core() {
    let reference =
        SecretRef::new("env", "OPENAI_API_KEY", "provider.request", "openai", 4).unwrap();
    let resolution = CredentialResolution {
        secret_ref: reference.clone(),
        state: CredentialState::Available,
        expires_at_unix_ms: Some(10_000),
        resolved_digest: Some(kiana_domain::json_digest(&serde_json::json!("secret"))),
    };
    resolution.validate(1_000).unwrap();
    let encoded = serde_json::to_string(&resolution.secret_ref).unwrap();
    assert!(!encoded.contains("secret"));
    let _ = MissingCredential;
    let _ = UnsupportedIdentity;
    let _ = UnsupportedConfig;
    let _ = UnsupportedRotation;
}
