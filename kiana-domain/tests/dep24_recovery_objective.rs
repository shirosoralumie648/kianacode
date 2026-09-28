//! DEP-24 recovery objective fixtures: deny-first, the success path last.
//!
//! Nothing in this file crashes a process, restores a root, takes a backup or reads a clock. The
//! measurements are supplied, and the only thing under test is whether the contract refuses to
//! call a bad rehearsal good.

use kiana_domain::{
    evaluate_cursor_monotonicity, evaluate_durability, evaluate_evidence_completeness,
    evaluate_fence_state, evaluate_rpo, evaluate_rto, FenceTokenId, RecoveryFaultScenario,
    RecoveryMeasurement, RecoveryObjective, RecoveryObjectiveStatus, RecoveryObservation,
    RecoveryRehearsalEvidence, RecoveryRehearsalLedger, RecoveryRehearsalReport, RequestId,
};
use uuid::Uuid;

const REHEARSAL_ID: &str = "rehearsal-2026q1-process-crash";
const ENVIRONMENT_DIGEST: &str =
    "sha256:0e1f2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7";
const OTHER_ENVIRONMENT_DIGEST: &str =
    "sha256:aa1f2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7";
const LIMITATIONS: &str = "no process was killed and no root was restored by this fixture";
const RPO_MS: u64 = 60_000;
const RTO_MS: u64 = 900_000;

fn objective() -> RecoveryObjective {
    RecoveryObjective::new(
        RequestId::from_uuid(Uuid::from_u128(0x0b1e_c0de_0000_4000_8000_0000_0000_0001)),
        "recovery-scope",
        "recovery-owner",
        RPO_MS,
        RTO_MS,
        1_700_000_000_000,
    )
    .expect("objective")
}

fn evidence() -> RecoveryRehearsalEvidence {
    RecoveryRehearsalEvidence::new(
        "clean",
        vec![
            "cargo".to_owned(),
            "rehearse".to_owned(),
            "--fault".to_owned(),
            "process_crash".to_owned(),
        ],
        Some(0),
        ENVIRONMENT_DIGEST,
        "fixture-process-crash-v1",
        LIMITATIONS,
    )
    .expect("evidence")
}

fn evidence_with(command_exit_code: Option<i32>, limitations: &str) -> RecoveryRehearsalEvidence {
    RecoveryRehearsalEvidence::new(
        "clean",
        vec!["cargo".to_owned(), "rehearse".to_owned()],
        command_exit_code,
        ENVIRONMENT_DIGEST,
        "fixture-process-crash-v1",
        limitations,
    )
    .expect("evidence")
}

fn fence(value: u128) -> FenceTokenId {
    FenceTokenId::from_uuid(Uuid::from_u128(value))
}

/// A rehearsal that lost nothing, took well under its budgets and left nothing stale behind.
fn good() -> RecoveryMeasurement {
    RecoveryMeasurement::new(
        REHEARSAL_ID,
        RecoveryFaultScenario::ProcessCrash,
        "process-crash-fixture",
        objective().objective_digest,
        evidence(),
        1_000,
        1_000,
        1_000,
        0,
        120_000,
        RecoveryObservation::Succeeded,
        true,
        fence(0x5eed),
        7,
        7,
        false,
        false,
        false,
    )
    .expect("measurement")
}

/// Re-seal a variant so the digest self-check passes and only the *rule* under test can fire.
fn resealed(mut measurement: RecoveryMeasurement) -> RecoveryMeasurement {
    measurement.measurement_digest = measurement.digest();
    measurement
        .validate()
        .expect("a resealed measurement is self-consistent");
    measurement
}

/// The reason a rehearsal was refused, with the report's own coherence re-checked on the way out.
fn reason_of(measurement: RecoveryMeasurement) -> String {
    let objective = objective();
    let report = RecoveryRehearsalReport::evaluate(&objective, &measurement).expect("report");
    assert!(
        !report.within_objective(),
        "a missed objective must never read as met"
    );
    assert_ne!(report.reason, "");
    assert_ne!(report.remediation, "none");
    report
        .validate_against(&objective, &measurement)
        .expect("the report re-derives from the same facts");
    report.reason
}

