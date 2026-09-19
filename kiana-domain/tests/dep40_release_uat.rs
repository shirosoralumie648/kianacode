use kiana_domain::{
    ReleaseUatMatrix, UatCase, UatEffectStatus, UatEntrypoint, UatOutcome, UatProviderMode,
    UatScenario,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn case(entrypoint: UatEntrypoint, scenario: UatScenario, outcome: UatOutcome) -> UatCase {
    let (effect_status, receipt_digest, reconcile_required, retry_permitted) = match outcome {
        UatOutcome::Denied => (UatEffectStatus::None, None, false, false),
        UatOutcome::ResultUnknown => (UatEffectStatus::Unknown, None, true, false),
        UatOutcome::Succeeded | UatOutcome::RestartRecovered | UatOutcome::Replayed => (
            UatEffectStatus::Known,
            Some(DIGEST_A.to_owned()),
            false,
            false,
        ),
    };
    UatCase::new(
        entrypoint,
        scenario,
        outcome,
        UatProviderMode::Fake,
        false,
        effect_status,
        receipt_digest,
        reconcile_required,
        retry_permitted,
    )
    .unwrap()
}

fn full_matrix() -> ReleaseUatMatrix {
    let mut cases = Vec::new();
    for entrypoint in [
        UatEntrypoint::Cli,
        UatEntrypoint::Web,
        UatEntrypoint::Workbench,
        UatEntrypoint::Desktop,
    ] {
        for scenario in [
            UatScenario::Release,
            UatScenario::Upgrade,
            UatScenario::Rollback,
            UatScenario::Backup,
            UatScenario::Restore,
            UatScenario::Migration,
            UatScenario::Health,
        ] {
            cases.push(case(entrypoint, scenario, UatOutcome::Denied));
            cases.push(case(entrypoint, scenario, UatOutcome::Succeeded));
        }
    }
    cases.push(case(
        UatEntrypoint::Cli,
        UatScenario::Release,
        UatOutcome::RestartRecovered,
    ));
    cases.push(case(
        UatEntrypoint::Web,
        UatScenario::Upgrade,
        UatOutcome::Replayed,
    ));
    cases.push(case(
        UatEntrypoint::Workbench,
        UatScenario::Rollback,
        UatOutcome::ResultUnknown,
    ));
    ReleaseUatMatrix::new(DIGEST_A, DIGEST_B, DIGEST_C, UatProviderMode::Fake, cases).unwrap()
}

#[test]
fn matrix_covers_all_entries_and_release_lifecycle_scenarios() {
    let matrix = full_matrix();
    assert_eq!(matrix.cases.len(), 59);
    assert_eq!(matrix.provider_mode, UatProviderMode::Fake);
    assert!(matrix.validate().is_ok());
}

#[test]
fn unknown_effects_cannot_be_retried_and_live_opt_in_requires_approval() {
    let unknown = UatCase::new(
        UatEntrypoint::Cli,
        UatScenario::Rollback,
        UatOutcome::ResultUnknown,
        UatProviderMode::Fake,
        false,
        UatEffectStatus::Unknown,
        None,
        true,
        true,
    );
    assert_eq!(unknown.unwrap_err(), "uat_unknown_retry_forbidden");

    let live = UatCase::new(
        UatEntrypoint::Web,
        UatScenario::Health,
        UatOutcome::Succeeded,
        UatProviderMode::LiveOptIn,
        false,
        UatEffectStatus::Known,
        Some(DIGEST_D.to_owned()),
        false,
        false,
    );
    assert_eq!(live.unwrap_err(), "uat_live_provider_approval_missing");
}

#[test]
fn matrix_rejects_missing_success_coverage() {
    let mut cases = full_matrix().cases;
    cases.retain(|case| {
        !(case.entrypoint == UatEntrypoint::Desktop
            && case.scenario == UatScenario::Restore
            && case.outcome == UatOutcome::Succeeded)
    });
    assert_eq!(
        ReleaseUatMatrix::new(DIGEST_A, DIGEST_B, DIGEST_C, UatProviderMode::Fake, cases)
            .unwrap_err(),
        "uat_success_coverage_missing"
    );
}
