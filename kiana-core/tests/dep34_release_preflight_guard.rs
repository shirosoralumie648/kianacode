//! DEP-34 source guard for release preflight.

#[test]
fn release_preflight_binds_build_lock_targets_supply_chain_and_migration() {
    let source = include_str!("../../kiana-domain/src/release_preflight.rs");
    let baseline = include_str!("../../docs/roadmap/dep34-release-preflight-baseline.md");
    for marker in [
        "ReleaseTargetFact",
        "ReleasePreflightFacts",
        "ReleasePreflightReport",
        "reproducible_build",
        "source_tree_clean",
        "expected_cargo_lock_digest",
        "observed_cargo_lock_digest",
        "targets",
        "sbom_present",
        "signature_verified",
        "migration_registry_digest",
        "migration_preflight_ready",
        "verified_backup",
        "release_build_digest_mismatch",
        "release_cargo_lock_digest_mismatch",
        "release_target_matrix_mismatch",
        "release_migration_preflight_blocked",
        "publish_allowed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-34 source marker missing: {marker}"
        );
    }
    for marker in [
        "reproducible build",
        "Cargo.lock",
        "target matrix",
        "SBOM",
        "checksum",
        "signature",
        "migration",
        "backup",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-34 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("std::fs"));
}