#[test]
fn a_declared_objective_must_carry_real_budgets() {
    // A zero RPO would make "met the objective" trivially true for any loss at all, and a zero
    // RTO would make it true only for a restore that takes no time. Both are declarations nobody
    // could keep, so neither may be written down.
    assert_eq!(
        RecoveryObjective::new(
            RequestId::new(),
            "recovery-scope",
            "recovery-owner",
            0,
            RTO_MS,
            1_700_000_000_000,
        )
        .unwrap_err(),
        "recovery_objective_rpo_invalid"
    );
    assert_eq!(
        RecoveryObjective::new(
            RequestId::new(),
            "recovery-scope",
            "recovery-owner",
            RPO_MS,
            0,
            1_700_000_000_000,
        )
        .unwrap_err(),
        "recovery_objective_rto_invalid"
    );
    assert_eq!(
        RecoveryObjective::new(
            RequestId::new(),
            "recovery-scope",
            "recovery-owner",
            RPO_MS,
            RTO_MS,
            0,
        )
        .unwrap_err(),
        "recovery_objective_header_invalid"
    );
}

#[test]
fn a_measurement_must_be_bound_to_the_objective_it_claims() {
    // A measurement taken against somebody else's declaration is not evidence about this one, even
    // though it is perfectly well formed.
    let foreign = resealed(RecoveryMeasurement {
        objective_digest: OTHER_ENVIRONMENT_DIGEST.to_owned(),
        ..good()
    });
    assert_eq!(
        RecoveryRehearsalReport::evaluate(&objective(), &foreign).unwrap_err(),
        "recovery_measurement_objective_binding_invalid"
    );
}

#[test]
fn a_restore_that_lost_more_history_than_the_rpo_allows_is_refused() {
    // One millisecond past the declared recovery point is still past it.
    let lost = resealed(RecoveryMeasurement {
        observed_data_loss_ms: RPO_MS + 1,
        ..good()
    });
    assert_eq!(
        evaluate_rpo(&objective(), &lost).unwrap_err(),
        "recovery_rpo_objective_exceeded"
    );
    assert_eq!(reason_of(lost), "recovery_rpo_objective_exceeded");
    // Exactly at the budget is inside it: the objective is a ceiling, not a target to beat.
    let at_budget = resealed(RecoveryMeasurement {
        observed_data_loss_ms: RPO_MS,
        ..good()
    });
    assert_eq!(evaluate_rpo(&objective(), &at_budget), Ok(()));
}

#[test]
fn a_restore_that_took_longer_than_the_rto_allows_is_refused() {
    let slow = resealed(RecoveryMeasurement {
        observed_recovery_ms: RTO_MS + 1,
        ..good()
    });
    assert_eq!(
        evaluate_rto(&objective(), &slow).unwrap_err(),
        "recovery_rto_objective_exceeded"
    );
    assert_eq!(reason_of(slow), "recovery_rto_objective_exceeded");
    let at_budget = resealed(RecoveryMeasurement {
        observed_recovery_ms: RTO_MS,
        ..good()
    });
    assert_eq!(evaluate_rto(&objective(), &at_budget), Ok(()));
}

#[test]
fn a_recovered_cursor_may_not_exceed_the_source() {
    // Losing history is what the RPO budgets. Inventing it is not a restore at all.
    let fabricated = resealed(RecoveryMeasurement {
        recovered_cursor: 1_001,
        ..good()
    });
    assert_eq!(
        evaluate_cursor_monotonicity(&fabricated).unwrap_err(),
        "recovery_cursor_ahead_of_source"
    );
    assert_eq!(reason_of(fabricated), "recovery_cursor_ahead_of_source");
}

#[test]
fn a_resume_behind_the_restored_root_is_a_regression() {
    // The quarantined root had already been verified up to 900. Resuming at 899 is a second loss
    // that no RPO budget covers, because the budget is about the source, not about this root.
    let regressed = resealed(RecoveryMeasurement {
        restored_root_cursor: 900,
        recovered_cursor: 899,
        ..good()
    });
    assert_eq!(
        evaluate_cursor_monotonicity(&regressed).unwrap_err(),
        "recovery_cursor_regressed"
    );
    assert_eq!(reason_of(regressed), "recovery_cursor_regressed");
}

#[test]
fn a_dispatch_under_a_stale_lease_outranks_every_later_rule() {
    // Everything else in this measurement is also wrong on purpose. The reported reason is still
    // the dispatch, because a command that already ran against the restored root is the harm,
    // and the other two violations are only descriptions of how it got there.
    let stale = resealed(RecoveryMeasurement {
        stale_dispatch_observed: true,
        old_lease_still_effective: true,
        old_fence_token_still_effective: true,
        observed_data_loss_ms: RPO_MS + 1,
        observed_recovery_ms: RTO_MS + 1,
        evidence: evidence_with(None, "none"),
        ..good()
    });
    assert_eq!(
        evaluate_fence_state(&stale).unwrap_err(),
        "recovery_stale_dispatch_after_restore"
    );
    assert_eq!(reason_of(stale), "recovery_stale_dispatch_after_restore");
}

