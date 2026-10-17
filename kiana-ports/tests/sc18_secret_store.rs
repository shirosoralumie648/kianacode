//! SC-18 deny-first fixtures for the `SecretStore` port.
//!
//! The store adapter holds the raw value in private state; every assertion here checks that the
//! port returns only opaque metadata and denies unknown store/provider/purpose, non-transferable
//! references and malformed bindings before any value can cross the boundary.

use async_trait::async_trait;
use kiana_domain::{json_digest, SecretRef};
use kiana_ports::{
    CredentialResolution, CredentialState, PortError, SecretStore, SecretStoreBinding,
    SECRET_STORE_BINDING_SCHEMA,
};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Raw value that must never appear in any port output.
const SENTINEL: &str = "SC18_SECRET_STORE_SENTINEL";

fn reference(store: &str, purpose: &str, audience: &str, generation: u64) -> SecretRef {
    SecretRef::new(store, "SC18_STORE_KEY", purpose, audience, generation).expect("valid reference")
}

fn endpoint_digest() -> String {
    json_digest(&serde_json::json!("https://provider.invalid/v1"))
}

fn binding(provider: &str, purpose: &str, audience: &str) -> SecretStoreBinding {
    SecretStoreBinding::new(provider, purpose, audience, endpoint_digest()).expect("valid binding")
}

/// Store that knows exactly one provider account and purpose, keeps the raw value in private state
/// and returns only opaque metadata.
struct FixtureSecretStore {
    store: String,
    provider_account: String,
    purpose: String,
    values: Mutex<BTreeMap<String, String>>,
}

impl FixtureSecretStore {
    fn new() -> Self {
        let mut values = BTreeMap::new();
        values.insert("SC18_STORE_KEY".to_owned(), SENTINEL.to_owned());
        Self {
            store: "env".to_owned(),
            provider_account: "fake-provider".to_owned(),
            purpose: "provider.request".to_owned(),
            values: Mutex::new(values),
        }
    }
}

#[async_trait]
impl SecretStore for FixtureSecretStore {
    fn store_name(&self) -> &str {
        &self.store
    }

    async fn resolve_secret(
        &self,
        secret_ref: &SecretRef,
        binding: &SecretStoreBinding,
        now_unix_ms: u64,
    ) -> Result<CredentialResolution, PortError> {
        if binding.provider_account != self.provider_account {
            return Err(PortError::Failed(
                "secret_store_provider_unknown".to_owned(),
            ));
        }
        if binding.purpose != self.purpose {
            return Err(PortError::Conflict(
                "secret_store_purpose_denied".to_owned(),
            ));
        }
        let known = self
            .values
            .lock()
            .map_err(|_| PortError::Failed("secret_store_lock_poisoned".to_owned()))?
            .contains_key(&secret_ref.key);
        if !known {
            return Ok(CredentialResolution {
                secret_ref: secret_ref.clone(),
                state: CredentialState::Missing,
                expires_at_unix_ms: None,
                resolved_digest: None,
            });
        }
        Ok(CredentialResolution {
            secret_ref: secret_ref.clone(),
            state: CredentialState::Available,
            expires_at_unix_ms: Some(now_unix_ms + 60_000),
            resolved_digest: Some(json_digest(&serde_json::json!(SENTINEL))),
        })
    }
}

fn code(error: PortError) -> String {
    error.to_string()
}

#[tokio::test]
async fn unknown_store_is_denied_before_any_lookup() {
    let store = FixtureSecretStore::new();
    let unknown_store = reference("vault", "provider.request", "fake-provider", 1);
    let resolved = store
        .resolve_secret_checked(
            &unknown_store,
            &binding("fake-provider", "provider.request", "fake-provider"),
            1_000,
        )
        .await
        .expect_err("unknown store must be denied");
    assert!(code(resolved).contains("secret_store_unknown:vault"));
}

#[tokio::test]
async fn unknown_provider_account_or_purpose_is_denied() {
    let store = FixtureSecretStore::new();
    let reference = reference("env", "provider.request", "fake-provider", 1);

    let unknown_account = store
        .resolve_secret_checked(
            &reference,
            &binding("other-provider", "provider.request", "fake-provider"),
            1_000,
        )
        .await
        .expect_err("unknown provider account must be denied");
    assert!(code(unknown_account).contains("secret_store_provider_unknown"));

    let mut export_reference = reference.clone();
    export_reference.purpose = "export".to_owned();
    export_reference.reference_digest = export_reference.digest();
    let denied = store
        .resolve_secret_checked(
            &export_reference,
            &binding("fake-provider", "export", "fake-provider"),
            1_000,
        )
        .await
        .expect_err("unknown purpose must be denied");
    assert!(code(denied).contains("secret_store_purpose_denied"));
}

