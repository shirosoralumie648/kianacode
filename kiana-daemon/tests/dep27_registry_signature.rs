//! DEP-27: the release binding is cryptographically verified, not merely referenced.
//!
//! `MigrationRegistry` carries a `signature_digest` naming a key. That is a claim. These tests
//! sign a real registry and prove the adapter accepts a genuine signature and rejects everything
//! else: a wrong key, a tampered registry, a missing signature, an untrusted key id.

use kiana_daemon::{verify_registry_signature, TrustedReleaseKeys};
use kiana_domain::{
    MigrationCompatibilityWindow, MigrationPrecondition, MigrationRegistry,
    MigrationReleaseBinding, MigrationStep,
};
use ring::signature::{Ed25519KeyPair, KeyPair};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const KEY_ID: &str = "release-key-1";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn step(ordinal: u32, from: u32, to: u32) -> MigrationStep {
    MigrationStep {
        step_id: format!("step-{ordinal}"),
        ordinal,
        from_format_version: from,
        to_format_version: to,
        checksum: DIGEST_A.to_owned(),
        owner: "release-owner".to_owned(),
        backup_required: true,
        precondition: MigrationPrecondition {
            source_schema_digest: DIGEST_A.to_owned(),
            expected_source_revision: 7,
            required_owner: "release-owner".to_owned(),
            requires_verified_backup: true,
        },
        compatibility_window: MigrationCompatibilityWindow {
            min_reader_version: from,
            max_reader_version: to,
            expires_at_unix_ms: 1_900_000_000_000,
        },
        upcaster: format!("upcaster-{ordinal}"),
    }
}

/// A registry carrying whatever signature bytes the caller produced.
fn registry_with_signature(signature: Option<String>) -> MigrationRegistry {
    MigrationRegistry::new(
        MigrationReleaseBinding {
            release_manifest_digest: DIGEST_A.to_owned(),
            artifact_digest: DIGEST_B.to_owned(),
            signature_digest: KEY_ID.to_owned(),
            signature_value: signature,
        },
        vec![step(1, 1, 2), step(2, 2, 3)],
    )
    .expect("registry")
}

fn trusted(public_key: &str) -> TrustedReleaseKeys {
    let mut keys = TrustedReleaseKeys::new();
    keys.insert(
        DIGEST_A.to_owned(),
        [(KEY_ID.to_owned(), public_key.to_owned())]
            .into_iter()
            .collect(),
    );
    keys
}

fn failure(error: &kiana_ports::PortError) -> String {
    format!("{error:?}")
}

#[test]
fn a_genuine_signature_over_the_registry_verifies() {
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).expect("seed");
    let public_key = hex(pair.public_key().as_ref());

    // Sign the registry's own canonical bytes, exactly as a release process would.
    let unsigned = registry_with_signature(None);
    let signature = hex(pair
        .sign(&unsigned.signing_bytes().expect("bytes"))
        .as_ref());

    let signed = registry_with_signature(Some(signature));
    verify_registry_signature(&signed, &trusted(&public_key))
        .expect("a real signature must verify");
}

#[test]
fn a_signature_from_a_different_key_is_rejected() {
    let signer = Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).expect("seed");
    let other = Ed25519KeyPair::from_seed_unchecked(&[9_u8; 32]).expect("seed");

    let unsigned = registry_with_signature(None);
    let signature = hex(signer
        .sign(&unsigned.signing_bytes().expect("bytes"))
        .as_ref());
    let signed = registry_with_signature(Some(signature));

    // Trusted key is a different key pair: the signature is well-formed but not ours.
    let error =
        verify_registry_signature(&signed, &trusted(hex(other.public_key().as_ref()).as_str()))
            .expect_err("a foreign key must not verify");
    assert!(
        failure(&error).contains("migration_signature_verification_failed"),
        "unexpected error: {error:?}"
    );
}

#[test]
fn tampering_with_the_registry_after_signing_is_detected() {
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).expect("seed");
    let public_key = hex(pair.public_key().as_ref());

    let unsigned = registry_with_signature(None);
    let signature = hex(pair
        .sign(&unsigned.signing_bytes().expect("bytes"))
        .as_ref());
    let mut signed = registry_with_signature(Some(signature));

    // Change the steps after signing. The signature covers them, so this must not verify.
    signed.steps[0].owner = "attacker".to_owned();
    signed.registry_digest = signed.digest();

    let error = verify_registry_signature(&signed, &trusted(&public_key))
        .expect_err("a tampered registry must not verify");
    assert!(
        failure(&error).contains("migration_owner_precondition_mismatch")
            || failure(&error).contains("migration_signature_verification_failed"),
        "unexpected error: {error:?}"
    );
}

#[test]
fn a_registry_with_no_signature_at_all_is_refused() {
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).expect("seed");
    let public_key = hex(pair.public_key().as_ref());

    let unsigned = registry_with_signature(None);
    let error = verify_registry_signature(&unsigned, &trusted(&public_key))
        .expect_err("an unsigned registry must not verify");
    assert!(
        failure(&error).contains("migration_signature_missing"),
        "unexpected error: {error:?}"
    );
}

#[test]
fn a_key_the_caller_does_not_trust_is_refused_even_with_a_valid_signature() {
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).expect("seed");
    let public_key = hex(pair.public_key().as_ref());

    let unsigned = registry_with_signature(None);
    let signature = hex(pair
        .sign(&unsigned.signing_bytes().expect("bytes"))
        .as_ref());
    let signed = registry_with_signature(Some(signature));

    // The registry names a key the operator has never trusted.
    let error = verify_registry_signature(&signed, &TrustedReleaseKeys::new())
        .expect_err("an untrusted key id must not verify");
    assert!(
        failure(&error).contains("migration_release_key_untrusted"),
        "unexpected error: {error:?}"
    );
}

#[test]
fn a_malformed_signature_is_refused_without_panicking() {
    // A real trusted public key, so a malformed signature is rejected for being malformed
    // rather than for being untrusted.
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).expect("seed");
    let public_key = hex(pair.public_key().as_ref());

    for bad in ["not-hex", "abc", ""] {
        let signed = registry_with_signature(Some(bad.to_owned()));
        let error = verify_registry_signature(&signed, &trusted(&public_key))
            .expect_err("a malformed signature must not verify");
        assert!(
            failure(&error).contains("migration_hex_value_invalid")
                || failure(&error).contains("migration_signature_verification_failed"),
            "signature {bad:?} produced unexpected error: {error:?}"
        );
    }
}
