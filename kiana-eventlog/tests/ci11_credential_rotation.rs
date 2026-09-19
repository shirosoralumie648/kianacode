use kiana_domain::SecretRef;
use kiana_eventlog::MemoryCredentialRotationStore;
use kiana_ports::CredentialRotationPort;

fn secret_ref(generation: u64) -> SecretRef {
    SecretRef::new(
        "ci11-memory",
        "provider/account",
        "model.request",
        "provider.example",
        generation,
    )
    .expect("secret ref")
}

#[tokio::test]
async fn rotation_is_opaque_and_advances_generation() {
    let initial = secret_ref(1);
    let store = MemoryCredentialRotationStore::new([initial.clone()]).expect("store");
    let rotated = store
        .rotate_credential(&initial, 1)
        .await
        .expect("rotation");
    assert_eq!(rotated.generation, 2);
    assert_ne!(rotated.reference_digest, initial.reference_digest);
    assert!(store.rotate_credential(&initial, 1).await.is_err());
}

#[tokio::test]
async fn stale_generation_does_not_mutate_the_reference() {
    let initial = secret_ref(1);
    let store = MemoryCredentialRotationStore::new([initial.clone()]).expect("store");
    assert!(store.rotate_credential(&initial, 2).await.is_err());
    let rotated = store
        .rotate_credential(&initial, 1)
        .await
        .expect("unchanged reference remains usable");
    assert_eq!(rotated.generation, 2);
}

#[tokio::test]
async fn revoke_advances_the_fence_without_returning_secret_material() {
    let initial = secret_ref(4);
    let store = MemoryCredentialRotationStore::new([initial.clone()]).expect("store");
    let revoked = store.revoke_credential(&initial, 4).await.expect("revoke");
    assert_eq!(revoked.generation, 5);
    let encoded = serde_json::to_string(&revoked).expect("opaque reference serialization");
    assert!(!encoded.contains("access_token"));
    assert!(!encoded.contains("refresh_token"));
}
