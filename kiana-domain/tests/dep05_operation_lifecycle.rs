use kiana_domain::{
    schema_contract, OperationJournal, OperationPhase, OperationState, OperationTransition,
    OperationTransitionKind, PhaseDeadline, MAX_OPERATION_TRANSITIONS, OPERATION_LIFECYCLE_SCHEMA,
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

fn transition_with_evidence(
    from: OperationState,
    kind: OperationTransitionKind,
    sequence: u64,
    source_cursor: u64,
    evidence_refs: Vec<String>,
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
        evidence_refs,
        100,
        None,
    )
    .unwrap()
}

/// 已通过完整状态机折叠的 terminal journal，供拒绝夹具与重放比较复用。
fn completed_journal() -> OperationJournal {
    let drain_deadline = PhaseDeadline::new(OperationPhase::Draining, 200).unwrap();
    OperationJournal::replay(
        operation_id(),
        "revision-a",
        REVISION_DIGEST,
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
        ],
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

#[test]
fn lifecycle_denies_transition_without_evidence() {
    // 拒绝优先：构造阶段就拒绝空 evidence 列表。
    let constructed = OperationTransition::new(
        operation_id(),
        "revision-a",
        REVISION_DIGEST,
        1,
        10,
        OperationState::Created,
        OperationTransitionKind::PreflightComplete,
        "PreflightComplete",
        Vec::new(),
        100,
        None,
    );
    assert_eq!(constructed.unwrap_err(), "operation_evidence_required");

    // wire 伪造：重算 evidence/transition 摘要也不能绕过非空证据要求。
    let mut forged = transition(
        OperationState::Created,
        OperationTransitionKind::PreflightComplete,
        1,
        10,
        100,
        None,
    );
    forged.evidence_refs.clear();
    forged.evidence_digest = kiana_domain::json_digest(&serde_json::json!(forged.evidence_refs));
    forged.transition_digest = forged.digest();
    assert_eq!(
        forged.validate().unwrap_err(),
        "operation_evidence_required"
    );

    // append 也必须传播同一拒绝码，并保持 journal 逐字段不变。
    let mut target = journal();
    let before = target.clone();
    assert_eq!(
        target.append(forged).unwrap_err(),
        "operation_evidence_required"
    );
    assert_eq!(target, before);

    // 已创建的 bootstrap journal 由 `operation.created` 提供初始证据。
    let created = journal();
    assert_eq!(created.evidence_refs, vec!["operation.created".to_owned()]);
    assert_eq!(created.sequence, 0);
    assert!(created.transitions.is_empty());
}

#[test]
fn journal_validate_rejects_forged_nested_transition_digest() {
    let mut journal = completed_journal();
    journal.transitions[0].reason = "OperationComplete".to_owned();
    // 攻击者只重算外层 journal_digest；内层 transition_digest 与字段不再一致。
    journal.journal_digest = journal.digest();
    assert_eq!(
        journal.validate().unwrap_err(),
        "operation_transition_digest_mismatch"
    );

    // 只改内层 state 并同步内层摘要，仍会被 expected transition 映射拒绝。
    let mut state_forged = completed_journal();
    state_forged.transitions[1].state = OperationState::Unknown;
    state_forged.transitions[1].transition_digest = state_forged.transitions[1].digest();
    state_forged.journal_digest = state_forged.digest();
    assert_eq!(
        state_forged.validate().unwrap_err(),
        "operation_transition_state_mismatch"
    );
}

#[test]
fn journal_validate_rejects_empty_projection_evidence() {
    let mut journal = completed_journal();
    journal.evidence_refs.clear();
    journal.evidence_digest = kiana_domain::json_digest(&serde_json::json!(journal.evidence_refs));
    journal.journal_digest = journal.digest();
    assert_eq!(
        journal.validate().unwrap_err(),
        "operation_evidence_required"
    );
}

#[test]
fn append_error_leaves_journal_unmodified() {
    // 容量围栏是防御性上界：合法状态机最长链远小于 256，因此直接检查常量与拒绝不变性。
    assert_eq!(MAX_OPERATION_TRANSITIONS, 256);

    let mut target = journal();
    let before = target.clone();
    let skip = transition(
        OperationState::Preflight,
        OperationTransitionKind::ExecuteStart,
        1,
        10,
        100,
        None,
    );
    assert_eq!(
        target.append(skip).unwrap_err(),
        "operation_from_state_mismatch"
    );
    assert_eq!(target, before);

    let mut cursor = completed_journal();
    let cursor_before = cursor.clone();
    let regressed = transition(
        OperationState::Executing,
        OperationTransitionKind::Complete,
        5,
        13,
        140,
        None,
    );
    // terminal fence 先于 cursor 检查触发，但仍必须保持原 journal 不变。
    assert_eq!(
        cursor.append(regressed).unwrap_err(),
        "operation_terminal_immutable"
    );
    assert_eq!(cursor, cursor_before);

    let mut preflight_journal = journal();
    preflight_journal
        .append(transition(
            OperationState::Created,
            OperationTransitionKind::PreflightComplete,
            1,
            10,
            100,
            None,
        ))
        .unwrap();
    let preflight_before = preflight_journal.clone();
    let bad_cursor = transition(
        OperationState::Preflight,
        OperationTransitionKind::ExecuteStart,
        2,
        10,
        101,
        None,
    );
    assert_eq!(
        preflight_journal.append(bad_cursor).unwrap_err(),
        "operation_source_cursor_regressed"
    );
    assert_eq!(preflight_journal, preflight_before);
}

#[test]
fn replay_still_rebuilds_same_digest_with_non_empty_evidence() {
    let first = completed_journal();
    let second = completed_journal();
    assert_eq!(first, second);
    assert_eq!(first.journal_digest, second.journal_digest);
    assert!(first
        .transitions
        .iter()
        .all(|transition| !transition.evidence_refs.is_empty()));
}

#[test]
fn append_rejects_at_capacity_without_mutation() {
    // 用合法 16 状态链逼近上限的确定性夹具太长；这里直接构造一个“已到容量上界”的畸形 journal，
    // 证明容量围栏先于任何校验/投影写入返回稳定错误，且对象逐字段不变。
    let mut journal = journal();
    journal.transitions = Vec::new();
    // 伪造一个长度等于上界、但内部不一致的 transition 列表（重复 clone 同一事实）。
    let filler = transition(
        OperationState::Created,
        OperationTransitionKind::PreflightComplete,
        1,
        10,
        100,
        None,
    );
    journal.transitions = vec![filler; MAX_OPERATION_TRANSITIONS];
    let before = journal.clone();
    let rejected = transition(
        OperationState::Preflight,
        OperationTransitionKind::DrainStart,
        2,
        11,
        110,
        Some(PhaseDeadline::new(OperationPhase::Draining, 200).unwrap()),
    );
    assert_eq!(
        journal.append(rejected).unwrap_err(),
        "operation_transition_limit_or_sequence_invalid"
    );
    assert_eq!(journal, before);
}
