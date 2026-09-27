//! BQ-26 source guard.
//!
//! The card's core instruction is "a fault that is merely DOCUMENTED is not tested". That is
//! checkable from source, so this guard asserts two things a review cannot check by reading a
//! table:
//!
//! 1. The harness actually *calls* the seams it claims to drive. A harness that only named
//!    `MemoryEventLog::commit_transition` would pass a prose review and prove nothing, so each
//!    marker below is a call site, not a doc comment. The library cases live in
//!    `src/bq26_fault_harness.rs`; the three adapter-backed cases live in
//!    `tests/support/bq26_adapter_seam.rs` because `kiana-eventlog` and `kiana-provider` are
//!    test dependencies, and both files are checked.
//! 2. The harness did not grow into a second execution loop or a second vocabulary. It may not
//!    spawn a runtime, sleep, open a socket, write a file, or define a classifier of its own.

const HARNESS: &str = include_str!("../src/bq26_fault_harness.rs");
const ADAPTER: &str = include_str!("support/bq26_adapter_seam.rs");
const PLATFORM: &str = include_str!("../src/platform.rs");

#[test]
fn bq26_harness_drives_the_named_seams_instead_of_describing_them() {
    // Library-seam call sites: these must live in the library module.
    for marker in [
        // flush failure
        "model_attempt_dispatch_requires_prepared_flush",
        // settlement loss
        "ledger.release_unused(attempt_id, source)",
        "settlement_not_consumed",
        // retry classification
        "policy.classify(1, 1, bq26_fixture_now(), &observation)?",
        "RetryDenyReason::SideEffectUnknown",
        // disk full
        "crate::classify_failure_summary(",
        "FailureClass::DiskFull",
        // capacity / over budget / lease
        "controller.admit(request(500)?)?",
        "controller.dispatch_next(bq26_fixture_now())?",
        "provider_capacity_lease_owner_mismatch",
        "provider_capacity_lease_unknown",
        // clock fault
        "ClockTrust::Rollback",
        "QuotaWindow::from_clock(&rolled, 60_000)",
    ] {
        assert!(
            HARNESS.contains(marker),
            "BQ-26 seam marker missing from the library harness (it describes instead of drives): {marker}"
        );
    }
    // Adapter-seam call sites: the CAS race and the provider framer.
    for marker in [
        "MemoryEventLog::new()",
        ".commit_transition(first_batch)",
        "CommitOutcome::Conflict",
        "bq26_cas_race_duplicate_fact",
        "replay_stream_fixture(prepared, &chunks, 4_096)",
        "provider_frame_truncated",
        "provider_stream_incomplete",
        "ModelSideEffectState::Unknown",
    ] {
        assert!(
            ADAPTER.contains(marker),
            "BQ-26 adapter seam marker missing (it describes instead of drives): {marker}"
        );
    }
}

#[test]
fn bq26_adapter_cases_are_not_faked_in_the_library_harness() {
    // The library refuses the three adapter cases rather than faking them, so a report can never
    // claim to have driven an adapter seam it could not reach.
    assert!(HARNESS.contains("bq26_case_requires_adapter_seam"));
    assert!(HARNESS.contains(
        "Bq26FaultCase::CasRace | Bq26FaultCase::PartialFrame | Bq26FaultCase::NetworkEof =>"
    ));
    // The adapter seam delegates the other nine back to the library rather than reimplementing.
    assert!(ADAPTER.contains("kiana_core::bq26_inject_library_case(case)"));
    // And the harness does not pull the adapters in as library dependencies.
    assert!(!HARNESS.contains("kiana_eventlog::"));
    assert!(!HARNESS.contains("kiana_provider::"));
}

#[test]
fn bq26_harness_is_deterministic_and_never_sleeps_on_wall_time() {
    for (name, source) in [("harness", HARNESS), ("adapter seam", ADAPTER)] {
        for forbidden in [
            "std::thread::sleep",
            "tokio::time::sleep",
            "SystemTime::now()",
            "Instant::now()",
            "thread::sleep",
        ] {
            assert!(
                !source.contains(forbidden),
                "BQ-26 {name} must inject a clock, not sample one: {forbidden}"
            );
        }
    }
    // Every time the harness uses comes from the injected fixture, never a literal read.
    assert!(HARNESS.contains("pub fn bq26_fixture_now() -> u64"));
    assert!(HARNESS.contains("pub const BQ26_FAKE_NOW_UNIX_MS: u64"));
    assert!(HARNESS.contains("ClockObservation::observe("));
    assert!(ADAPTER.contains("bq26_fixture_deadline()"));
}

#[test]
fn bq26_harness_is_not_a_second_execution_loop() {
    for forbidden in [
        "tokio::spawn",
        "std::process::Command",
        "libc::kill",
        "std::fs::write",
        "std::fs::File",
        "ProviderGateway",
        "ControlPlane::new",
        "CapabilityBroker",
    ] {
        assert!(
            !HARNESS.contains(forbidden) && !ADAPTER.contains(forbidden),
            "BQ-26 widened past its boundary: {forbidden}"
        );
    }
    // The adapter seam opens no socket and contacts no provider: it drives the offline framer.
    for forbidden in ["reqwest::", "TcpStream", "Command::new", "kill("] {
        assert!(
            !ADAPTER.contains(forbidden),
            "BQ-26 adapter seam widened past its boundary: {forbidden}"
        );
    }
}

