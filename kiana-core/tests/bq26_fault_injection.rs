//! BQ-26 failure-first fixtures: one named test per item in the card's rejected-first column.
//!
//! Card rejected-first column (all six must be refused):
//! `CAS race`, `partial frame`, `flush 失败`, `settlement 丢失`, `未知自动重试`,
//! `超额继续 dispatch`.
//!
//! Every test below drives a real seam. None of them asserts against a table that restates a
//! policy: they call the same functions production calls and compare against what those functions
//! actually returned. A test that cannot fail is not a test, so each one names the specific
//! wrong outcome it is guarding against.

mod support;

use kiana_core::{Bq26FaultCase, Bq26FaultClass, Bq26FaultHarnessRun};
use std::collections::{BTreeMap, BTreeSet};

fn source_ids() -> Vec<String> {
    vec![
        "sha256:".to_owned() + &"1".repeat(64),
        "sha256:".to_owned() + &"2".repeat(64),
    ]
}

/// The full report. Every one of the twelve cases is driven through a real seam: nine by the
/// library's own seam and the three adapter-backed ones by `support::AdapterSeam`, which calls
/// `MemoryEventLog::commit_transition` and `kiana_provider::replay_stream_fixture` for real.
fn run() -> Bq26FaultHarnessRun {
    support::bq26_adapter_seam::run_all(26, 2, source_ids())
}

/// CAS race — card item 1. The losing writer of a stale expected version must get a real
/// `CommitOutcome::Conflict` and must not leave a second fact on the stream.
#[test]
fn cas_race_second_writer_conflicts_and_writes_no_duplicate_fact() {
    let observation = run()
        .observation(Bq26FaultCase::CasRace)
        .expect("cas race case")
        .clone();
    assert_eq!(observation.family, Bq26FaultClass::Concurrency);
    assert!(
        observation.refusal.fired,
        "the race must actually reach the CAS seam"
    );
    assert!(
        !observation.refusal.returned_ok,
        "a stale writer that returned Ok is a lost update"
    );
    assert_eq!(observation.refusal.code, "event_store_transition_conflict");
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("CommitOutcome::Committed"),
        "the refusal must name the outcome the stale writer was forbidden to produce"
    );
    assert_eq!(observation.leaked_reservations, 0);
    assert!(observation.facts_replayable);
}

/// Partial frame — card item 2. A stream that stops mid-frame must be refused, never parsed.
#[test]
fn partial_frame_is_never_parsed_as_a_complete_reply() {
    let observation = run()
        .observation(Bq26FaultCase::PartialFrame)
        .expect("partial frame case")
        .clone();
    assert_eq!(observation.family, Bq26FaultClass::NetworkEof);
    assert_eq!(observation.refusal.code, "provider_frame_truncated");
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("ModelReply"),
        "naming the parsed reply as forbidden is what makes this a partial-parse test"
    );
    assert!(!observation.refusal.returned_ok);
    assert_eq!(observation.queued_entries, 0);
}

/// Network EOF distinct from a partial frame: the framer completes, the accumulator does not.
/// A clean frame boundary is not a complete answer, and it must not become one.
#[test]
fn network_eof_after_a_clean_frame_boundary_is_still_incomplete() {
    let observation = run()
        .observation(Bq26FaultCase::NetworkEof)
        .expect("network eof case")
        .clone();
    assert_eq!(observation.refusal.code, "provider_stream_incomplete");
    assert_ne!(
        observation.refusal.code, "provider_frame_truncated",
        "EOF on a frame boundary must be distinguished from a mid-frame truncation"
    );
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("ModelReply")
    );
}

/// Flush failure — card item 3. A dispatch fact with no prepared-flush acknowledgement is refused.
#[test]
fn flush_failure_refuses_dispatch_and_holds_the_reservation() {
    let observation = run()
        .observation(Bq26FaultCase::FlushFailure)
        .expect("flush failure case")
        .clone();
    assert_eq!(observation.family, Bq26FaultClass::Crash);
    assert_eq!(
        observation.refusal.code,
        "model_attempt_dispatch_requires_prepared_flush"
    );
    assert!(
        observation.reconciliation_required,
        "a dispatch that was never flushed must be reconcilable, not settled"
    );
    assert_eq!(
        observation.leaked_reservations, 1,
        "the reservation is deliberately held; a release here would be a double charge"
    );
}

