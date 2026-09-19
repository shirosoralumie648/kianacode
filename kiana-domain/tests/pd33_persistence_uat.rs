use kiana_domain::{
    PersistenceUatCase, PersistenceUatMatrix, PersistenceUatStage, UatEntrypoint, UatOutcome,
};

const ROOT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const STORE: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SPINE: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RECEIPT: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn case(
    entrypoint: UatEntrypoint,
    stage: PersistenceUatStage,
    outcome: UatOutcome,
) -> PersistenceUatCase {
    let success = matches!(
        outcome,
        UatOutcome::Succeeded | UatOutcome::RestartRecovered | UatOutcome::Replayed
    );
    let unknown = outcome == UatOutcome::ResultUnknown;
    let (backup, quarantine, auth, parity, old_root, migration, journal, legal_hold, deletion) =
        if !success {
            (
                false, false, false, false, false, false, false, false, false,
            )
        } else {
            match stage {
                PersistenceUatStage::Backup => {
                    (true, false, true, false, true, false, false, false, false)
                }
                PersistenceUatStage::Restore => {
                    (true, true, true, true, true, false, false, false, false)
                }
                PersistenceUatStage::Upgrade => {
                    (false, false, true, false, true, true, false, false, false)
                }
                PersistenceUatStage::Restart => {
                    (false, false, true, true, true, false, true, false, false)
                }
                PersistenceUatStage::GovernanceDelete => {
                    (false, false, true, true, false, false, true, false, true)
                }
            }
        };
    PersistenceUatCase::new(
        entrypoint,
        stage,
        outcome,
        ROOT,
        STORE,
        1,
        if success { 1 } else { 0 },
        success.then(|| RECEIPT.to_owned()),
        backup,
        quarantine,
        auth,
        parity,
        old_root,
        migration,
        journal,
        legal_hold,
        deletion,
        unknown,
        false,
    )
    .unwrap()
}

fn matrix() -> PersistenceUatMatrix {
    let mut cases = Vec::new();
    for entrypoint in [
        UatEntrypoint::Cli,
        UatEntrypoint::Web,
        UatEntrypoint::Workbench,
    ] {
        for stage in [
            PersistenceUatStage::Backup,
            PersistenceUatStage::Restore,
            PersistenceUatStage::Upgrade,
            PersistenceUatStage::Restart,
            PersistenceUatStage::GovernanceDelete,
        ] {
            cases.push(case(entrypoint, stage, UatOutcome::Denied));
            cases.push(case(entrypoint, stage, UatOutcome::Succeeded));
        }
    }
    cases.push(case(
        UatEntrypoint::Workbench,
        PersistenceUatStage::Restart,
        UatOutcome::Replayed,
    ));
    cases.push(case(
        UatEntrypoint::Web,
        PersistenceUatStage::Restore,
        UatOutcome::ResultUnknown,
    ));
    PersistenceUatMatrix::new(ROOT, STORE, SPINE, cases).unwrap()
}

#[test]
fn persistence_matrix_covers_backup_restore_upgrade_restart_and_delete() {
    let matrix = matrix();
    assert_eq!(matrix.cases.len(), 32);
    assert!(matrix.validate().is_ok());
}

#[test]
fn restore_re_admission_and_governance_delete_are_fail_closed() {
    let restore = PersistenceUatCase::new(
        UatEntrypoint::Cli,
        PersistenceUatStage::Restore,
        UatOutcome::Succeeded,
        ROOT,
        STORE,
        1,
        1,
        Some(RECEIPT.to_owned()),
        true,
        true,
        false,
        true,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
    );
    assert_eq!(restore.unwrap_err(), "persistence_uat_restore_gate_failed");

    let delete = PersistenceUatCase::new(
        UatEntrypoint::Web,
        PersistenceUatStage::GovernanceDelete,
        UatOutcome::Succeeded,
        ROOT,
        STORE,
        1,
        1,
        Some(RECEIPT.to_owned()),
        false,
        false,
        true,
        true,
        false,
        false,
        true,
        true,
        true,
        false,
        false,
    );
    assert_eq!(delete.unwrap_err(), "persistence_uat_delete_gate_failed");
}

#[test]
fn unknown_restore_cannot_be_retried() {
    let unknown = PersistenceUatCase::new(
        UatEntrypoint::Workbench,
        PersistenceUatStage::Restore,
        UatOutcome::ResultUnknown,
        ROOT,
        STORE,
        1,
        0,
        None,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        true,
        true,
    );
    assert_eq!(
        unknown.unwrap_err(),
        "persistence_uat_unknown_retry_forbidden"
    );
}