#[test]
fn bq26_harness_does_not_invent_a_second_failure_classifier() {
    // There is exactly one disk-full classifier, and both the production incident arm and the
    // fixture route through it.
    assert!(HARNESS.contains("crate::classify_failure_summary("));
    assert!(PLATFORM.contains("pub fn classify_failure_summary("));
    assert!(PLATFORM.contains("let class = classify_failure_summary("));
    let occurrences = PLATFORM.matches("FailureClass::DiskFull").count();
    assert_eq!(
        occurrences, 1,
        "the disk-full arm must exist exactly once, not be duplicated per caller"
    );
    assert!(
        !HARNESS.contains("fn classify_disk_full"),
        "BQ-26 must not define a private copy of the failure classifier"
    );
    assert!(
        !ADAPTER.contains("classify_disk_full"),
        "BQ-26 must not define a private copy of the failure classifier"
    );
}

#[test]
fn bq26_harness_reuses_existing_vocabularies_instead_of_new_ones() {
    for marker in [
        // BQ-17 retry vocabulary
        "RetryPolicy",
        "RetryObservation",
        "RetryDenyReason",
        "ModelRetryClass",
        "ModelSideEffectState",
        // BQ-12 settlement vocabulary
        "SettlementFoldLedger",
        "SettlementFoldEventKind",
        "SettlementFoldState",
        "QuotaReservation",
        "QuotaReservationId",
        // BQ-16 capacity vocabulary
        "ProviderCapacityController",
        "ProviderCapacityOutcomeKind",
        "ProviderCapacityRequest",
        // journal CAS vocabulary
        "AggregateVersion",
        "TransitionBatch",
        "CommitOutcome",
        // clock vocabulary
        "ClockObservation",
        "ClockTrust",
    ] {
        assert!(
            HARNESS.contains(marker) || ADAPTER.contains(marker),
            "BQ-26 must reuse the existing vocabulary, missing: {marker}"
        );
    }
    // No second settlement ledger, no second capacity controller, no second retry class.
    for forbidden in [
        "struct Bq26Settlement",
        "enum Bq26RetryClass",
        "struct Bq26Capacity",
        "struct Bq26QuotaReservation",
    ] {
        assert!(
            !HARNESS.contains(forbidden) && !ADAPTER.contains(forbidden),
            "BQ-26 invented a parallel vocabulary: {forbidden}"
        );
    }
}

#[test]
fn bq26_report_cannot_claim_coverage_it_did_not_run() {
    // `evaluate_with_seam` iterates the full constant, so a caller cannot assemble a partial
    // report, and the seal refuses anything that is not the full matrix.
    assert!(HARNESS.contains("pub fn evaluate_with_seam("));
    assert!(HARNESS.contains("pub fn evaluate_with_default("));
    assert!(HARNESS.contains("observations.len() != BQ26_FAULT_MAX_CASES"));
    assert!(HARNESS.contains("Bq26FaultCase::ALL"));
    assert!(HARNESS.contains("bq26_fault_case_missing"));
    assert!(HARNESS.contains("bq26_fault_family_incomplete"));
    assert!(HARNESS.contains("bq26_fault_case_duplicate"));
    // The negative assertion is the point: a fired flag, not a defaulted one.
    assert!(HARNESS.contains("if !self.fired {"));
    assert!(HARNESS.contains("bq26_fault_did_not_fire"));
    assert!(HARNESS.contains("returned_ok"));
    assert!(HARNESS.contains("forbidden_value"));
    // The honesty clause is bound into the digest.
    assert!(HARNESS.contains("unproven: Vec<String>"));
    assert!(HARNESS.contains("fn unproven_claims()"));
    assert!(HARNESS.contains("bq26_fault_harness_unproven_mismatch"));
}

#[test]
fn bq26_registration_lines_are_present() {
    let core = include_str!("../src/lib.rs");
    assert!(core.contains("mod bq26_fault_harness;"));
    assert!(core.contains("pub use bq26_fault_harness::{"));
    assert!(core.contains("BQ26_FAULT_HARNESS_SCHEMA"));
    assert!(core.contains("pub use platform::classify_failure_summary;"));
}

#[test]
fn bq26_does_not_shadow_the_er31_and_notification_fault_contracts() {
    // The pre-existing replay-only matrices stay where they are; BQ-26 adds a driving harness and
    // does not restate or replace their schemas.
    for forbidden in [
        "kiana.er31-fault-matrix",
        "kiana.notification-fault-matrix",
        "kiana.fault-matrix",
        "Er31FaultMatrix",
        "NotificationFaultMatrix",
    ] {
        assert!(
            !HARNESS.contains(forbidden) && !ADAPTER.contains(forbidden),
            "BQ-26 must not restate an existing fault vocabulary: {forbidden}"
        );
    }
    assert!(HARNESS.contains("kiana.bq26-fault-harness-run.v1"));
    assert!(HARNESS.contains("kiana.bq26-fault-cases.v1"));
}