/// Settlement loss — card item 4. An attempt whose settlement never lands stays held; the
/// fold must refuse to release it.
#[test]
fn settlement_loss_holds_the_reservation_and_refuses_release() {
    let observation = run()
        .observation(Bq26FaultCase::SettlementLoss)
        .expect("settlement loss case")
        .clone();
    assert_eq!(observation.refusal.code, "settlement_not_consumed");
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("usage.released"),
        "the release fold must be named as the forbidden value"
    );
    assert_eq!(observation.leaked_reservations, 1);
    assert!(observation.reconciliation_required);
    assert!(!observation.incident_action.trim().is_empty());
}

/// Unknown auto-retry — card item 5. A post-send `Unknown` side effect must be denied a retry.
/// This is the double-charge guard.
#[test]
fn unknown_auto_retry_is_denied_because_the_side_effect_is_unknown() {
    let observation = run()
        .observation(Bq26FaultCase::UnknownAutoRetry)
        .expect("unknown auto retry case")
        .clone();
    assert_eq!(observation.family, Bq26FaultClass::ProviderHttp);
    assert_eq!(observation.refusal.code, "side_effect_unknown");
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("retry"),
        "a granted retry is the exact double charge the card rejects"
    );
    assert_eq!(observation.leaked_reservations, 1);
    assert!(observation.reconciliation_required);
}

/// Over-budget dispatch — card item 6. Once the RPM window is exhausted, no outcome may be
/// dispatchable — not through `admit`, and not through the queue back door.
#[test]
fn over_budget_request_is_never_dispatched_and_the_queue_is_not_a_back_door() {
    let observation = run()
        .observation(Bq26FaultCase::OverBudgetDispatch)
        .expect("over budget dispatch case")
        .clone();
    assert_eq!(
        observation.refusal.code,
        "provider_capacity_quota_window_exhausted"
    );
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("ProviderCapacityOutcomeKind::Accept"),
        "an Accept after the budget is gone is the leak this case names"
    );
    assert!(!observation.refusal.returned_ok);
    assert_eq!(
        observation.leaked_leases, 0,
        "the lease must be released, not leaked"
    );
}

// ---------------------------------------------------------------------------
// The remaining six card families get their own named fixture.
// ---------------------------------------------------------------------------

#[test]
fn disk_full_is_classified_and_never_treated_as_transient() {
    let observation = run()
        .observation(Bq26FaultCase::DiskFull)
        .expect("disk full case")
        .clone();
    assert_eq!(observation.family, Bq26FaultClass::DiskFull);
    assert_eq!(observation.refusal.code, "platform_disk_full");
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("platform_transient_retry"),
        "retrying an ENOSPC write is the failure this case refuses"
    );
    assert!(observation.facts_replayable);
}

#[test]
fn provider_429_retries_only_because_it_was_refused_before_any_charge() {
    let observation = run()
        .observation(Bq26FaultCase::Provider429)
        .expect("provider 429 case")
        .clone();
    assert_eq!(
        observation.refusal.code, "retry_backoff_honors_retry_after",
        "429 is the one provider status that may produce a new bounded attempt"
    );
    assert_eq!(observation.leaked_reservations, 0);
    assert!(!observation.reconciliation_required);
}

#[test]
fn provider_5xx_after_send_is_never_auto_retried() {
    let observation = run()
        .observation(Bq26FaultCase::Provider5xx)
        .expect("provider 5xx case")
        .clone();
    assert_eq!(
        observation.refusal.code, "side_effect_unknown",
        "a 503 is checked against the side-effect fence before its retry class"
    );
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("retry")
    );
    assert!(
        !observation.incident_action.trim().is_empty(),
        "the card requires an incident to carry an action"
    );
}

#[test]
fn clock_rollback_cannot_build_a_window_or_extend_a_deadline() {
    let observation = run()
        .observation(Bq26FaultCase::ClockRollback)
        .expect("clock rollback case")
        .clone();
    assert_eq!(observation.family, Bq26FaultClass::ClockFault);
    assert_eq!(observation.refusal.code, "clock_untrusted");
    assert_eq!(
        observation.refusal.forbidden_value.as_deref(),
        Some("ClockTrust::Trusted"),
        "a rolled-back sample must never be reported as trusted"
    );
    assert!(observation.facts_replayable);
}

