//! DEP-27 daemon adapter: cryptographic verification of a migration registry's release binding.
//!
//! `MigrationRegistry` carries a `signature_digest` so the key reference travels with the
//! registry, but a digest is a claim, not a proof. This adapter is where the claim is checked:
//! it verifies an Ed25519 signature over the registry's canonical signing bytes, against a
//! trusted key supplied by the operator. It applies no migration, acquires no lock, and takes no
//! backup; it only answers whether this registry is the one that was signed.

use kiana_domain::MigrationRegistry;
use kiana_ports::PortError;
use ring::signature::{UnparsedPublicKey, ED25519};
use std::collections::BTreeMap;

/// Trusted release keys, keyed by release id then key id. The registry names which key it was
/// signed with; nothing is trusted because it appears in the registry.
pub type TrustedReleaseKeys = BTreeMap<String, BTreeMap<String, String>>;

/// Verify that `registry` carries a valid release signature from a trusted key.
///
/// The registry is validated first, so a malformed registry is rejected on its own terms before
/// any key is consulted. The signature covers the registry's canonical signing bytes, which
/// exclude the signature binding itself.
pub fn verify_registry_signature(
    registry: &MigrationRegistry,
    trusted_keys: &TrustedReleaseKeys,
) -> Result<(), PortError> {
    registry
        .validate()
        .map_err(|_| failed("migration_registry_invalid"))?;

    let release_id = registry.release.release_manifest_digest.as_str();
    let key_id = registry.release.signature_digest.as_str();
    let public_key = trusted_keys
        .get(release_id)
        .and_then(|keys| keys.get(key_id))
        .ok_or_else(|| failed("migration_release_key_untrusted"))?;

    let signing_bytes = registry
        .signing_bytes()
        .map_err(|_| failed("migration_registry_signing_bytes_invalid"))?;
    // `signature_digest` is the key reference; the signature itself travels beside the registry
    // and is supplied by the caller that read it from the release.
    let signature = registry
        .release
        .signature_value
        .as_deref()
        .ok_or_else(|| failed("migration_signature_missing"))?;

    UnparsedPublicKey::new(&ED25519, decode_hex(public_key)?)
        .verify(&signing_bytes, &decode_hex(signature)?)
        .map_err(|_| failed("migration_signature_verification_failed"))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, PortError> {
    if value.len() % 2 != 0 {
        return Err(failed("migration_hex_value_invalid"));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| failed("migration_hex_value_invalid"))
        })
        .collect()
}

fn failed(message: &'static str) -> PortError {
    PortError::Failed(message.to_owned())
}
