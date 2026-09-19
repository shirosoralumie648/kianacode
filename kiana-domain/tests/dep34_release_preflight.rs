use kiana_domain::{
    ReleasePreflightFacts, ReleasePreflightReport, ReleasePreflightStatus, ReleaseTargetFact,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn target(name: &str) -> ReleaseTargetFact {
    ReleaseTargetFact {
        target: name.to_owned(),
        artifact_digest: DIGEST_C.to_owned(),
        checksum_digest: DIGEST_D.to_owned(),
        signature_digest: DIGEST_A.to_owned(),
        cargo_lock_digest: DIGEST_A.to_owned(),
        reproducible: true,
        sbom_present: true,
        signature_verified: true,
    }
}

fn facts() -> ReleasePreflightFacts {
    ReleasePreflightFacts {
        release_manifest_digest: DIGEST_C.to_owned(),
        expected_cargo_lock_digest: DIGEST_A.to_owned(),
        observed_cargo_lock_digest: DIGEST_A.to_owned(),
        build_digest: DIGEST_B.to_owned(),
        expected_build_digest: DIGEST_B.to_owned(),
        reproducible_build: true,
        source_tree_clean: true,
        sbom_present: true,
        signature_verified: true,
        migration_registry_digest: DIGEST_D.to_owned(),
        migration_preflight_ready: true,
        verified_backup: true,
        targets: vec![
            target("x86_64-unknown-linux-gnu"),
            target("aarch64-unknown-linux-gnu"),
        ],
    }
}

#[test]
fn release_preflight_allows_only_complete_target_matrix() {
    let report = ReleasePreflightReport::evaluate(&facts()).unwrap();
    report.validate().unwrap();
    assert_eq!(report.status, ReleasePreflightStatus::Ready);
    assert!(report.publish_allowed);
    assert_eq!(report.target_count, 2);
}

#[test]
fn release_preflight_blocks_build_lock_sbom_signature_migration_backup_and_target_drift() {
    let mut dirty = facts();
    dirty.source_tree_clean = false;
    assert_eq!(
        ReleasePreflightReport::evaluate(&dirty).unwrap().reason,
        "release_source_tree_dirty"
    );
    let mut build = facts();
    build.build_digest = DIGEST_A.to_owned();
    assert_eq!(
        ReleasePreflightReport::evaluate(&build).unwrap().reason,
        "release_build_digest_mismatch"
    );
    let mut lock = facts();
    lock.observed_cargo_lock_digest = DIGEST_B.to_owned();
    assert_eq!(
        ReleasePreflightReport::evaluate(&lock).unwrap().reason,
        "release_cargo_lock_digest_mismatch"
    );
    let mut sbom = facts();
    sbom.sbom_present = false;
    assert_eq!(
        ReleasePreflightReport::evaluate(&sbom).unwrap().reason,
        "release_sbom_missing"
    );
    let mut signature = facts();
    signature.signature_verified = false;
    assert_eq!(
        ReleasePreflightReport::evaluate(&signature).unwrap().reason,
        "release_signature_unverified"
    );
    let mut migration = facts();
    migration.migration_preflight_ready = false;
    assert_eq!(
        ReleasePreflightReport::evaluate(&migration).unwrap().reason,
        "release_migration_preflight_blocked"
    );
    let mut backup = facts();
    backup.verified_backup = false;
    assert_eq!(
        ReleasePreflightReport::evaluate(&backup).unwrap().reason,
        "release_backup_unverified"
    );
    let mut target_matrix = facts();
    target_matrix.targets[0].cargo_lock_digest = DIGEST_B.to_owned();
    assert_eq!(
        ReleasePreflightReport::evaluate(&target_matrix)
            .unwrap()
            .reason,
        "release_target_matrix_mismatch"
    );
}

#[test]
fn release_preflight_report_is_strictly_serialized() {
    let report = ReleasePreflightReport::evaluate(&facts()).unwrap();
    let mut value = serde_json::to_value(report).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ReleasePreflightReport>(value).is_err());
}
