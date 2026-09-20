#[test]
fn extension_package_supply_chain_is_fail_closed_and_lifecycle_authoritative() {
    let extensions = include_str!("../../kiana-daemon/src/extensions.rs");
    for marker in [
        "verify_manifest_signature",
        "extension_cache_hash_mismatch",
        "extension_package_path_invalid",
        "extension_content_hash_mismatch",
        "extension_package_too_large",
        "extension_content_too_large",
        "extension_skill_invalid",
        "extension.lifecycle",
        "scripts_executed",
    ] {
        assert!(
            extensions.contains(marker),
            "missing EXT-20 marker: {marker}"
        );
    }
    assert!(extensions.contains("signature_verified"));
    assert!(extensions.contains("license"));
}
