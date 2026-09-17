#[test]
fn extension_signature_is_verified_before_install() {
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    let domain = include_str!("../../kiana-domain/src/extensions.rs");
    let fixture = include_str!("../../kiana-domain/tests/p4_l6_01_supply_chain.rs");
    let baseline = include_str!("../../docs/roadmap/p4-l6-01-supply-chain-baseline.md");

    for marker in [
        "extension_signature_is_verified_before_install",
        "verify_manifest_signature",
        "UnparsedPublicKey::new(&ED25519",
        "signing_bytes",
        "extension_publisher_key_untrusted",
        "extension_signature_verification_failed",
        "manifest.validate",
        "content_hash",
        "content_hash_mismatch",
        "capability_diff",
        "license",
        "migration_ref",
        "rollback_ref",
        "extension_rollback_snapshot_missing",
        "append_idempotent_expected",
        "ExtensionPackage",
        "install",
        "upgrade",
        "revoke",
        "rollback",
    ] {
        assert!(
            daemon.contains(marker)
                || domain.contains(marker)
                || fixture.contains(marker)
                || baseline.contains(marker),
            "supply-chain marker missing: {marker}"
        );
    }

    let manifest_validation = daemon
        .find("manifest.validate()")
        .expect("manifest validation present");
    let signature_validation = daemon
        .find("verify_manifest_signature(manifest")
        .expect("signature validation present");
    let content_validation = daemon
        .find("extension_content_hash_mismatch")
        .expect("content hash validation present");
    assert!(manifest_validation < signature_validation);
    assert!(signature_validation < content_validation);
    assert!(daemon.contains("append_idempotent_expected"));
    assert!(daemon.contains("previous_package_sha256"));
    assert!(!daemon.contains("extension_signature_verified_without_check"));
    assert!(!daemon.contains("reqwest::Client"));
}
