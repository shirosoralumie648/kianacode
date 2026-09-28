//! SC-40 source guard: the envelope computes, it does not measure, and it reuses the BQ-26 fault
//! vocabulary instead of growing a second one.

#[test]
fn sc40_pins_every_bound_the_card_names() {
    let source = include_str!("../src/capacity_fault_envelope.rs");

    for marker in [
        "capacity_fault_queue_unbounded",
        "capacity_fault_output_flood",
        "capacity_fault_latency_unbounded",
        "capacity_fault_clock_rollback",
        "capacity_fault_clock_rollback_unproven",
        "capacity_fault_reservation_leak",
        "capacity_fault_provider_quota_exceeded",
        "capacity_fault_reservation_leak_after_recovery",
        "capacity_fault_backpressure_missing",
        "capacity_fault_recovery_not_established",
        "capacity_fault_budget_unbounded",
        "capacity_fault_budget_digest_mismatch",
        "capacity_fault_report_binding_invalid",
        "capacity_fault_report_digest_mismatch",
    ] {
        assert!(source.contains(marker), "SC-40 module lost {marker}");
    }
}

#[test]
fn sc40_reuses_the_bq26_fault_vocabulary() {
    let source = include_str!("../src/capacity_fault_envelope.rs");

    for marker in [
        "Bq26FaultCase",
        "Bq26FaultClass",
        "DiskFull",
        "ProviderHttp",
        "ClockFault",
        "CapacityBudget",
        "CapacityFaultSample",
        "CapacityFaultReport",
        "evaluate_capacity_fault",
        "budget_digest",
        "reservations_after_recovery",
    ] {
        assert!(source.contains(marker), "SC-40 module lost {marker}");
    }
    // It must not define a second fault taxonomy or a second budget of its own shape.
    for forbidden in [
        "pub enum Sc40Fault",
        "pub struct CapacityLimits",
        "pub enum CapacityBound",
    ] {
        assert!(
            !source.contains(forbidden),
            "SC-40 invented a parallel vocabulary: {forbidden}"
        );
    }
}

#[test]
fn sc40_computes_and_does_not_measure() {
    let source = include_str!("../src/capacity_fault_envelope.rs");

    for forbidden in [
        "std::fs",
        "File::",
        "Command::",
        "std::process",
        "TcpStream",
        "reqwest",
        "tokio",
        "spawn",
        "thread::sleep",
        "SystemTime",
        "Instant::now",
        "EventStore",
        "append_event",
    ] {
        assert!(
            !source.contains(forbidden),
            "SC-40 module gained a measurement or effect token: {forbidden}"
        );
    }
}