#[test]
fn an_old_lease_or_fence_that_still_effects_is_refused() {
    let live_lease = resealed(RecoveryMeasurement {
        old_lease_still_effective: true,
        old_fence_token_still_effective: true,
        ..good()
    });
    assert_eq!(
        evaluate_fence_state(&live_lease).unwrap_err(),
        "recovery_stale_lease_still_effective"
    );
    assert_eq!(
        reason_of(live_lease),
        "recovery_stale_lease_still_effective"
    );

    // A live lease outranks a live fence token, because the lease is the writer.
    let live_fence = resealed(RecoveryMeasurement {
        old_fence_token_still_effective: true,
        ..good()
    });
    assert_eq!(
        evaluate_fence_state(&live_fence).unwrap_err(),
        "recovery_stale_fence_token_still_effective"
    );
    assert_eq!(
        reason_of(live_fence),
        "recovery_stale_fence_token_still_effective"
    );
}

#[test]
fn a_restart_is_not_a_durable_recovery() {
    // The process came back up and reported success without re-reading a single fact. That is a
    // restart, and a restart is not a restore.
    let restart = resealed(RecoveryMeasurement {
        durable_recovery: false,
        ..good()
    });
    assert_eq!(
        evaluate_durability(&restart).unwrap_err(),
        "recovery_restart_not_durable"
    );
    assert_eq!(reason_of(restart), "recovery_restart_not_durable");
}

#[test]
fn unknown_stays_unknown_and_never_meets_an_objective() {
    // Two different non-successes, and neither is quietly promoted. Unknown is reported as
    // "did not succeed" rather than as the more flattering "restart, not durable", because
    // deciding what actually happened is a reconciliation and not this slice's guess.
    for observation in [
        RecoveryObservation::ResultUnknown,
        RecoveryObservation::Denied,
    ] {
        let not_successful = resealed(RecoveryMeasurement {
            observation,
            durable_recovery: false,
            ..good()
        });
        assert_eq!(
            evaluate_durability(&not_successful).unwrap_err(),
            "recovery_rehearsal_outcome_not_succeeded"
        );
        assert_eq!(
            reason_of(not_successful),
            "recovery_rehearsal_outcome_not_succeeded"
        );
    }
    // And an unknown that also claims durability still does not become a success.
    let unknown_but_durable = resealed(RecoveryMeasurement {
        observation: RecoveryObservation::ResultUnknown,
        ..good()
    });
    assert_eq!(
        evaluate_durability(&unknown_but_durable).unwrap_err(),
        "recovery_rehearsal_outcome_not_succeeded"
    );
}

#[test]
fn a_rehearsal_without_an_exit_code_is_not_evidence() {
    // A run that never captured how it ended is a claim, not a measurement.
    let no_exit = resealed(RecoveryMeasurement {
        evidence: evidence_with(None, LIMITATIONS),
        ..good()
    });
    assert_eq!(
        evaluate_evidence_completeness(&no_exit).unwrap_err(),
        "recovery_evidence_exit_code_missing"
    );
    assert_eq!(reason_of(no_exit), "recovery_evidence_exit_code_missing");
}

#[test]
fn an_unstated_limitation_is_not_a_clean_result() {
    // The one field reserved for what the run did not prove is the one field a self-report is
    // most tempted to fill with a placeholder.
    for placeholder in ["none", "None", " n/a ", "TBD", "-", "unknown", ""] {
        let unstated = resealed(RecoveryMeasurement {
            evidence: evidence_with(Some(0), placeholder),
            ..good()
        });
        assert_eq!(
            evaluate_evidence_completeness(&unstated).unwrap_err(),
            "recovery_evidence_limitations_missing",
            "placeholder: {placeholder:?}"
        );
    }
    let stated = resealed(RecoveryMeasurement {
        evidence: evidence_with(Some(0), LIMITATIONS),
        ..good()
    });
    assert_eq!(evaluate_evidence_completeness(&stated), Ok(()));
}

