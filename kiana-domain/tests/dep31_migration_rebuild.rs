use kiana_domain::{
    MigrationCompatibilityWindow, MigrationPrecondition, MigrationRebuildFacts,
    MigrationRebuildReport, MigrationRebuildStatus, MigrationRegistry, MigrationReleaseBinding,
    MigrationStep,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

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

fn facts(registry: &MigrationRegistry) -> MigrationRebuildFacts {
    MigrationRebuildFacts {
        registry_digest: registry.registry_digest.clone(),
        source_cursor: 10,
        projection_cursor: 10,
        source_generation: 1,
        projection_generation: 2,
        expected_generation: 2,
        index_generation: 2,
        receipt_generation: 2,
        source_replay_digest: DIGEST_A.to_owned(),
        projection_digest: DIGEST_A.to_owned(),
        index_digest: DIGEST_B.to_owned(),
        receipt_source_digest: DIGEST_A.to_owned(),
        receipt_refs_present: true,
    }
}

#[test]
fn rebuild_report_opens_ready_gate_only_on_cursor_generation_and_receipt_parity() {
    let registry = registry();
    let report = MigrationRebuildReport::evaluate(&registry, &facts(&registry)).unwrap();
    report.validate().unwrap();
    assert_eq!(report.status, MigrationRebuildStatus::Ready);
    assert!(report.ready_gate);
    assert_eq!(report.source_cursor, report.projection_cursor);
}

#[test]
fn rebuild_report_blocks_projection_overrun_lag_generation_and_receipt_drift() {
    let registry = registry();
    let mut over = facts(&registry);
    over.projection_cursor = 11;
    assert_eq!(
        MigrationRebuildReport::evaluate(&registry, &over)
            .unwrap()
            .reason,
        "migration_projection_over_facts"
    );

    let mut lag = facts(&registry);
    lag.projection_cursor = 9;
    assert_eq!(
        MigrationRebuildReport::evaluate(&registry, &lag)
            .unwrap()
            .reason,
        "migration_projection_cursor_lag"
    );

    let mut generation = facts(&registry);
    generation.projection_generation = 1;
    assert_eq!(
        MigrationRebuildReport::evaluate(&registry, &generation)
            .unwrap()
            .reason,
        "migration_projection_generation_stale"
    );

    let mut index = facts(&registry);
    index.index_generation = 1;
    assert_eq!(
        MigrationRebuildReport::evaluate(&registry, &index)
            .unwrap()
            .reason,
        "migration_index_generation_stale"
    );

    let mut receipt = facts(&registry);
    receipt.receipt_source_digest = DIGEST_D.to_owned();
    assert_eq!(
        MigrationRebuildReport::evaluate(&registry, &receipt)
            .unwrap()
            .reason,
        "migration_receipt_source_invariant_failed"
    );
}

#[test]
fn rebuild_report_rejects_registry_drift_and_strict_fields() {
    let registry = registry();
    let mut drift = facts(&registry);
    drift.registry_digest = DIGEST_D.to_owned();
    assert_eq!(
        MigrationRebuildReport::evaluate(&registry, &drift).unwrap_err(),
        "migration_rebuild_registry_checksum_drift"
    );
    let report = MigrationRebuildReport::evaluate(&registry, &facts(&registry)).unwrap();
    let mut value = serde_json::to_value(report).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MigrationRebuildReport>(value).is_err());
}
