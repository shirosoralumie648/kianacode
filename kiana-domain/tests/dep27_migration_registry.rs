use kiana_domain::{
    MigrationCompatibilityWindow, MigrationPrecondition, MigrationRegistry,
    MigrationReleaseBinding, MigrationStep,
};
use serde_json::json;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn release() -> MigrationReleaseBinding {
    MigrationReleaseBinding {
        release_manifest_digest: DIGEST_A.to_owned(),
        artifact_digest: DIGEST_B.to_owned(),
        signature_digest: DIGEST_C.to_owned(),
    }
}

fn step(ordinal: u32, from: u32, to: u32, checksum: &str) -> MigrationStep {
    MigrationStep {
        step_id: format!("step-{ordinal}"),
        ordinal,
        from_format_version: from,
        to_format_version: to,
        checksum: checksum.to_owned(),
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

#[test]
fn registry_is_ordered_digest_bound_and_release_bound() {
    let registry = MigrationRegistry::new(
        release(),
        vec![step(1, 1, 2, DIGEST_A), step(2, 2, 3, DIGEST_B)],
    )
    .expect("valid registry");
    registry.validate().unwrap();
    registry.validate_for_release(DIGEST_A, DIGEST_B).unwrap();
    assert_eq!(registry.ordered_steps()[0].ordinal, 1);
    assert_eq!(registry.ordered_steps()[1].from_format_version, 2);
}

#[test]
fn registry_rejects_duplicate_checksum_version_gap_and_down_migration() {
    let base = MigrationRegistry::new(
        release(),
        vec![step(1, 1, 2, DIGEST_A), step(2, 2, 3, DIGEST_B)],
    )
    .unwrap();

    let mut duplicate = base.clone();
    duplicate.steps[1].checksum = DIGEST_A.to_owned();
    duplicate.registry_digest = duplicate.digest();
    assert_eq!(
        duplicate.validate().unwrap_err(),
        "migration_registry_order_or_duplicate_invalid"
    );

    let mut gap = base.clone();
    gap.steps[1].from_format_version = 9;
    gap.registry_digest = gap.digest();
    assert_eq!(
        gap.validate().unwrap_err(),
        "migration_registry_version_gap"
    );

    let mut down = base.clone();
    down.steps[0].to_format_version = 0;
    down.registry_digest = down.digest();
    assert_eq!(
        down.validate().unwrap_err(),
        "migration_step_header_invalid"
    );
}

#[test]
fn registry_requires_backup_owner_and_strict_fields() {
    let mut no_backup = step(1, 1, 2, DIGEST_A);
    no_backup.backup_required = false;
    assert_eq!(
        no_backup.validate().unwrap_err(),
        "migration_step_header_invalid"
    );

    let mut unknown = serde_json::to_value(
        MigrationRegistry::new(release(), vec![step(1, 1, 2, DIGEST_A)]).unwrap(),
    )
    .unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<MigrationRegistry>(unknown).is_err());
}