#[test]
fn leaked_capacity_lease_is_never_recycled_to_another_owner() {
    let observation = run()
        .observation(Bq26FaultCase::LeaseLeak)
        .expect("lease leak case")
        .clone();
    assert_eq!(observation.family, Bq26FaultClass::Concurrency);
    assert_eq!(
        observation.refusal.code,
        "provider_capacity_lease_owner_mismatch"
    );
    assert_eq!(observation.leaked_leases, 0);
    assert_eq!(observation.queued_entries, 0);
}

// ---------------------------------------------------------------------------
// Report integrity. A run that silently dropped a case would report success it did not earn.
// ---------------------------------------------------------------------------

#[test]
fn run_executes_every_case_in_the_matrix_exactly_once() {
    let report = run();
    let cases = report
        .observations
        .iter()
        .map(|observation| observation.case)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        cases.len(),
        report.observations.len(),
        "a case ran more than once"
    );
    for case in Bq26FaultCase::ALL {
        assert!(cases.contains(&case), "case never driven: {case:?}");
    }
    assert_eq!(report.observations.len(), 12);
}

#[test]
fn run_covers_all_six_card_families() {
    let report = run();
    assert_eq!(report.families_covered, Bq26FaultClass::ALL);
    for family in Bq26FaultClass::ALL {
        assert!(
            report
                .observations
                .iter()
                .any(|observation| observation.family == family),
            "family not exercised: {}",
            family.as_str()
        );
    }
}

#[test]
fn every_case_fired_its_fault_and_refused_it() {
    for observation in &run().observations {
        assert!(
            observation.refusal.fired,
            "fault never reached its seam: {}",
            observation.case.as_str()
        );
        assert!(
            !observation.refusal.returned_ok,
            "seam returned Ok under an injected fault: {}",
            observation.case.as_str()
        );
        assert!(
            !observation.incident_action.trim().is_empty(),
            "incident without an action: {}",
            observation.case.as_str()
        );
        assert!(
            observation.facts_replayable,
            "facts not replayable after the fault: {}",
            observation.case.as_str()
        );
    }
}

#[test]
fn report_states_what_it_does_not_prove() {
    let report = run();
    assert!(
        !report.unproven.is_empty(),
        "a fault report that claims no limits is a false claim"
    );
    let joined = report.unproven.join(" ");
    for claim in [
        "no process was killed",
        "no filesystem was filled",
        "no socket was opened",
        "no provider was called",
    ] {
        assert!(joined.contains(claim), "missing honesty clause: {claim}");
    }
    // The claims are sealed into the digest, so they cannot be edited away without re-sealing.
    let mut tampered = run();
    tampered.unproven.clear();
    assert!(
        tampered.validate().is_err(),
        "the unproven list must be bound to the report"
    );
}

#[test]
fn forged_digest_and_missing_case_are_both_refused() {
    let report = run();
    assert!(report.validate().is_ok());

    let mut forged = report.clone();
    forged.run_digest = "sha256:".to_owned() + &"f".repeat(64);
    assert_eq!(
        forged.validate().unwrap_err(),
        "bq26_fault_harness_digest_mismatch"
    );

    let mut truncated = report.clone();
    truncated.observations.pop();
    assert_eq!(
        truncated.validate().unwrap_err(),
        "bq26_fault_harness_header_invalid",
        "a report that dropped a case must not validate as full coverage"
    );

    let mut duplicated = report.clone();
    let first = duplicated.observations[0].clone();
    duplicated.observations[0] = first;
    assert!(
        duplicated.validate().is_err(),
        "a duplicated case must be refused"
    );
}

#[test]
fn observation_leak_and_marker_fields_cannot_be_forged() {
    let report = run();

    let mut leaked = report.clone();
    let index = leaked
        .observations
        .iter()
        .position(|observation| observation.case == Bq26FaultCase::CasRace)
        .expect("cas race case");
    leaked.observations[index].leaked_leases = 1;
    assert_eq!(
        leaked.validate().unwrap_err(),
        "bq26_fault_observation_digest_mismatch",
        "an edited observation must not pass the digest"
    );

    let mut cleared = report;
    let index = cleared
        .observations
        .iter()
        .position(|observation| observation.case == Bq26FaultCase::SettlementLoss)
        .expect("settlement loss case");
    cleared.observations[index].reconciliation_required = false;
    assert!(cleared.validate().is_err());
}

