use kiana_domain::{
    CanaryObservation, ExecutionRevisionPin, OldRevisionRetention, OrchestratedBackend,
    OrchestratedRolloutPlan, OrchestratedRolloutProfile, OrchestratedRolloutState,
    PostDeployVerification, RolloutHealthWindow, WorkerBuildRoute, WorkerBuildRoutingTable,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn pin(revision: &str, build: &str) -> ExecutionRevisionPin {
    ExecutionRevisionPin::new(
        revision,
        build,
        DIGEST_B,
        DIGEST_C,
        DIGEST_D,
        DIGEST_A,
        "kiana.replay.v1",
    )
    .unwrap()
}

fn plan(old_accepts_writes: bool) -> OrchestratedRolloutPlan {
    let routes = vec![
        WorkerBuildRoute {
            pin: pin("revision-old", DIGEST_A),
            traffic_weight_bps: if old_accepts_writes { 9_900 } else { 100 },
            accepts_writes: old_accepts_writes,
        },
        WorkerBuildRoute {
            pin: pin("revision-target", DIGEST_B),
            traffic_weight_bps: if old_accepts_writes { 100 } else { 9_900 },
            accepts_writes: !old_accepts_writes,
        },
    ];
    let active_writer = if old_accepts_writes {
        "revision-old"
    } else {
        "revision-target"
    };
    OrchestratedRolloutPlan::new(
        "rollout-1",
        OrchestratedRolloutProfile::Canary,
        OrchestratedBackend::Simulation,
        "revision-target",
        "revision-old",
        WorkerBuildRoutingTable::new(routes, active_writer, DIGEST_C).unwrap(),
        CanaryObservation::new(100, 10, 9_990, 100, 9_900, 1_000, 2).unwrap(),
        1,
        10_000,
    )
    .unwrap()
}

fn health(failures: u64) -> RolloutHealthWindow {
    RolloutHealthWindow::new(3, 100, 100, failures, 1).unwrap()
}

#[test]
fn lifecycle_requires_verified_health_before_promote_and_retirement() {
    let mut state = OrchestratedRolloutState::new(plan(false), 1, DIGEST_D, 200).unwrap();
    state.start(2).unwrap();
    state.begin_drain(3, 1_000, true).unwrap();
    state.observe_drain(0, 0).unwrap();

    let failed_health = health(2);
    let failed_verification =
        PostDeployVerification::new("revision-target", true, true, &failed_health, DIGEST_A)
            .unwrap();
    assert_eq!(
        state
            .promote(100, failed_health, failed_verification)
            .unwrap_err(),
        "rollout_post_deploy_verification_failed"
    );

    let good_health = health(0);
    let verification =
        PostDeployVerification::new("revision-target", true, true, &good_health, DIGEST_A).unwrap();
    state.promote(100, good_health, verification).unwrap();
    assert_eq!(
        state.retire_old_revision(199).unwrap_err(),
        "rollout_retention_window_open"
    );
    state.retire_old_revision(200).unwrap();
    assert_eq!(state.phase, kiana_domain::RolloutLifecyclePhase::Retired);
    assert!(state.retention.deletion_eligible);
}

#[test]
fn pause_resume_rollback_and_old_writer_guards_are_explicit() {
    let mut paused = OrchestratedRolloutState::new(plan(true), 1, DIGEST_D, 200).unwrap();
    paused.start(2).unwrap();
    paused.pause("operator_review").unwrap();
    paused.resume(3).unwrap();
    assert_eq!(paused.phase, kiana_domain::RolloutLifecyclePhase::Running);
    paused.rollback(4, "canary_regression").unwrap();
    assert_eq!(
        paused.phase,
        kiana_domain::RolloutLifecyclePhase::RolledBack
    );

    let mut old_writer = OrchestratedRolloutState::new(plan(true), 1, DIGEST_D, 200).unwrap();
    old_writer.start(2).unwrap();
    old_writer.begin_drain(3, 1_000, true).unwrap();
    old_writer.observe_drain(0, 0).unwrap();
    let good_health = health(0);
    let verification =
        PostDeployVerification::new("revision-target", true, true, &good_health, DIGEST_A).unwrap();
    assert_eq!(
        old_writer
            .promote(100, good_health, verification)
            .unwrap_err(),
        "rollout_old_worker_not_fenced"
    );
}

#[test]
fn retention_contract_starts_with_old_root_retained() {
    let retention = OldRevisionRetention::new(DIGEST_D, 100).unwrap();
    assert!(retention.old_root_retained);
    assert!(!retention.deletion_eligible);
}
