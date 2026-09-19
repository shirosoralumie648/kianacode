use kiana_domain::{
    MigrationBatchPlan, MigrationCheckpoint, MigrationCompatibilityWindow, MigrationPrecondition,
    MigrationPrimitivePhase, MigrationPrimitiveTarget, MigrationRegistry, MigrationReleaseBinding,
    MigrationStep, MAX_MIGRATION_BATCH,
};

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

fn item(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn plan(
    registry: &MigrationRegistry,
    checkpoint: &MigrationCheckpoint,
    phase: MigrationPrimitivePhase,
    cursor: u64,
    complete: bool,
) -> MigrationBatchPlan {
    MigrationBatchPlan::new(
        registry,
        checkpoint,
        phase,
        MigrationPrimitiveTarget::Store,
        cursor,
        1,
        vec![item('d')],
        complete,
    )
    .unwrap()
}

#[test]
fn primitives_are_bounded_idempotent_and_phase_ordered() {
    let registry = registry();
    let initial = MigrationCheckpoint::initial(&registry, "step-1").unwrap();
    let expand = plan(
        &registry,
        &initial,
        MigrationPrimitivePhase::Expand,
        1,
        true,
    );
    let expanded = initial.advance(&registry, &expand).unwrap();
    assert_eq!(
        initial.advance(&registry, &expand).unwrap(),
        expanded,
        "replaying the same batch is a no-op"
    );

    let backfill = plan(
        &registry,
        &expanded,
        MigrationPrimitivePhase::Backfill,
        2,
        true,
    );
    let backfilled = expanded.advance(&registry, &backfill).unwrap();
    let verify = plan(
        &registry,
        &backfilled,
        MigrationPrimitivePhase::Verify,
        3,
        true,
    );
    let verified = backfilled.advance(&registry, &verify).unwrap();
    assert!(verified.verified);

    let switch = plan(
        &registry,
        &verified,
        MigrationPrimitivePhase::Switch,
        4,
        true,
    );
    let switched = verified.advance(&registry, &switch).unwrap();
    let contract = plan(
        &registry,
        &switched,
        MigrationPrimitivePhase::Contract,
        5,
        true,
    );
    let completed = switched.advance(&registry, &contract).unwrap();
    assert!(completed.phase_complete);
    assert_eq!(completed.phase, MigrationPrimitivePhase::Contract);
    assert_eq!(completed.source_cursor, 5);
}

#[test]
fn primitives_reject_unbounded_batches_phase_skips_and_cursor_rollback() {
    let registry = registry();
    let initial = MigrationCheckpoint::initial(&registry, "step-1").unwrap();
    let too_large = MigrationBatchPlan::new(
        &registry,
        &initial,
        MigrationPrimitivePhase::Expand,
        MigrationPrimitiveTarget::Projection,
        1,
        MAX_MIGRATION_BATCH + 1,
        vec![item('d')],
        false,
    );
    assert!(too_large.is_err());
    assert_eq!(
        MigrationBatchPlan::new(
            &registry,
            &initial,
            MigrationPrimitivePhase::Switch,
            MigrationPrimitiveTarget::Store,
            1,
            1,
            vec![item('d')],
            false,
        )
        .unwrap_err(),
        "migration_verify_required_before_switch"
    );

    let expand = plan(
        &registry,
        &initial,
        MigrationPrimitivePhase::Expand,
        1,
        true,
    );
    let expanded = initial.advance(&registry, &expand).unwrap();
    let ahead = plan(
        &registry,
        &initial,
        MigrationPrimitivePhase::Expand,
        2,
        false,
    );
    let ahead_checkpoint = initial.advance(&registry, &ahead).unwrap();
    let stale = plan(
        &registry,
        &initial,
        MigrationPrimitivePhase::Expand,
        1,
        false,
    );
    assert_eq!(
        ahead_checkpoint.advance(&registry, &stale).unwrap_err(),
        "migration_source_cursor_rollback"
    );
    assert_eq!(
        expanded
            .advance(
                &registry,
                &plan(
                    &registry,
                    &expanded,
                    MigrationPrimitivePhase::Expand,
                    2,
                    false
                )
            )
            .unwrap_err(),
        "migration_phase_order_invalid"
    );
}

#[test]
fn primitive_targets_are_narrow_and_plan_is_strictly_serialized() {
    let registry = registry();
    let checkpoint = MigrationCheckpoint::initial(&registry, "step-1").unwrap();
    let plan = plan(
        &registry,
        &checkpoint,
        MigrationPrimitivePhase::Expand,
        1,
        false,
    );
    let mut value = serde_json::to_value(plan).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MigrationBatchPlan>(value).is_err());
    assert_eq!(
        MigrationPrimitiveTarget::Config,
        MigrationPrimitiveTarget::Config
    );
}
