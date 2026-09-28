//! SC-40: the capacity and resource envelope a fault has to stay inside.
//!
//! Deny-first. Every bound is broken on its own so the refusal names the bound rather than
//! reporting "something was exceeded", and the leak-after-recovery case is tested against a sample
//! that is otherwise entirely inside budget.

use kiana_core::{
    evaluate_capacity_fault, Bq26FaultCase, Bq26FaultClass, CapacityBudget, CapacityFaultReport,
    CapacityFaultSample,
};

fn budget() -> CapacityBudget {
    CapacityBudget::new(5_000, 64, 1_048_576, true)
}

/// A sample that is inside every bound and has actually recovered.
fn healthy() -> CapacityFaultSample {
    CapacityFaultSample::new(
        Bq26FaultCase::CasRace,
        Bq26FaultClass::Concurrency,
        1_200,
        8,
        4_096,
        true,
        0,
        0,
        true,
        0,
        false,
    )
}

#[test]
fn an_unbounded_queue_is_refused() {
    let mut sample = healthy();
    sample.observed_queue_depth = 65;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_queue_unbounded"
    );
}

#[test]
fn a_flood_of_output_bytes_is_refused() {
    let mut sample = healthy();
    sample.observed_bytes = 1_048_577;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_output_flood"
    );
}

#[test]
fn unbounded_latency_is_refused() {
    let mut sample = healthy();
    sample.observed_latency_ms = 5_001;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_latency_unbounded"
    );
}

#[test]
fn a_system_that_absorbed_the_load_without_pushing_back_is_refused() {
    let mut sample = healthy();
    sample.backpressure_applied = false;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_backpressure_missing"
    );

    // And the disk-full case is refused even by a budget that did not ask for backpressure: a disk
    // that reported full and then "recovered" absorbed the load somewhere the sample cannot see.
    let mut relaxed = budget();
    relaxed.require_backpressure = false;
    relaxed.budget_digest = relaxed.digest();
    let mut disk = healthy();
    disk.family = Bq26FaultClass::DiskFull;
    disk.backpressure_applied = false;
    assert_eq!(
        evaluate_capacity_fault(&disk, &relaxed).unwrap_err(),
        "capacity_fault_backpressure_missing"
    );
}

#[test]
fn a_clock_rollback_is_refused_before_any_number_is_compared() {
    // Every other field in this sample is comfortably inside budget. It is still refused, because
    // a latency measured against a clock that went backwards is not evidence of a bound.
    let mut sample = healthy();
    sample.clock_rollback_detected = true;
    sample.observed_latency_ms = 1;
    sample.observed_queue_depth = 1;
    sample.observed_bytes = 1;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_clock_rollback"
    );
}

#[test]
fn a_provider_that_kept_spending_after_a_429_is_refused_under_its_own_reason() {
    let mut sample = healthy();
    sample.family = Bq26FaultClass::ProviderHttp;
    sample.leaked_reservations = 2;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_reservation_leak"
    );
    // The provider-specific code exists so the reason names the cause. It is reachable once the
    // generic leak check is satisfied, which is why the ordering in the module matters: the more
    // specific reason is checked first for this family.
    let mut spent = healthy();
    spent.family = Bq26FaultClass::ProviderHttp;
    spent.observed_queue_depth = 65;
    assert_eq!(
        evaluate_capacity_fault(&spent, &budget()).unwrap_err(),
        "capacity_fault_queue_unbounded"
    );
}

#[test]
fn a_clock_fault_that_did_not_move_the_clock_is_refused() {
    // Reporting a clock fault as though it had been observed, when the clock never moved, is the
    // same mistake as quoting a latency measured against a clock nobody trusts.
    let mut sample = healthy();
    sample.family = Bq26FaultClass::ClockFault;
    sample.clock_rollback_detected = false;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_clock_rollback_unproven"
    );
    // With the rollback actually observed, the same family is judged on its ordinary bounds.
    let mut observed = healthy();
    observed.family = Bq26FaultClass::ClockFault;
    assert_eq!(
        evaluate_capacity_fault(&observed, &budget()).unwrap_err(),
        "capacity_fault_clock_rollback"
    );
}

#[test]
fn a_leak_that_survives_recovery_is_refused_even_when_everything_else_is_inside_budget() {
    // This is the case the card names and the one a during-fault sample cannot see: the queue
    // drained, the latency came back, and the reservation is still held.
    let mut sample = healthy();
    sample.recovered = true;
    sample.reservations_after_recovery = 1;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_reservation_leak_after_recovery"
    );
}

#[test]
fn a_sample_that_never_established_recovery_cannot_claim_the_envelope() {
    let mut sample = healthy();
    sample.recovered = false;
    assert_eq!(
        evaluate_capacity_fault(&sample, &budget()).unwrap_err(),
        "capacity_fault_recovery_not_established"
    );
}

#[test]
fn a_budget_with_no_ceiling_is_refused_rather_than_read_as_unbounded() {
    let unbounded = CapacityBudget::new(0, 0, 0, false);
    assert_eq!(
        evaluate_capacity_fault(&healthy(), &unbounded).unwrap_err(),
        "capacity_fault_budget_unbounded"
    );
}

#[test]
fn a_report_cannot_be_edited_after_the_fact() {
    let sample = healthy();
    let budget = budget();
    let report: CapacityFaultReport = evaluate_capacity_fault(&sample, &budget).expect("report");
    report
        .validate_against(&sample, &budget)
        .expect("un edited");

    let mut shrunk = report.clone();
    shrunk.observed_queue_depth = 1;
    assert_eq!(
        shrunk.validate_against(&sample, &budget).unwrap_err(),
        "capacity_fault_report_binding_invalid"
    );

    let mut unsealed = report;
    unsealed.report_digest = format!("sha256:{}", "9".repeat(64));
    assert_eq!(
        unsealed.validate_against(&sample, &budget).unwrap_err(),
        "capacity_fault_report_digest_mismatch"
    );
}

#[test]
fn a_fault_inside_every_bound_produces_a_report_that_says_what_it_did_not_show() {
    let sample = healthy();
    let budget = budget();
    let report = evaluate_capacity_fault(&sample, &budget).expect("report");
    report
        .validate_against(&sample, &budget)
        .expect("re-derives");
    assert!(!report.limitations.is_empty());
    assert_eq!(report.budget_digest, budget.budget_digest);
    assert_eq!(report.reservations_after_recovery, 0);
    assert!(report.backpressure_applied);
}
