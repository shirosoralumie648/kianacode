//! DEP-27 daemon guard: the release signature is verified, not merely referenced.

#[test]
fn migration_registry_verifier_checks_a_signature_instead_of_trusting_a_digest() {
    let source = include_str!("../src/migration_registry_verify.rs");
    for marker in [
        "verify_registry_signature",
        "UnparsedPublicKey",
        "ED25519",
        "signing_bytes",
        "migration_release_key_untrusted",
        "migration_signature_missing",
        "migration_signature_verification_failed",
        "migration_registry_invalid",
    ] {
        assert!(
            source.contains(marker),
            "DEP-27 verifier marker missing: {marker}"
        );
    }
    // Verification must not apply a migration, take a lock, or create a backup.
    for forbidden in [
        "apply_migration",
        "MigrationRunnerPort",
        "create_backup",
        "acquire(",
        "acquire_lock",
        "std::fs",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-27 verifier crossed effect boundary: {forbidden}"
        );
    }
    // The trusted keys come from the caller, never from the registry being verified.
    assert!(
        source.contains("trusted_keys\n        .get(release_id)"),
        "the key must be resolved from the caller's trusted set, not from the registry"
    );
}
