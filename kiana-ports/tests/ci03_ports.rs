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
    let sentinel = "ci03-fixture-value-unique-9d23";
    let resolution = credential_resolution(sentinel);
    resolution.validate(1_000).unwrap();
    let encoded = serde_json::to_string(&resolution).unwrap();
    assert!(!encoded.contains(sentinel));
    assert!(!format!("{resolution:?}").contains(sentinel));
    let decoded: CredentialResolution = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, resolution);
}

fn credential_resolution(value: &str) -> CredentialResolution {
    let reference =
        SecretRef::new("env", "OPENAI_API_KEY", "provider.request", "openai", 4).unwrap();
    CredentialResolution {
        secret_ref: reference,
        state: CredentialState::Available,
        expires_at_unix_ms: Some(10_000),
        resolved_digest: Some(kiana_domain::json_digest(&serde_json::json!(value))),
    }
}

#[test]
fn credential_resolution_metadata_is_strict_and_fail_closed() {
    let resolution = credential_resolution("ci03-fixture-value-unique-9d23");
    resolution.validate(1_000).unwrap();
    let encoded = serde_json::to_string(&resolution).unwrap();
    assert!(encoded.contains("\"state\":\"available\""));
    assert!(encoded.contains("\"expires_at_unix_ms\":10000"));
    assert!(encoded.contains("\"resolved_digest\":\"sha256:"));
    let decoded: CredentialResolution = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, resolution);

    let mut forged = serde_json::to_value(&resolution).unwrap();
    forged["raw_secret"] = serde_json::json!("sk-live-secret");
    assert!(serde_json::from_value::<CredentialResolution>(forged).is_err());

    let mut expired = resolution.clone();
    expired.expires_at_unix_ms = Some(1_000);
    assert_eq!(
        expired.validate(1_000).unwrap_err(),
        kiana_ports::PortError::Conflict("credential_resolution_expired".to_owned())
    );

    let mut malformed_digest = resolution;
    malformed_digest.resolved_digest = Some("not-a-digest".to_owned());
    assert_eq!(
        malformed_digest.validate(1_000).unwrap_err(),
        kiana_ports::PortError::Failed("credential_resolution_digest_invalid".to_owned())
    );
    let _ = MissingCredential;
    let _ = UnsupportedIdentity;
    let _ = UnsupportedConfig;
    let _ = UnsupportedRotation;
}
