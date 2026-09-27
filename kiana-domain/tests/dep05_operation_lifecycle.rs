use kiana_domain::{
    schema_contract, OperationJournal, OperationPhase, OperationState, OperationTransition,
    OperationTransitionKind, PhaseDeadline, OPERATION_LIFECYCLE_SCHEMA,
};
use uuid::Uuid;

const REVISION_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn operation_id() -> kiana_domain::OperationId {
    kiana_domain::OperationId::from_uuid(Uuid::from_u128(1))
}

#[test]
fn lifecycle_schema_is_registered_as_a_strict_domain_contract() {
    let contract = schema_contract(OPERATION_LIFECYCLE_SCHEMA).unwrap();
    assert_eq!(contract.owner_crate, "kiana-domain");
    assert!(!contract.allow_unknown_fields);
}

fn journal() -> OperationJournal {
    OperationJournal::new(operation_id(), "revision-a", REVISION_DIGEST).unwrap()
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

#[test]
fn operation_replay_rebuilds_terminal_projection() {
    let drain_deadline = PhaseDeadline::new(OperationPhase::Draining, 200).unwrap();
    let transitions = vec![
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
    ];
    let replayed = OperationJournal::replay(
        operation_id(),
        "revision-a",
        REVISION_DIGEST,
        transitions.clone(),
    )
    .unwrap();
    assert_eq!(replayed.current_state, OperationState::Completed);
    assert_eq!(replayed.current_phase, OperationPhase::Terminal);
    assert!(replayed.preflight_completed);
    assert_eq!(replayed.source_cursor, 13);
    assert_eq!(replayed.transitions, transitions);
    assert_eq!(
        serde_json::from_str::<OperationJournal>(&serde_json::to_string(&replayed).unwrap())
            .unwrap(),
        replayed
    );
}

#[test]
fn lifecycle_denies_skip_late_timeout_and_duplicate_terminal_paths() {
    let mut skipped = journal();
    let execute = transition(
        OperationState::Preflight,
        OperationTransitionKind::ExecuteStart,
        1,
        10,
        100,
        None,
    );
    assert_eq!(
        skipped.append(execute).unwrap_err(),
        "operation_from_state_mismatch"
    );

    let mut late = journal();
    let mut old_revision = transition(
        OperationState::Created,
        OperationTransitionKind::PreflightComplete,
        1,
        10,
        100,
        None,
    );
    old_revision.revision_digest =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    old_revision.transition_digest = old_revision.digest();
    assert_eq!(
        late.append(old_revision).unwrap_err(),
        "operation_revision_late_event"
    );

    let deadline = PhaseDeadline::new(OperationPhase::Draining, 200).unwrap();
    let mut timeout = journal();
    timeout
        .append(transition(
            OperationState::Created,
            OperationTransitionKind::PreflightComplete,
            1,
            10,
            100,
            None,
        ))
        .unwrap();
    timeout
        .append(transition(
            OperationState::Preflight,
            OperationTransitionKind::DrainStart,
            2,
            11,
            110,
            Some(deadline.clone()),
        ))
        .unwrap();
    timeout
        .append(transition(
            OperationState::Draining,
            OperationTransitionKind::DrainTimeout,
            3,
            12,
            201,
            Some(deadline),
        ))
        .unwrap();
    assert_eq!(timeout.current_state, OperationState::Unknown);
    assert_eq!(timeout.current_phase, OperationPhase::Terminal);
    assert_ne!(timeout.current_state, OperationState::Completed);

    let duplicate_terminal = transition(
        OperationState::Executing,
        OperationTransitionKind::Complete,
        4,
        13,
        202,
        None,
    );
    assert_eq!(
        timeout.append(duplicate_terminal).unwrap_err(),
        "operation_terminal_immutable"
    );
}

#[test]
fn lifecycle_rejects_forged_state_and_cursor_regression() {
    let mut forged = journal();
    let mut preflight = transition(
        OperationState::Created,
        OperationTransitionKind::PreflightComplete,
        1,
        10,
        100,
        None,
    );
    preflight.state = OperationState::Failed;
    preflight.transition_digest = preflight.digest();
    assert_eq!(
        forged.append(preflight).unwrap_err(),
        "operation_transition_state_mismatch"
    );

    let mut cursor = journal();
    cursor
        .append(transition(
            OperationState::Created,
            OperationTransitionKind::PreflightComplete,
            1,
            10,
            100,
            None,
        ))
        .unwrap();
    let regressed = transition(
        OperationState::Preflight,
        OperationTransitionKind::ExecuteStart,
        2,
        10,
        101,
        None,
    );
    assert_eq!(
        cursor.append(regressed).unwrap_err(),
        "operation_source_cursor_regressed"
    );
}
