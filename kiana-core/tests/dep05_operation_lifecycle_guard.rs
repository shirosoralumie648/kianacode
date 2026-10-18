#[test]
fn dep05_operation_lifecycle_is_pure_and_replayable() {
    let domain = include_str!("../../kiana-domain/src/operation_lifecycle.rs");
    let core = include_str!("../src/deployment_operation.rs");
    for marker in [
        "OperationJournal",
        "OperationTransition",
        "OperationTransitionKind",
        "PhaseDeadline",
        "operation_preflight_required",
        "operation_revision_late_event",
        "operation_drain_timeout_not_due",
        "operation_terminal_immutable",
        "replay_operation_journal",
        "source_cursor",
        "evidence_refs",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-05 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "dispatch_capability",
        "start_revision",
        "stop_revision",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-05 lifecycle path crossed an effect boundary: {forbidden}"
        );
    }
}

use kiana_core::replay_operation_journal;
use kiana_domain::{
    OperationJournal, OperationPhase, OperationState, OperationTransition, OperationTransitionKind,
    PhaseDeadline,
};

const REVISION_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn operation_id() -> kiana_domain::OperationId {
    kiana_domain::OperationId::parse_str("00000000-0000-0000-0000-000000000007")
        .expect("operation id")
}

fn transition(
    from: OperationState,
    kind: OperationTransitionKind,
    sequence: u64,
    source_cursor: u64,
    observed_at_unix_ms: u64,
    deadline: Option<PhaseDeadline>,
) -> OperationTransition {
    OperationTransition::new(
        operation_id(),
        "revision-a",
        REVISION_DIGEST,
        sequence,
        source_cursor,
        from,
        kind,
        format!("{kind:?}"),
        vec![format!("evidence.{sequence}")],
        observed_at_unix_ms,
        deadline,
    )
    .unwrap()
}

fn ordered_transitions() -> Vec<OperationTransition> {
    let drain_deadline = PhaseDeadline::new(OperationPhase::Draining, 200).unwrap();
    vec![
        transition(
            OperationState::Created,
            OperationTransitionKind::PreflightComplete,
            1,
            10,
            100,
            None,
        ),
        transition(
            OperationState::Preflight,
            OperationTransitionKind::DrainStart,
            2,
            11,
            110,
            Some(drain_deadline),
        ),
        transition(
            OperationState::Draining,
            OperationTransitionKind::ExecuteStart,
            3,
            12,
            120,
            None,
        ),
        transition(
            OperationState::Executing,
            OperationTransitionKind::Complete,
            4,
            13,
            130,
            None,
        ),
    ]
}

fn completed_journal() -> OperationJournal {
    OperationJournal::replay(
        operation_id(),
        "revision-a",
        REVISION_DIGEST,
        ordered_transitions(),
    )
    .unwrap()
}

#[test]
fn dep05_replay_requires_exact_persisted_snapshot() {
    let journal = completed_journal();

    // 成功路径：完整有序序列重建出与快照逐字段相等的 journal。
    let replayed = replay_operation_journal(&journal, ordered_transitions()).unwrap();
    assert_eq!(replayed, journal);
    assert_eq!(replayed.current_state, OperationState::Completed);

    // 拒绝优先：截断序列不能静默返回更早的有效投影。
    let truncated: Vec<_> = ordered_transitions().into_iter().take(3).collect();
    assert_eq!(
        replay_operation_journal(&journal, truncated).unwrap_err(),
        "operation_journal_replay_mismatch"
    );

    // 空序列同样不能伪装成完整快照。
    assert_eq!(
        replay_operation_journal(&journal, Vec::new()).unwrap_err(),
        "operation_journal_replay_mismatch"
    );

    // 不同 cursor 但内部自洽的等价状态序列也必须与 persisted snapshot 不一致。
    let mut cursor_drift = ordered_transitions();
    for (offset, transition) in cursor_drift.iter_mut().enumerate() {
        transition.source_cursor = 20 + offset as u64;
        transition.transition_digest = transition.digest();
    }
    assert_eq!(
        replay_operation_journal(&journal, cursor_drift).unwrap_err(),
        "operation_journal_replay_mismatch"
    );
}

#[test]
fn dep05_replay_rejects_invalid_persisted_snapshot_first() {
    let mut forged = completed_journal();
    forged.transitions[0].reason = "OperationComplete".to_owned();
    // 只重算外层摘要；内层 transition_digest 不再匹配。
    forged.journal_digest = forged.digest();
    assert_eq!(
        replay_operation_journal(&forged, ordered_transitions()).unwrap_err(),
        "operation_transition_digest_mismatch"
    );

    let mut truncated_projection = completed_journal();
    truncated_projection.current_state = OperationState::Executing;
    truncated_projection.current_phase = OperationPhase::Executing;
    truncated_projection.journal_digest = truncated_projection.digest();
    assert_eq!(
        replay_operation_journal(&truncated_projection, ordered_transitions()).unwrap_err(),
        "operation_journal_projection_mismatch"
    );
}

#[test]
fn dep05_replay_never_returns_partial_journal_on_failure() {
    // 失败路径只返回稳定错误字符串，不产生可被误用的部分成功 journal。
    let journal = completed_journal();
    let mut bad_sequence = ordered_transitions();
    bad_sequence[2].sequence = 5;
    bad_sequence[2].transition_digest = bad_sequence[2].digest();
    let error = replay_operation_journal(&journal, bad_sequence).unwrap_err();
    assert_eq!(error, "operation_sequence_not_contiguous");
    // 输入快照未被修改。
    assert_eq!(journal, completed_journal());
}