#[test]
fn run_is_bound_to_its_source_cursor_and_event_ids() {
    let report = run();
    assert!(report.validate_against(2, &source_ids()).is_ok());
    assert_eq!(
        report.validate_against(3, &source_ids()).unwrap_err(),
        "bq26_fault_harness_source_mismatch"
    );
    let other = vec!["sha256:".to_owned() + &"9".repeat(64)];
    assert!(report.validate_against(2, &other).is_err());
}

#[test]
fn report_is_deterministic_for_the_same_seed_and_source() {
    let first = run();
    let second = run();
    assert_eq!(first.run_digest, second.run_digest);
    assert_eq!(first.digest(), second.digest());
}

#[test]
fn invalid_headers_are_refused_before_any_fault_runs() {
    assert!(Bq26FaultHarnessRun::evaluate_with_default(0, 1, source_ids()).is_err());
    assert!(Bq26FaultHarnessRun::evaluate_with_default(1, 0, source_ids()).is_err());
    assert!(Bq26FaultHarnessRun::evaluate_with_default(1, 1, Vec::new()).is_err());
    let duplicate = vec![
        "sha256:".to_owned() + &"1".repeat(64),
        "sha256:".to_owned() + &"1".repeat(64),
    ];
    assert_eq!(
        Bq26FaultHarnessRun::evaluate_with_default(1, 1, duplicate).unwrap_err(),
        "bq26_fault_source_duplicate"
    );
}

#[test]
fn fault_classes_and_cases_map_to_the_card_vocabulary() {
    assert_eq!(Bq26FaultCase::CasRace.family(), Bq26FaultClass::Concurrency);
    assert_eq!(Bq26FaultCase::FlushFailure.family(), Bq26FaultClass::Crash);
    assert_eq!(Bq26FaultCase::DiskFull.family(), Bq26FaultClass::DiskFull);
    assert_eq!(
        Bq26FaultCase::PartialFrame.family(),
        Bq26FaultClass::NetworkEof
    );
    assert_eq!(
        Bq26FaultCase::Provider429.family(),
        Bq26FaultClass::ProviderHttp
    );
    assert_eq!(
        Bq26FaultCase::ClockRollback.family(),
        Bq26FaultClass::ClockFault
    );
    assert_eq!(Bq26FaultClass::ALL.len(), 6);
    assert_eq!(Bq26FaultCase::ALL.len(), 12);
    for class in Bq26FaultClass::ALL {
        assert!(!class.as_str().is_empty());
    }
}

#[test]
fn wire_decoding_revalidates_so_a_hand_edited_report_is_refused() {
    let report = run();
    let mut value = serde_json::to_value(&report).expect("report json");
    value["observations"][0]["leaked_leases"] = serde_json::json!(7);
    let decoded: Bq26FaultHarnessRun = serde_json::from_value(value).expect("decode report");
    assert!(
        decoded.validate().is_err(),
        "a report edited outside the constructor must not validate"
    );
}

#[test]
fn card_six_rejections_are_each_covered_by_a_refusal_code() {
    let report = run();
    let codes = report
        .observations
        .iter()
        .map(|observation| (observation.case, observation.refusal.code.clone()))
        .collect::<BTreeMap<_, _>>();
    for (case, expected) in [
        (Bq26FaultCase::CasRace, "event_store_transition_conflict"),
        (Bq26FaultCase::PartialFrame, "provider_frame_truncated"),
        (
            Bq26FaultCase::FlushFailure,
            "model_attempt_dispatch_requires_prepared_flush",
        ),
        (Bq26FaultCase::SettlementLoss, "settlement_not_consumed"),
        (Bq26FaultCase::UnknownAutoRetry, "side_effect_unknown"),
        (
            Bq26FaultCase::OverBudgetDispatch,
            "provider_capacity_quota_window_exhausted",
        ),
    ] {
        assert_eq!(
            codes.get(&case).map(String::as_str),
            Some(expected),
            "card rejection {case:?} is not covered"
        );
    }
}
