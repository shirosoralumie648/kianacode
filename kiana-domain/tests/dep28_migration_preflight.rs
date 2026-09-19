use kiana_domain::{
    MigrationAxisResult, MigrationCompatibilityWindow, MigrationDigestFact, MigrationLeaseFact,
    MigrationPrecondition, MigrationPreflightAxis, MigrationPreflightFacts,
    MigrationPreflightReport, MigrationPreflightStatus, MigrationProjectionFact, MigrationRegistry,
    MigrationReleaseBinding, MigrationSchemaFact, MigrationSpaceFact, MigrationStep,
};
use serde_json::json;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn registry() -> MigrationRegistry {
    MigrationRegistry::new(
        MigrationReleaseBinding {
            release_manifest_digest: DIGEST_A.to_owned(),
            artifact_digest: DIGEST_B.to_owned(),
            signature_digest: DIGEST_C.to_owned(),
        },
        vec![MigrationStep {
            step_id: "step-1".to_owned(),
            ordinal: 1,
            from_format_version: 1,
            to_format_version: 2,
            checksum: DIGEST_A.to_owned(),
            owner: "owner".to_owned(),
            backup_required: true,
            precondition: MigrationPrecondition {
                source_schema_digest: DIGEST_A.to_owned(),
                expected_source_revision: 1,
                required_owner: "owner".to_owned(),
                requires_verified_backup: true,
            },
            compatibility_window: MigrationCompatibilityWindow {
                min_reader_version: 1,
                max_reader_version: 2,
                expires_at_unix_ms: 1_900_000_000_000,
            },
            upcaster: "upcaster-1".to_owned(),
        }],
    )
    .unwrap()
}

fn digest_fact() -> MigrationDigestFact {
    MigrationDigestFact {
        observed_digest: DIGEST_A.to_owned(),
        expected_digest: DIGEST_A.to_owned(),
        matches: true,
    }
}

fn facts() -> MigrationPreflightFacts {
    MigrationPreflightFacts {
        store: digest_fact(),
        schema: MigrationSchemaFact {
            current_format_version: 1,
            current_major: 1,
            target_major: 1,
            observed_schema_digest: DIGEST_A.to_owned(),
            expected_schema_digest: DIGEST_A.to_owned(),
        },
        projection: MigrationProjectionFact {
            source_cursor: 10,
            projection_cursor: 10,
            current_generation: 3,
            expected_generation: 3,
        },
        workflow: digest_fact(),
        provider: digest_fact(),
        config: digest_fact(),
        space: MigrationSpaceFact {
            available_bytes: 100,
            required_bytes: 10,
            available_inodes: 100,
            required_inodes: 10,
        },
        clock: kiana_domain::MigrationClockFact {
            trusted: true,
            revision: 2,
        },
        lease: MigrationLeaseFact {
            runner_held: false,
            old_writer_count: 0,
            active_unknown_count: 0,
            owner_available: true,
        },
        verified_backup: true,
    }
}

#[test]
fn preflight_is_read_only_and_reports_all_axes() {
    let report = MigrationPreflightReport::evaluate(&registry(), &facts(), 1_700_000_000_000)
        .expect("valid preflight");
    report.validate().unwrap();
    assert!(report.is_ready());
    assert_eq!(report.axes.len(), 9);
    assert!(report.read_only);
    assert_eq!(report.effect_calls, 0);
    assert_eq!(report.axes[0].axis, MigrationPreflightAxis::Store);
}

#[test]
fn preflight_blocks_mismatch_space_clock_writer_and_unknown_effects() {
    let mut blocked_facts = facts();
    blocked_facts.space.available_bytes = 1;
    blocked_facts.clock.trusted = false;
    blocked_facts.lease.old_writer_count = 1;
    blocked_facts.lease.active_unknown_count = 1;
    let report = MigrationPreflightReport::evaluate(&registry(), &blocked_facts, 1_700_000_000_000)
        .expect("blocked preflight is still a report");
    assert!(!report.is_ready());
    assert_eq!(report.axes[6].reason, "migration_space_insufficient");
    assert_eq!(report.axes[7].reason, "migration_clock_untrusted");
    assert_eq!(report.axes[8].reason, "migration_old_writer_active");

    let mut downgrade = facts();
    downgrade.schema.current_format_version = 9;
    let report =
        MigrationPreflightReport::evaluate(&registry(), &downgrade, 1_700_000_000_000).unwrap();
    assert_eq!(report.axes[1].reason, "migration_downgrade_denied");
}

#[test]
fn preflight_report_is_strictly_serialized() {
    let report =
        MigrationPreflightReport::evaluate(&registry(), &facts(), 1_700_000_000_000).unwrap();
    let mut value = serde_json::to_value(report).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<MigrationPreflightReport>(value).is_err());

    let axis = MigrationAxisResult {
        axis: MigrationPreflightAxis::Store,
        status: MigrationPreflightStatus::Ready,
        reason: "ok".to_owned(),
        remediation: "none".to_owned(),
        observed_digest: DIGEST_A.to_owned(),
    };
    assert_eq!(axis.status, MigrationPreflightStatus::Ready);
}
