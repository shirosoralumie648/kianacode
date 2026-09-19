use kiana_domain::{
    SecurityRehearsalCase, SecurityRehearsalMatrix, SecurityRehearsalScenario, UatOutcome,
};

const SOURCE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OP: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FACT: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn case(scenario: SecurityRehearsalScenario, outcome: UatOutcome) -> SecurityRehearsalCase {
    let success = matches!(
        outcome,
        UatOutcome::Succeeded | UatOutcome::RestartRecovered | UatOutcome::Replayed
    );
    let unknown = outcome == UatOutcome::ResultUnknown;
    let quarantine = scenario == SecurityRehearsalScenario::RestoreQuarantine && success;
    let reconcile = scenario == SecurityRehearsalScenario::UnknownReconcile && success;
    let retention = scenario == SecurityRehearsalScenario::RetentionPrune && success;
    SecurityRehearsalCase::new(
        scenario,
        outcome,
        OP,
        1,
        FACT,
        success,
        quarantine,
        success,
        reconcile || unknown,
        retention,
        retention,
        false,
    )
    .unwrap()
}

fn matrix() -> SecurityRehearsalMatrix {
    let scenarios = [
        SecurityRehearsalScenario::Restart,
        SecurityRehearsalScenario::RestoreQuarantine,
        SecurityRehearsalScenario::Replay,
        SecurityRehearsalScenario::UnknownReconcile,
        SecurityRehearsalScenario::RetentionPrune,
    ];
    let mut cases = scenarios
        .into_iter()
        .flat_map(|scenario| {
            [
                case(scenario, UatOutcome::Denied),
                case(scenario, UatOutcome::Succeeded),
            ]
        })
        .collect::<Vec<_>>();
    cases.push(case(
        SecurityRehearsalScenario::UnknownReconcile,
        UatOutcome::ResultUnknown,
    ));
    SecurityRehearsalMatrix::new(SOURCE, cases).unwrap()
}

#[test]
fn rehearsal_covers_fence_quarantine_replay_unknown_and_retention() {
    let matrix = matrix();
    assert_eq!(matrix.cases.len(), 11);
    assert!(matrix.validate().is_ok());
}

#[test]
fn old_lease_and_retention_guards_fail_closed() {
    let unfenced = SecurityRehearsalCase::new(
        SecurityRehearsalScenario::Restart,
        UatOutcome::Succeeded,
        OP,
        1,
        FACT,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
    );
    assert_eq!(
        unfenced.unwrap_err(),
        "security_rehearsal_fence_or_duplicate_guard_failed"
    );

    let hold_ignored = SecurityRehearsalCase::new(
        SecurityRehearsalScenario::RetentionPrune,
        UatOutcome::Succeeded,
        OP,
        1,
        FACT,
        true,
        false,
        true,
        false,
        true,
        false,
        false,
    );
    assert_eq!(
        hold_ignored.unwrap_err(),
        "security_rehearsal_retention_guard_failed"
    );
}

#[test]
fn unknown_rehearsal_cannot_retry() {
    let unknown = SecurityRehearsalCase::new(
        SecurityRehearsalScenario::UnknownReconcile,
        UatOutcome::ResultUnknown,
        OP,
        1,
        FACT,
        false,
        false,
        false,
        true,
        false,
        true,
        true,
    );
    assert_eq!(
        unknown.unwrap_err(),
        "security_rehearsal_unknown_retry_forbidden"
    );
}
