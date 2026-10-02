use async_trait::async_trait;
use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, ConfigSnapshot, Principal, PrincipalId, PrincipalKind,
    ProjectIdentity, SecretRef,
};
use kiana_ports::{
    ConfigSnapshotStore, CredentialResolution, CredentialResolver, CredentialRotationPort,
    CredentialState, IdentityResolver, PortError,
};
use std::sync::Arc;
use tokio::sync::RwLock;

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

#[derive(Clone)]
struct MemoryConfigSnapshot {
    project_id: String,
    snapshot: Arc<RwLock<ConfigSnapshot>>,
}

impl MemoryConfigSnapshot {
    fn new(project: &ProjectIdentity, snapshot: ConfigSnapshot) -> Self {
        Self {
            project_id: project.project_id.to_string(),
            snapshot: Arc::new(RwLock::new(snapshot)),
        }
    }
}

#[async_trait]
impl ConfigSnapshotStore for MemoryConfigSnapshot {
    async fn read_snapshot(&self, project: &ProjectIdentity) -> Result<ConfigSnapshot, PortError> {
        if project.project_id.to_string() != self.project_id {
            return Err(PortError::Unavailable(
                "config_fixture_project_unavailable".to_owned(),
            ));
        }
        Ok(self.snapshot.read().await.clone())
    }

    async fn publish_snapshot(
        &self,
        snapshot: ConfigSnapshot,
        expected_revision: Option<&str>,
    ) -> Result<ConfigSnapshot, PortError> {
        snapshot.validate().map_err(PortError::Failed)?;
        let expected_revision = expected_revision.ok_or_else(|| {
            PortError::Unavailable("config_fixture_initial_publish_not_covered".to_owned())
        })?;
        let mut current = self.snapshot.write().await;
        if current.config_revision != expected_revision {
            return Err(PortError::Conflict(
                "config_snapshot_revision_stale".to_owned(),
            ));
        }
        *current = snapshot.clone();
        Ok(snapshot)
    }
}

#[derive(Clone)]
struct MemoryCredentialRotation {
    current: Arc<RwLock<SecretRef>>,
}

impl MemoryCredentialRotation {
    fn new(secret_ref: SecretRef) -> Self {
        Self {
            current: Arc::new(RwLock::new(secret_ref)),
        }
    }

    async fn current(&self) -> SecretRef {
        self.current.read().await.clone()
    }