#[test]
fn a_measurement_must_be_well_formed_before_it_is_judged() {
    // Nothing committed on the source, so there is no recovery point to have missed.
    assert_eq!(
        RecoveryMeasurement {
            source_cursor: 0,
            ..good()
        }
        .validate()
        .unwrap_err(),
        "recovery_measurement_cursor_invalid"
    );
    // `resealed` cannot be used here: it validates, and a shape-invalid variant is exactly what
    // this fixture is building. The digest is recomputed so the epoch rule is the one that fires.
    let mut epochless = RecoveryMeasurement {
        authority_epoch: 0,
        ..good()
    };
    epochless.measurement_digest = epochless.digest();
    assert_eq!(
        epochless.validate().unwrap_err(),
        "recovery_measurement_epoch_invalid"
    );
    // An empty argv is a command that was never run.
    assert_eq!(
        RecoveryRehearsalEvidence::new(
            "clean",
            Vec::new(),
            Some(0),
            ENVIRONMENT_DIGEST,
            "fixture-process-crash-v1",
            LIMITATIONS,
        )
        .unwrap_err(),
        "recovery_evidence_command_argv_invalid"
    );
    // A credential in the evidence block is refused rather than sealed into a rehearsal record.
    // `redact_text` runs before the sentinel scan, so an unredacted credential is reported as
    // unredacted rather than as a detected secret.
    assert_eq!(
        RecoveryRehearsalEvidence::new(
            "clean",
            vec!["cargo".to_owned(), "token=abc".to_owned()],
            Some(0),
            ENVIRONMENT_DIGEST,
            "fixture-process-crash-v1",
            LIMITATIONS,
        )
        .unwrap_err(),
        "recovery_evidence_command_argv_not_redacted"
    );
    // A path is not an evidence reference.
    assert_eq!(
        RecoveryRehearsalEvidence::new(
            "clean",
            vec!["cargo".to_owned()],
            Some(0),
            ENVIRONMENT_DIGEST,
            "file://backup/cassette",
            LIMITATIONS,
        )
        .unwrap_err(),
        "recovery_evidence_fixture_cassette_invalid"
    );
}

#[test]
fn a_report_cannot_be_rewritten_into_success() {
    // Re-sealing the digest around an edit does not help: the decision is re-derived.
    let mut restated = RecoveryRehearsalReport::evaluate(&objective(), &good()).expect("report");
    restated.measured_recovery_ms = 1;
    restated.report_digest = restated.digest();
    assert_eq!(
        restated
            .validate_against(&objective(), &good())
            .unwrap_err(),
        "recovery_rehearsal_report_binding_invalid"
    );

    // And an edit that does not even re-seal is caught one layer earlier.
    let mut tampered = RecoveryRehearsalReport::evaluate(&objective(), &good()).expect("report");
    tampered.measured_recovery_ms = 1;
    assert_eq!(
        tampered
            .validate_against(&objective(), &good())
            .unwrap_err(),
        "recovery_rehearsal_report_digest_mismatch"
    );
}

#[test]
fn a_report_cannot_present_itself_as_success_while_naming_a_reason() {
    // A within-objective decision carries no reason; a missed one always names the rule it hit.
    let mut relabelled = RecoveryRehearsalReport::evaluate(&objective(), &good()).expect("report");
    relabelled.status = RecoveryObjectiveStatus::ObjectiveMissed;
    assert_eq!(
        relabelled
            .validate_against(&objective(), &good())
            .unwrap_err(),
        "recovery_rehearsal_report_reason_incoherent"
    );

    let missed = resealed(RecoveryMeasurement {
        observed_data_loss_ms: RPO_MS + 1,
        ..good()
    });
    let mut cleared =
        RecoveryRehearsalReport::evaluate(&objective(), &missed).expect("missed report");
    cleared.reason = String::new();
    assert_eq!(
        cleared.validate_against(&objective(), &missed).unwrap_err(),
        "recovery_rehearsal_report_reason_incoherent"
    );
}

