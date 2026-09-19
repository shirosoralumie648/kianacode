use kiana_domain::{
    MigrationBatchPlan, MigrationCompatibilityWindow, MigrationPrecondition,
    MigrationPrimitivePhase, MigrationPrimitiveTarget, MigrationRegistry, MigrationReleaseBinding,
    MigrationRunnerEventKind, MigrationRunnerState, MigrationRunnerStatus, MigrationStep,
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

fn item() -> String {
    DIGEST_D.to_owned()
}

fn step_plan(
    registry: &MigrationRegistry,
    state: &MigrationRunnerState,
    phase: MigrationPrimitivePhase,
    cursor: u64,
    complete: bool,
) -> MigrationBatchPlan {
    MigrationBatchPlan::new(
        registry,
        &state.checkpoint,
        phase,
        MigrationPrimitiveTarget::Store,
        cursor,
        1,
        vec![item()],
        complete,
    )
    .unwrap()
}

#[test]
fn runner_emits_ordered_events_and_completes_only_after_contract() {
    let registry = registry();
    let (mut state, started) =
        MigrationRunnerState::start(&registry, "run-1", "owner", 1, 100, 200, None).unwrap();
    assert_eq!(started.kind, MigrationRunnerEventKind::Started);
    let token = state.resume_token().unwrap();
    state.resume(&token, 101).unwrap();

    for (phase, cursor) in [
        (MigrationPrimitivePhase::Expand, 1),
        (MigrationPrimitivePhase::Backfill, 2),
        (MigrationPrimitivePhase::Verify, 3),
        (MigrationPrimitivePhase::Switch, 4),
        (MigrationPrimitivePhase::Contract, 5),
    ] {
        let plan = step_plan(&registry, &state, phase, cursor, true);
        let event = state.apply_step(&registry, &plan, 110).unwrap();
        assert_eq!(event.kind, MigrationRunnerEventKind::Step);
    }
    let completed = state.complete(120).unwrap();
    assert_eq!(completed.kind, MigrationRunnerEventKind::Completed);
    assert_eq!(state.status, MigrationRunnerStatus::Completed);
    assert_eq!(
        state.resume_token().unwrap_err(),
        "migration_resume_not_active"
    );
}

#[test]
fn runner_rejects_second_owner_expired_lease_bad_resume_and_quarantine_continue() {
    let registry = registry();
    let (state, _) =
        MigrationRunnerState::start(&registry, "run-1", "owner", 4, 100, 200, None).unwrap();
    assert_eq!(
        MigrationRunnerState::start(&registry, "run-2", "other", 5, 110, 210, Some(&state))
            .unwrap_err(),
        "migration_runner_concurrent"
    );
    let (expired, _) =
        MigrationRunnerState::start(&registry, "run-2", "other", 5, 210, 220, Some(&state))
            .unwrap();
    assert_eq!(expired.fence_token, 5);

    let mut bad_token = state.resume_token().unwrap();
    bad_token.owner = "forged".to_owned();
    assert_eq!(
        state.resume(&bad_token, 120).unwrap_err(),
        "migration_resume_token_digest_mismatch"
    );

    let (mut blocked, _) =
        MigrationRunnerState::start(&registry, "run-3", "owner", 6, 100, 200, None).unwrap();
    let event = blocked.block("checksum drift").unwrap();
    assert_eq!(event.kind, MigrationRunnerEventKind::Blocked);
    let plan = step_plan(
        &registry,
        &blocked,
        MigrationPrimitivePhase::Expand,
        1,
        false,
    );
    assert_eq!(
        blocked.apply_step(&registry, &plan, 120).unwrap_err(),
        "migration_failure_quarantined"
    );
    assert_eq!(
        MigrationRunnerState::start(&registry, "run-4", "owner", 7, 100, 200, Some(&blocked))
            .unwrap_err(),
        "migration_failure_quarantined"
    );
}

#[test]
fn runner_rejects_expiry_and_registry_checksum_drift() {
    let registry = registry();
    let (mut state, _) =
        MigrationRunnerState::start(&registry, "run-1", "owner", 1, 100, 200, None).unwrap();
    let plan = step_plan(&registry, &state, MigrationPrimitivePhase::Expand, 1, false);
    assert_eq!(
        state.apply_step(&registry, &plan, 200).unwrap_err(),
        "migration_lease_expired"
    );

    let drift = MigrationRegistry::new(
        MigrationReleaseBinding {
            release_manifest_digest: DIGEST_A.to_owned(),
            artifact_digest: DIGEST_B.to_owned(),
            signature_digest: DIGEST_D.to_owned(),
        },
        registry.ordered_steps().to_vec(),
    )
    .unwrap();
    assert_eq!(
        state.apply_step(&drift, &plan, 120).unwrap_err(),
        "migration_registry_checksum_drift"
    );
}