    async fn compare_and_swap_generation(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, PortError> {
        let mut current = self.current.write().await;
        if current.generation != observed_generation || &*current != secret_ref {
            return Err(PortError::Conflict(
                "credential_rotation_generation_stale".to_owned(),
            ));
        }

        let next_generation = current.generation.checked_add(1).ok_or_else(|| {
            PortError::Failed("credential_rotation_generation_overflow".to_owned())
        })?;
        let next = SecretRef::new(
            current.store.clone(),
            current.key.clone(),
            current.purpose.clone(),
            current.audience.clone(),
            next_generation,
        )
        .map_err(PortError::Failed)?;
        *current = next.clone();
        Ok(next)
    }
}

#[async_trait]
impl CredentialRotationPort for MemoryCredentialRotation {
    async fn rotate_credential(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, kiana_ports::PortError> {
        self.compare_and_swap_generation(secret_ref, observed_generation)
            .await
    }

    async fn revoke_credential(
        &self,
        secret_ref: &SecretRef,
        observed_generation: u64,
    ) -> Result<SecretRef, kiana_ports::PortError> {
        self.compare_and_swap_generation(secret_ref, observed_generation)
            .await
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
    CredentialResolution {
        secret_ref: credential_secret_ref(4),
        state: CredentialState::Available,
        expires_at_unix_ms: Some(10_000),
        resolved_digest: Some(kiana_domain::json_digest(&serde_json::json!(value))),
    }
}

fn credential_secret_ref(generation: u64) -> SecretRef {
    SecretRef::new(
        "env",
        "OPENAI_API_KEY",
        "provider.request",
        "openai",
        generation,
    )
    .unwrap()
}

fn project_identity() -> ProjectIdentity {
    ProjectIdentity::new(
        "/workspace/kiana",
        "/workspace/kiana",
        None,
        None,
        json_digest(&serde_json::json!("ci03-trust-revision")),
    )
    .unwrap()
}

fn config_snapshot(revision: &str, model: &str) -> ConfigSnapshot {
    ConfigSnapshot::new(
        vec!["project-settings".to_owned()],
        serde_json::json!({"model": model}),
        json_digest(&serde_json::json!(revision)),
        json_digest(&serde_json::json!("ci03-trust-revision")),
    )
    .unwrap()
}

fn principal() -> Principal {
    let principal_id = PrincipalId::new();
    let mut authentication = AuthenticatedPrincipalRef::local();
    authentication.principal_id = principal_id.to_string();
    authentication.principal_digest = authentication.digest();
    Principal::new(principal_id, PrincipalKind::Human, authentication, 1).unwrap()
}

#[tokio::test]
async fn config_snapshot_store_revision_cas_is_deterministic() {
    let project = project_identity();
    let initial = config_snapshot("config-v1", "fixture-model-v1");
    let store = MemoryConfigSnapshot::new(&project, initial.clone());

    let first_read = store.read_snapshot(&project).await.unwrap();
    let second_read = store.read_snapshot(&project).await.unwrap();
    assert_eq!(first_read, initial);
    assert_eq!(second_read, first_read);

    let updated = config_snapshot("config-v2", "fixture-model-v2");
    let published = store
        .publish_snapshot(updated.clone(), Some(initial.config_revision.as_str()))
        .await
        .unwrap();
    assert_eq!(published, updated);
    assert_eq!(store.read_snapshot(&project).await.unwrap(), updated);

    let stale_update = config_snapshot("config-v3", "fixture-model-v3");
    assert_eq!(
        store
            .publish_snapshot(stale_update, Some(initial.config_revision.as_str()))
            .await
            .unwrap_err(),
        PortError::Conflict("config_snapshot_revision_stale".to_owned())
    );
    assert_eq!(store.read_snapshot(&project).await.unwrap(), updated);
}

#[tokio::test]
async fn credential_rotation_port_generation_cas_rejects_stale_without_mutation() {
    let initial = credential_secret_ref(4);
    let store = MemoryCredentialRotation::new(initial.clone());

    assert_eq!(
        store.rotate_credential(&initial, 3).await.unwrap_err(),
        PortError::Conflict("credential_rotation_generation_stale".to_owned())
    );
    assert_eq!(store.current().await, initial);

    let rotated = store.rotate_credential(&initial, 4).await.unwrap();
    assert_eq!(rotated, credential_secret_ref(5));
    assert_eq!(store.current().await, rotated);

    assert_eq!(
        store.revoke_credential(&rotated, 4).await.unwrap_err(),
        PortError::Conflict("credential_rotation_generation_stale".to_owned())
    );
    assert_eq!(store.current().await, rotated);

    let revoked = store.revoke_credential(&rotated, 5).await.unwrap();
    assert_eq!(revoked, credential_secret_ref(6));
    assert_eq!(store.current().await, revoked);
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

    let mut non_hex_digest = credential_resolution("ci03-fixture-value-unique-9d23");
    non_hex_digest.resolved_digest = Some(format!("sha256:{}", "g".repeat(64)));
    assert_eq!(
        non_hex_digest.validate(1_000).unwrap_err(),
        kiana_ports::PortError::Failed("credential_resolution_digest_invalid".to_owned())
    );
}

#[tokio::test]
async fn unavailable_identity_and_config_ports_fail_closed_and_missing_stays_explicit() {
    let identity = UnsupportedIdentity;
    let principal = principal();
    let project = project_identity();

    assert_eq!(
        identity
            .resolve_principal(&principal.authentication)
            .await
            .unwrap_err(),
        PortError::Unavailable("identity_fixture_unavailable".to_owned())
    );
    assert_eq!(
        identity
            .resolve_authority(&principal, &project, "fixture-owner", "builder", 1_000)
            .await
            .unwrap_err(),
        PortError::Unavailable("authority_fixture_unavailable".to_owned())
    );

    let config = UnsupportedConfig;
    assert_eq!(
        config.read_snapshot(&project).await.unwrap_err(),
        PortError::Unavailable("config_fixture_unavailable".to_owned())
    );
    assert_eq!(
        config
            .publish_snapshot(
                config_snapshot("config-v1", "fixture-model-v1"),
                Some("fixture-expected-revision"),
            )
            .await
            .unwrap_err(),
        PortError::Unavailable("config_fixture_unavailable".to_owned())
    );

    let requested = credential_secret_ref(4);
    let missing = MissingCredential
        .resolve_credential(&requested, 1_000)
        .await
        .unwrap();
    assert_eq!(missing.secret_ref, requested);
    assert_eq!(missing.state, CredentialState::Missing);
    assert_eq!(missing.expires_at_unix_ms, None);
    assert_eq!(missing.resolved_digest, None);
    missing.validate(1_000).unwrap();
}