#[test]
fn a_periodic_rehearsal_cannot_be_rewritten_under_one_identity() {
    let missed = resealed(RecoveryMeasurement {
        observed_data_loss_ms: RPO_MS + 1,
        ..good()
    });
    let good_report = RecoveryRehearsalReport::evaluate(&objective(), &good()).expect("report");
    let missed_report = RecoveryRehearsalReport::evaluate(&objective(), &missed).expect("report");

    let ledger = RecoveryRehearsalLedger::new(Vec::new()).expect("empty ledger");
    let recorded = ledger.record(&good_report).expect("record");
    assert_eq!(recorded.reports.len(), 1);
    assert_eq!(
        recorded.latest().map(|report| report.rehearsal_id.as_str()),
        Some(REHEARSAL_ID)
    );

    // Replaying the identical decision is a no-op, so a retry cannot double-count a rehearsal.
    let replayed = recorded.record(&good_report).expect("idempotent replay");
    assert_eq!(replayed, recorded);
    assert_eq!(replayed.reports.len(), 1);

    // A different decision under the same identity is a contradiction, not an update.
    assert_eq!(
        recorded.record(&missed_report).unwrap_err(),
        "recovery_rehearsal_identity_digest_conflict"
    );
    assert_eq!(
        RecoveryRehearsalLedger::new(vec![good_report.clone(), good_report]).unwrap_err(),
        "recovery_rehearsal_ledger_duplicate_rehearsal"
    );
}

#[test]
fn every_named_fault_is_measured_by_the_same_rules() {
    let scenarios = [
        (RecoveryFaultScenario::ProcessCrash, "process_crash"),
        (RecoveryFaultScenario::BackupPartial, "backup_partial"),
        (
            RecoveryFaultScenario::RestoreInterrupted,
            "restore_interrupted",
        ),
        (RecoveryFaultScenario::CorruptFrame, "corrupt_frame"),
        (RecoveryFaultScenario::DiskFull, "disk_full"),
        (RecoveryFaultScenario::ClockRegression, "clock_regression"),
        (
            RecoveryFaultScenario::PostRestoreDispatch,
            "post_restore_dispatch",
        ),
    ];
    for (scenario, name) in scenarios {
        assert_eq!(scenario.as_str(), name);
        // The objective is a property of the scope, not of the fault, so the same clean run meets
        // it under every named scenario.
        let measured = resealed(RecoveryMeasurement { scenario, ..good() });
        let report = RecoveryRehearsalReport::evaluate(&objective(), &measured).expect("report");
        assert!(
            report.within_objective(),
            "{name} should meet the objective"
        );
        assert_eq!(report.reason, "", "{name}");

        // And a dispatch under a stale lease is refused identically under every one of them.
        let stale = resealed(RecoveryMeasurement {
            scenario,
            stale_dispatch_observed: true,
            ..good()
        });
        assert_eq!(
            reason_of(stale),
            "recovery_stale_dispatch_after_restore",
            "{name}"
        );
    }
}

#[test]
fn a_rehearsal_inside_its_declared_objective_is_within_objective() {
    let objective = objective();
    let measurement = good();
    let report = RecoveryRehearsalReport::evaluate(&objective, &measurement).expect("report");

    assert_eq!(report.rehearsal_id, REHEARSAL_ID);
    assert_eq!(report.status, RecoveryObjectiveStatus::WithinObjective);
    assert_eq!(report.observation, RecoveryObservation::Succeeded);
    assert!(report.durable_recovery);
    assert!(report.within_objective());
    assert_eq!(report.reason, "");
    assert_eq!(report.remediation, "none");
    assert_eq!(report.measured_data_loss_ms, 0);
    assert_eq!(report.allowed_data_loss_ms, RPO_MS);
    assert_eq!(report.measured_recovery_ms, 120_000);
    assert_eq!(report.allowed_recovery_ms, RTO_MS);
    assert_eq!(report.source_cursor, 1_000);
    assert_eq!(report.recovered_cursor, 1_000);
    assert_eq!(report.objective_digest, objective.objective_digest);
    assert_eq!(report.measurement_digest, measurement.measurement_digest);
    report
        .validate_against(&objective, &measurement)
        .expect("the report re-derives from the same facts");

    // Every standalone judgment agrees with the sealed decision, so the facade and the functions
    // behind it cannot drift apart.
    assert_eq!(evaluate_rpo(&objective, &measurement), Ok(()));
    assert_eq!(evaluate_rto(&objective, &measurement), Ok(()));
    assert_eq!(evaluate_cursor_monotonicity(&measurement), Ok(()));
    assert_eq!(evaluate_fence_state(&measurement), Ok(()));
    assert_eq!(evaluate_evidence_completeness(&measurement), Ok(()));
    assert_eq!(evaluate_durability(&measurement), Ok(()));

    // And it records into a periodic ledger exactly once.
    let ledger = RecoveryRehearsalLedger::new(Vec::new())
        .expect("empty ledger")
        .record(&report)
        .expect("record");
    assert_eq!(ledger.latest(), Some(&report));
}
