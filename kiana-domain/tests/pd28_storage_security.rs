use kiana_domain::{
    validate_secret_free, validate_secret_free_text, SecretRef, StorageCipher,
    StorageEncryptionBinding, StorageFileIdentity, StorageSecurityCapabilities,
};
use serde_json::json;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn secret_ref(key: &str) -> SecretRef {
    SecretRef::new("vault", key, "storage", "owner:pd28", 1).expect("fixture SecretRef")
}

fn binding() -> StorageEncryptionBinding {
    StorageEncryptionBinding::new(
        StorageCipher::Aes256Gcm,
        secret_ref("pd28/journal"),
        "storage",
        DIGEST_A,
        "sessions",
        DIGEST_B,
    )
    .expect("fixture encryption binding")
}

#[test]
fn secret_sentinels_are_rejected_but_opaque_references_are_allowed() {
    assert!(validate_secret_free(&json!({"token": "raw-token"})).is_err());
    assert!(validate_secret_free_text("Authorization: Bearer raw-token", "log").is_err());
    assert!(validate_secret_free(&json!({
        "secret_ref": "vault://owner/pd28/journal",
        "purpose": "storage"
    }))
    .is_ok());
}

#[test]
fn file_identity_rejects_escape_symlink_hardlink_and_broad_permissions() {
    assert!(StorageFileIdentity::new(DIGEST_A, "../outside", false, false, false, false, None)
        .is_err());
    assert!(StorageFileIdentity::new(DIGEST_A, "events.jsonl", true, true, true, false, None)
        .is_err());
    assert!(StorageFileIdentity::new(DIGEST_A, "events.jsonl", true, true, false, true, None)
        .is_err());
    assert!(StorageFileIdentity::new(
        DIGEST_A,
        "events.jsonl",
        true,
        true,
        false,
        false,
        Some(0o640),
    )
    .is_err());
}

#[test]
fn file_identity_digest_binds_the_observed_metadata() {
    let mut identity = StorageFileIdentity::new(
        DIGEST_A,
        "events.jsonl",
        true,
        true,
        false,
        false,
        Some(0o600),
    )
    .expect("fixture file identity");
    identity.relative_path = "other.jsonl".to_owned();
    assert_eq!(
        identity.validate().unwrap_err(),
        "storage_file_identity_digest_mismatch"
    );
}

#[test]
fn encryption_binding_is_scoped_to_owner_namespace_and_purpose() {
    let binding = binding();
    binding
        .validate_for_scope(DIGEST_A, "sessions", "storage")
        .expect("matching scope");
    assert_eq!(
        binding
            .validate_for_scope(DIGEST_B, "sessions", "storage")
            .unwrap_err(),
        "storage_encryption_owner_scope_mismatch"
    );
    assert_eq!(
        binding
            .validate_for_scope(DIGEST_A, "other", "storage")
            .unwrap_err(),
        "storage_encryption_namespace_mismatch"
    );
    assert_eq!(
        binding
            .validate_for_scope(DIGEST_A, "sessions", "backup")
            .unwrap_err(),
        "storage_encryption_key_purpose_mismatch"
    );
}

#[test]
fn encryption_key_material_sentinel_is_rejected() {
    let result = StorageEncryptionBinding::new(
        StorageCipher::XChaCha20Poly1305,
        secret_ref("token=raw-key-material"),
        "storage",
        DIGEST_A,
        "sessions",
        DIGEST_B,
    );
    assert_eq!(
        result.unwrap_err(),
        "storage_encryption_key_secret_sentinel"
    );
}

#[test]
fn platform_capability_limitations_are_explicit() {
    let capabilities = StorageSecurityCapabilities::current();
    capabilities.validate().expect("capability digest");
    if cfg!(unix) {
        assert!(capabilities.symlink_guard);
        assert!(capabilities.hardlink_guard);
        assert!(capabilities.permission_guard);
    } else {
        assert!(!capabilities.symlink_guard);
        assert!(!capabilities.limitations.is_empty());
    }
}