#[tokio::test]
async fn reference_is_not_transferable_across_purpose_or_audience() {
    let store = FixtureSecretStore::new();
    let reference = reference("env", "provider.request", "fake-provider", 1);

    let purpose_drift = store
        .resolve_secret_checked(
            &reference,
            &binding("fake-provider", "audit", "fake-provider"),
            1_000,
        )
        .await
        .expect_err("purpose drift must be denied");
    assert!(code(purpose_drift).contains("secret_store_binding_mismatch"));

    let audience_drift = store
        .resolve_secret_checked(
            &reference,
            &binding("fake-provider", "provider.request", "other-audience"),
            1_000,
        )
        .await
        .expect_err("audience drift must be denied");
    assert!(code(audience_drift).contains("secret_store_binding_mismatch"));
}

#[tokio::test]
async fn happy_path_returns_only_opaque_metadata() {
    let store = FixtureSecretStore::new();
    let reference = reference("env", "provider.request", "fake-provider", 1);
    let resolution = store
        .resolve_secret_checked(
            &reference,
            &binding("fake-provider", "provider.request", "fake-provider"),
            1_000,
        )
        .await
        .expect("bound resolution");
    assert_eq!(resolution.state, CredentialState::Available);
    assert_eq!(resolution.secret_ref, reference);
    assert!(resolution.resolved_digest.is_some());

    let encoded = serde_json::to_string(&resolution).expect("resolution json");
    assert!(!encoded.contains(SENTINEL));
    assert!(!encoded.contains("secret_value"));
}

#[tokio::test]
async fn wire_log_event_payload_carries_no_secret_value() {
    let store = FixtureSecretStore::new();
    let reference = reference("env", "provider.request", "fake-provider", 1);
    let binding = binding("fake-provider", "provider.request", "fake-provider");
    let resolution = store
        .resolve_secret_checked(&reference, &binding, 1_000)
        .await
        .expect("bound resolution");

    // A would-be wire/log/event envelope built only from the port DTOs stays secret-free.
    let envelope = serde_json::json!({
        "wire": &resolution,
        "log": &binding,
        "event": &reference,
    });
    let encoded = serde_json::to_string(&envelope).expect("envelope json");
    assert!(!encoded.contains(SENTINEL));
    assert!(!encoded.contains("SC18_SECRET_STORE_SENTINEL"));

    // Injecting a raw value slot is rejected by the closed schemas rather than accepted.
    let mut tampered = serde_json::to_value(&resolution).expect("resolution value");
    tampered["secret_value"] = serde_json::json!(SENTINEL);
    assert!(serde_json::from_value::<CredentialResolution>(tampered).is_err());

    let mut tampered_binding = serde_json::to_value(&binding).expect("binding value");
    tampered_binding["value"] = serde_json::json!(SENTINEL);
    assert!(serde_json::from_value::<SecretStoreBinding>(tampered_binding).is_err());
}

#[test]
fn malformed_binding_is_rejected_before_use() {
    assert_eq!(
        SecretStoreBinding::new("", "provider.request", "fake-provider", endpoint_digest())
            .unwrap_err(),
        "secret_store_binding_provider_invalid"
    );
    assert_eq!(
        SecretStoreBinding::new(
            "fake-provider",
            "provider.request",
            "fake-provider",
            "sha512:deadbeef"
        )
        .unwrap_err(),
        "secret_store_binding_endpoint_digest_invalid"
    );
    assert_eq!(
        SecretStoreBinding::new(
            "fake-provider",
            "provider.request",
            "fake-provider",
            format!("sha256:{}", "g".repeat(64))
        )
        .unwrap_err(),
        "secret_store_binding_endpoint_digest_invalid"
    );

    let binding = binding("fake-provider", "provider.request", "fake-provider");
    assert_eq!(binding.schema, SECRET_STORE_BINDING_SCHEMA);
    let mut value = serde_json::to_value(&binding).expect("binding value");
    value["secret"] = serde_json::json!(SENTINEL);
    assert!(serde_json::from_value::<SecretStoreBinding>(value).is_err());
}
