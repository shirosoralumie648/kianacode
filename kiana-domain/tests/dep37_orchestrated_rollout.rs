use kiana_domain::{
    CanaryObservation, ExecutionRevisionPin, OrchestratedBackend, OrchestratedRolloutAction,
    OrchestratedRolloutPlan, OrchestratedRolloutProfile, WorkerBuildRoute, WorkerBuildRoutingTable,
    FULL_TRAFFIC_BPS,
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

fn canary(passed: bool) -> CanaryObservation {
    let (error_rate, availability) = if passed { (10, 9_990) } else { (500, 9_000) };
    CanaryObservation::new(100, error_rate, availability, 100, 9_900, 1_000, 2).unwrap()
}

fn plan(old_accepts_writes: bool, canary_passed: bool) -> OrchestratedRolloutPlan {
    let old = pin("revision-old", DIGEST_A);
    let target = pin("revision-target", DIGEST_B);
    let routes = vec![
        WorkerBuildRoute {
            pin: old,
            traffic_weight_bps: if old_accepts_writes { 9_900 } else { 100 },
            accepts_writes: old_accepts_writes,
        },
        WorkerBuildRoute {
            pin: target,
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
        canary(canary_passed),
        1,
        10_000,
    )
    .unwrap()
}

#[test]
fn promote_rejects_old_writer_and_failed_canary() {
    let old_writer = plan(true, true);
    let decision = old_writer
        .decide(OrchestratedRolloutAction::Promote, 2, None)
        .unwrap();
    assert!(!decision.allowed);
    assert_eq!(decision.reason, "rollout_old_worker_not_fenced");

    let failed_canary = plan(false, false);
    let decision = failed_canary
        .decide(OrchestratedRolloutAction::Promote, 2, None)
        .unwrap();
    assert!(!decision.allowed);
    assert_eq!(decision.reason, "rollout_canary_gate_blocked");
}

#[test]
fn target_routing_allows_promote_and_simulates_pause_resume_rollback() {
    let plan = plan(false, true);
    let promote = plan
        .decide(OrchestratedRolloutAction::Promote, 2, None)
        .unwrap();
    assert!(promote.allowed);

    let pause = plan
        .decide(OrchestratedRolloutAction::Pause, 2, Some("operator_review"))
        .unwrap();
    assert!(pause.allowed);

    let resume = plan
        .decide(OrchestratedRolloutAction::Resume, 2, None)
        .unwrap();
    assert!(resume.allowed);

    let rollback = plan
        .decide(
            OrchestratedRolloutAction::Rollback,
            20_000,
            Some("health_regression"),
        )
        .unwrap();
    assert!(rollback.allowed);
    assert_eq!(FULL_TRAFFIC_BPS, 10_000);
}

#[test]
fn real_orchestrator_backends_are_explicit_targets() {
    assert!(OrchestratedBackend::KubernetesTarget.is_target());
    assert!(OrchestratedBackend::GenericOrchestratorTarget.is_target());
    assert!(!OrchestratedBackend::Simulation.is_target());
}
