use kiana_domain::{
    ExecutionRevisionPin, LocalRolloutEvidence, LocalRolloutMode, LocalRolloutPhase,
    LocalRolloutState,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn pin() -> ExecutionRevisionPin {
    ExecutionRevisionPin::new(
        "revision-1",
        DIGEST_A,
        DIGEST_B,
        DIGEST_C,
        DIGEST_D,
        DIGEST_A,
        "kiana.replay.v1",
    )
    .unwrap()
}

fn evidence(
    preflight_ready: bool,
    backup_verified: bool,
    active_run_count: u32,
    active_writer_count: u32,
    old_revision_fenced: bool,
    replacement_started: bool,
    readiness_verified: bool,
    old_root_retained: bool,
) -> LocalRolloutEvidence {
    let mut value = LocalRolloutEvidence {
        preflight_ready,
        backup_verified,
        active_run_count,
        active_writer_count,
        old_revision_fenced,
        replacement_started,
        readiness_verified,
        old_root_retained,
        evidence_digest: String::new(),
    };
    value.evidence_digest = value.digest();
    value
}

#[test]
fn local_rollout_requires_ordered_evidence_before_promote() {
    let initial = evidence(false, false, 1, 1, false, false, false, true);
    let mut state =
        LocalRolloutState::new("rollout-1", LocalRolloutMode::ManagedLocal, pin(), initial)
            .unwrap();
    let ready = evidence(true, true, 0, 0, true, true, true, true);
    for phase in [
        LocalRolloutPhase::Preflight,
        LocalRolloutPhase::Backup,
        LocalRolloutPhase::Drain,
        LocalRolloutPhase::Replace,
        LocalRolloutPhase::Ready,
        LocalRolloutPhase::Promote,
    ] {
        state.advance(phase, ready.clone()).unwrap();
    }
    assert_eq!(state.phase, LocalRolloutPhase::Promote);
}

#[test]
fn rollout_blocks_missing_preflight_backup_drain_fence_and_readiness() {
    let initial = evidence(false, false, 1, 1, false, false, false, true);
    let mut state =
        LocalRolloutState::new("rollout-1", LocalRolloutMode::EmbeddedLocal, pin(), initial)
            .unwrap();
    assert_eq!(
        state
            .advance(
                LocalRolloutPhase::Preflight,
                evidence(false, false, 1, 1, false, false, false, true)
            )
            .unwrap_err(),
        "rollout_preflight_blocked"
    );
    state
        .advance(
            LocalRolloutPhase::Preflight,
            evidence(true, false, 1, 1, false, false, false, true),
        )
        .unwrap();
    assert_eq!(
        state
            .advance(
                LocalRolloutPhase::Backup,
                evidence(false, false, 1, 1, false, false, false, true)
            )
            .unwrap_err(),
        "rollout_preflight_required"
    );
    state
        .advance(
            LocalRolloutPhase::Backup,
            evidence(true, true, 1, 1, false, false, false, true),
        )
        .unwrap();
    assert_eq!(
        state
            .advance(
                LocalRolloutPhase::Drain,
                evidence(true, true, 1, 1, false, false, false, true)
            )
            .unwrap_err(),
        "rollout_drain_not_safe"
    );
    state
        .advance(
            LocalRolloutPhase::Drain,
            evidence(true, true, 0, 0, false, false, false, true),
        )
        .unwrap();
    assert_eq!(
        state
            .advance(
                LocalRolloutPhase::Replace,
                evidence(true, true, 0, 0, false, true, false, true)
            )
            .unwrap_err(),
        "rollout_replace_fence_or_backup_missing"
    );
}

#[test]
fn rollout_phase_order_and_unknown_fields_fail_closed() {
    let value = evidence(true, true, 0, 0, true, true, true, true);
    let mut state =
        LocalRolloutState::new("rollout-1", LocalRolloutMode::ManagedLocal, pin(), value).unwrap();
    assert_eq!(
        state
            .advance(
                LocalRolloutPhase::Ready,
                evidence(true, true, 0, 0, true, true, true, true)
            )
            .unwrap_err(),
        "rollout_phase_order_invalid"
    );
    let mut serialized = serde_json::to_value(state).unwrap();
    serialized["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<LocalRolloutState>(serialized).is_err());
}
