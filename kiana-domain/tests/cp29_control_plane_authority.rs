use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn fact(
    sequence: u64,
    command_id: &str,
    command_digest: &str,
    before: Cp29LifecycleState,
    after: Cp29LifecycleState,
    permission_before: u32,
    permission_after: u32,
    effect_count: u32,
    replayed: bool,
    unknown: bool,
) -> Cp29CommandFact {
    Cp29CommandFact::new(
        sequence,
        command_id,
        command_digest,
        before,
        after,
        7,
        permission_before,
        permission_after,
        10,
        4,
        0,
        effect_count,
        replayed,
        unknown,
    )
}

fn matrix() -> Vec<Cp29CrashObservation> {
    vec![
        Cp29CrashObservation::new(
            Cp29CrashPoint::WriteFrame,
            false,
            false,
            Cp29LifecycleState::Paused,
            true,
            "fixture:write-frame",
        ),
        Cp29CrashObservation::new(
            Cp29CrashPoint::Sync,
            false,
            false,
            Cp29LifecycleState::Paused,
            true,
            "fixture:sync",
        ),
        Cp29CrashObservation::new(
            Cp29CrashPoint::Approval,
            false,
            false,
            Cp29LifecycleState::Paused,
            false,
            "fixture:approval",
        ),
        Cp29CrashObservation::new(
            Cp29CrashPoint::Prepare,
            false,
            false,
            Cp29LifecycleState::Paused,
            false,
            "fixture:prepare",
        ),
        Cp29CrashObservation::new(
            Cp29CrashPoint::Dispatch,
            true,
            false,
            Cp29LifecycleState::ResultUnknown,
            true,
            "fixture:dispatch",
        ),
        Cp29CrashObservation::new(
            Cp29CrashPoint::Result,
            true,
            false,
            Cp29LifecycleState::ResultUnknown,
            true,
            "fixture:result",
        ),
        Cp29CrashObservation::new(
            Cp29CrashPoint::Replay,
            true,
            true,
            Cp29LifecycleState::Completed,
            false,
            "fixture:replay",
        ),
    ]
}

fn scenario(
    facts: Vec<Cp29CommandFact>,
    observations: Vec<Cp29CrashObservation>,
) -> Cp29AuthorityScenario {
    let folded = Cp29AuthorityScenario::fold_digest(&facts);
    Cp29AuthorityScenario::new(
        "cp29-authority-seed-1",
        facts,
        observations,
        folded.clone(),
        folded,
    )
}

#[test]
fn adversarial_command_sequence_preserves_authority_budget_terminal_and_replay_invariants() {
    let command = hash('a');
    let facts = vec![
        fact(
            1,
            "command-1",
            &command,
            Cp29LifecycleState::Queued,
            Cp29LifecycleState::Running,
            5,
            3,
            0,
            false,
            false,
        ),
        fact(
            2,
            "command-2",
            &hash('b'),
            Cp29LifecycleState::Running,
            Cp29LifecycleState::ResultUnknown,
            3,
            3,
            1,
            false,
            true,
        ),
        fact(
            3,
            "command-2",
            &hash('b'),
            Cp29LifecycleState::ResultUnknown,
            Cp29LifecycleState::ResultUnknown,
            3,
            3,
            0,
            true,
            true,
        ),
    ];
    let valid = scenario(facts.clone(), matrix());
    valid.validate().expect("scenario preserves invariants");
    assert_eq!(
        valid.online_digest,
        Cp29AuthorityScenario::fold_digest(&facts)
    );

    let mut permission_widened = fact(
        1,
        "command-1",
        &command,
        Cp29LifecycleState::Queued,
        Cp29LifecycleState::Running,
        3,
        4,
        0,
        false,
        false,
    );
    permission_widened.digest = permission_widened.canonical_digest();
    assert_eq!(
        permission_widened.validate().unwrap_err(),
        "cp29_command_fact_header_or_monotonicity_invalid"
    );

    let budget_overrun = Cp29CommandFact::new(
        1,
        "command-1",
        command,
        Cp29LifecycleState::Queued,
        Cp29LifecycleState::Running,
        7,
        3,
        3,
        10,
        11,
        0,
        0,
        false,
        false,
    );
    assert_eq!(
        budget_overrun.validate().unwrap_err(),
        "cp29_command_fact_header_or_monotonicity_invalid"
    );

    let terminal_revival = fact(
        2,
        "new-command",
        &hash('c'),
        Cp29LifecycleState::Completed,
        Cp29LifecycleState::Running,
        3,
        3,
        1,
        false,
        false,
    );
    assert_eq!(
        terminal_revival.validate().unwrap_err(),
        "cp29_terminal_cannot_revive"
    );

    let mut drift = facts;
    drift[2] = fact(
        3,
        "command-2",
        &hash('d'),
        Cp29LifecycleState::ResultUnknown,
        Cp29LifecycleState::ResultUnknown,
        3,
        3,
        0,
        true,
        true,
    );
    assert_eq!(
        scenario(drift, matrix()).validate().unwrap_err(),
        "cp29_command_idempotency_or_payload_drift"
    );
}

#[test]
fn crash_matrix_keeps_unconfirmed_effect_unknown_and_fenced() {
    let facts = vec![fact(
        1,
        "command-1",
        &hash('a'),
        Cp29LifecycleState::Queued,
        Cp29LifecycleState::Running,
        5,
        3,
        0,
        false,
        false,
    )];
    scenario(facts.clone(), matrix())
        .validate()
        .expect("complete crash matrix");

    let mut unfenced = matrix();
    unfenced[4] = Cp29CrashObservation::new(
        Cp29CrashPoint::Dispatch,
        true,
        false,
        Cp29LifecycleState::ResultUnknown,
        false,
        "fixture:dispatch",
    );
    assert_eq!(
        unfenced[4].validate().unwrap_err(),
        "cp29_crash_started_effect_requires_unknown_fence"
    );

    let mut duplicate_effect = matrix();
    duplicate_effect[6].duplicate_effect = true;
    duplicate_effect[6].digest = duplicate_effect[6].canonical_digest();
    assert_eq!(
        duplicate_effect[6].validate().unwrap_err(),
        "cp29_crash_observation_header_or_duplicate_invalid"
    );

    let mut incomplete = matrix();
    incomplete.pop();
    assert_eq!(
        scenario(facts, incomplete).validate().unwrap_err(),
        "cp29_crash_matrix_incomplete"
    );
}

#[test]
fn online_projection_must_match_complete_deterministic_replay_fold() {
    let facts = vec![fact(
        1,
        "command-1",
        &hash('a'),
        Cp29LifecycleState::Queued,
        Cp29LifecycleState::Running,
        5,
        3,
        0,
        false,
        false,
    )];
    let mut drift = scenario(facts, matrix());
    drift.replay_digest = hash('f');
    drift.digest = drift.canonical_digest();
    assert_eq!(
        drift.validate().unwrap_err(),
        "cp29_online_replay_digest_mismatch"
    );
}
