//! PD-34 failure-first capacity, throughput, latency and degradation budget fixtures.
//!
//! One test per "先拒绝的" (rejected-first) column of the card:
//!
//! * **无界 frame/batch/queue** — an unbounded frame, batch, page or queue,
//! * **长 reader 阻塞 writer** — a long reader blocking the writer,
//! * **maintenance 挤占用户命令** — maintenance squeezing out user commands,
//!
//! plus the delivery column: 给出上限、P95/P99、背压和降级证明；超过容量进入可诊断拒绝 —
//! bounds, P95/P99 declarations, backpressure and a diagnosable refusal above capacity.
//!
//! ## Nothing here was measured
//!
//! **No pressure run, stress test, throughput measurement, artifact staging, index rebuild,
//! backup or prune was performed.** Every number in this file is a *declared* budget: a cap taken
//! from the existing `CapacityEnvelope`, paired with a recorded measurement method. The fixtures
//! assert the **disposition** — bounded vs unbounded, declared vs measured, accepted vs refused —
//! and deliberately assert *no* latency value, because asserting an invented latency number is
//! exactly how a capacity model becomes fiction. Where the DEP-17 percentile types are exercised,
//! they are exercised on a declared budget structure, not on a recorded run.

use std::collections::BTreeSet;

use kiana_domain::{
    BenchmarkOperation, BenchmarkSummary, CapacityEnvelope, PersistenceCapacityBudget,
    StorageErrorClass,
};
use kiana_ports::{
    BudgetMeasurement, BudgetOrigin, CapacityBackpressure, CapacityRefusalObservation,
    DeclaredBudget, StorageBudgetOutcome, StorageCapacitySubject, StorageDegradationReport,
    StorageDegradationStatus, MAX_MAINTENANCE_SHARE_BPS,
};

/// A sealed baseline digest standing in for a real one; this slice sealed no artifact.
fn baseline() -> String {
    format!("sha256:{}", "5".repeat(64))
}

fn measurement_receipt() -> String {
    format!("sha256:{}", "6".repeat(64))
}

/// The DEP-17 envelope. These are the configured caps the workspace already publishes, not
/// measurements; every budget below is declared against them rather than against new numbers.
fn envelope() -> CapacityEnvelope {
    CapacityEnvelope {
        journal_max_bytes: 64 * 1024 * 1024,
        journal_max_events: 1_000_000,
        max_frame_bytes: 1_048_576,
        max_event_bytes: 524_288,
        max_batch_events: 256,
        max_page_events: 1_000,
        max_export_records: 10_000,
        observability_queue_capacity: 4_096,
        max_artifact_bytes: 8_388_608,
        high_cardinality_rejected: true,
        oversize_rejected: true,
        backpressure_preserves_facts: true,
    }
}

/// A DEP-17 latency budget. Declared, so the p95/p99 numbers are caps somebody chose.
fn latency_budget(operation: BenchmarkOperation) -> PersistenceCapacityBudget {
    PersistenceCapacityBudget {
        operation,
        max_p95_micros: 2_000,
        max_p99_micros: 5_000,
        max_queue_depth: 256,
        max_rejection_rate_bps: 100,
        max_maintenance_share_bps: 2_500,
    }
}

fn budget(
    subject: StorageCapacitySubject,
    origin: BudgetOrigin,
    maintenance_share_bps: u16,
    receipt: Option<String>,
) -> DeclaredBudget {
    DeclaredBudget::new(
        subject,
        &envelope(),
        origin,
        Some(latency_budget(BenchmarkOperation::Append)),
        maintenance_share_bps,
        CapacityBackpressure::BoundedReject,
        receipt,
    )
    .expect("the declared budget is well formed")
}

/// The four subjects the card names, plus the writer queue PD-27 already bounds.
fn all_budgets() -> Vec<DeclaredBudget> {
    vec![
        budget(
            StorageCapacitySubject::EventFrame,
            BudgetOrigin::CapacityEnvelope,
            0,
            None,
        ),
        budget(
            StorageCapacitySubject::Artifact,
            BudgetOrigin::CapacityEnvelope,
            0,
            None,
        ),
        budget(
            StorageCapacitySubject::Index,
            BudgetOrigin::CapacityEnvelope,
            0,
            None,
        ),
        budget(
            StorageCapacitySubject::BackupPrune,
            BudgetOrigin::PersistenceBudget,
            2_500,
            None,
        ),
        budget(
            StorageCapacitySubject::WriterQueue,
            BudgetOrigin::WriterQueuePolicy,
            0,
            None,
        ),
    ]
}

// ---------------------------------------------------------------------------------------
// 成功：给出上限、P95/P99、背压和降级证明
// ---------------------------------------------------------------------------------------

#[test]
fn every_subject_has_a_declared_bound_a_measurement_method_and_a_backpressure() {
    let report = StorageDegradationReport::evaluate(
        baseline(),
        all_budgets(),
        Vec::new(),
        Vec::new(),
        1_000,
    )
    .expect("the report is well formed");
    report.validate().expect("the report validates");
    assert_eq!(report.status, StorageDegradationStatus::Refused);
    assert!(report.budgets_in_force(), "every subject is bounded");
    assert!(report.reason.is_empty());
    assert!(report.over_budget.is_empty());

    // The four subjects the card names are all present, and the bounds come from the existing
    // envelope rather than a second set of numbers.
    let subjects: BTreeSet<StorageCapacitySubject> =
        report.budgets.iter().map(|budget| budget.subject).collect();
    for subject in [
        StorageCapacitySubject::EventFrame,
        StorageCapacitySubject::Artifact,
        StorageCapacitySubject::Index,
        StorageCapacitySubject::BackupPrune,
    ] {
        assert!(
            subjects.contains(&subject),
            "PD-34 must budget {}",
            subject.as_str()
        );
    }
    for declared in &report.budgets {
        assert!(declared.envelope_bound > 0);
        assert_eq!(
            declared.envelope_bound,
            declared.subject.envelope_bound(&envelope()),
            "a budget's bound must be read from the envelope, not restated"
        );
        // The measurement method is recorded whether or not a run ever happened.
        assert_eq!(
            declared.measurement,
            BudgetMeasurement::for_subject(declared.subject)
        );
        // A declared budget says nothing about latency until a receipt exists.
        assert!(!declared.is_measured());
        assert_eq!(declared.outcome(), StorageBudgetOutcome::Bounded);
    }
    // And the report says plainly that nothing was measured.
    assert!(!report.anything_measured());
    assert!(report.measured_subjects.is_empty());
    assert!(report.summaries.is_empty());
}

#[test]
fn a_diagnosable_refusal_is_preserved_as_evidence_not_as_a_drop() {
    // 超过容量进入可诊断拒绝: the refusal carries a stable code and a capacity class, and the
    // committed facts are retained. This is a shape assertion over declared observations — no
    // request was presented and none was rejected.
    let observation = CapacityRefusalObservation::new(
        StorageCapacitySubject::EventFrame,
        StorageErrorClass::Unavailable,
        "eventlog_disk_limit",
        12,
        500,
        false,
        true,
    )
    .expect("the observation is well formed");
    observation.validate().expect("the observation validates");
    assert_eq!(observation.rejected, 12);
    assert!(observation.presented >= observation.rejected);
    assert!(observation.committed_facts_retained);
    assert!(!observation.refusal_code.is_empty());

    let report = StorageDegradationReport::evaluate(
        baseline(),
        all_budgets(),
        vec![observation],
        Vec::new(),
        1_000,
    )
    .expect("the report is well formed");
    report.validate().expect("a refusal is not a failure");
    assert!(report.budgets_in_force());
}

#[test]
fn a_measured_subject_requires_a_receipt_on_its_own_budget() {
    // The positive path a future pressure run will take: a budget carrying a receipt is reported
    // as measured, and the subject list is derived from the receipts rather than declared.
    let mut budgets = all_budgets();
    budgets[0] = budget(
        StorageCapacitySubject::EventFrame,
        BudgetOrigin::CapacityEnvelope,
        0,
        Some(measurement_receipt()),
    );
    let report = StorageDegradationReport::evaluate(baseline(), budgets, Vec::new(), Vec::new(), 0)
        .expect("the report is well formed");
    assert_eq!(
        report.measured_subjects,
        vec![StorageCapacitySubject::EventFrame]
    );
    assert!(report.anything_measured());
    // The subject is still `Measured`, not `OverBudget` — the receipt is evidence, not a breach.
    assert_eq!(report.budgets[0].outcome(), StorageBudgetOutcome::Measured);
}

// ---------------------------------------------------------------------------------------
// 先拒绝：无界 frame/batch/queue
// ---------------------------------------------------------------------------------------

#[test]
fn an_unbounded_subject_is_refused_rather_than_reported_ready() {
    // The card's first rejection. A frame, batch, page or queue with no declared cap is not
    // "fast": it is unbounded, and unbounded under load is how a store stops exposing new facts.
    let mut budgets = all_budgets();
    budgets[0] = DeclaredBudget::new(
        StorageCapacitySubject::EventFrame,
        &envelope(),
        BudgetOrigin::CapacityEnvelope,
        None,
        0,
        CapacityBackpressure::BoundedReject,
        None,
    )
    .expect("a declared budget is well formed");
    // Strip the bound the way a caller that never configured one would.
    budgets[0].envelope_bound = 0;
    let report = StorageDegradationReport::evaluate(baseline(), budgets, Vec::new(), Vec::new(), 0);
    assert_eq!(
        report.unwrap_err(),
        "capacity_budget_envelope_bound_mismatch",
        "a budget whose bound does not match the envelope is refused at construction"
    );

    // And when the envelope itself carries no bound, the subject is named as unbounded.
    let mut unbounded_envelope = envelope();
    unbounded_envelope.max_frame_bytes = 0;
    let unbounded = DeclaredBudget::new(
        StorageCapacitySubject::EventFrame,
        &unbounded_envelope,
        BudgetOrigin::CapacityEnvelope,
        None,
        0,
        CapacityBackpressure::BoundedReject,
        None,
    );
    // `CapacityEnvelope::validate` refuses a zero bound first, which is the same refusal one step
    // earlier: no subject is left unbounded by an envelope that validates.
    assert_eq!(unbounded.unwrap_err(), "performance_capacity_invalid");
}

#[test]
fn a_duplicate_budget_for_one_subject_is_refused() {
    let mut budgets = all_budgets();
    let duplicate = budgets[0].clone();
    budgets.push(duplicate);
    let report = StorageDegradationReport::evaluate(baseline(), budgets, Vec::new(), Vec::new(), 0)
        .expect("the report is well formed");
    assert_eq!(report.status, StorageDegradationStatus::Exceeded);
    assert_eq!(report.reason, "capacity_budget_duplicate");
    assert_eq!(report.over_budget, vec![StorageCapacitySubject::EventFrame]);
    assert!(!report.budgets_in_force());
}

#[test]
fn a_budget_with_the_wrong_envelope_field_is_refused() {
    // A budget that points at a field it does not read is a second number for the same subject,
    // which is the drift the card rejects.
    let mut value = budget(
        StorageCapacitySubject::EventFrame,
        BudgetOrigin::CapacityEnvelope,
        0,
        None,
    );
    value.envelope_field = "journal_max_bytes".to_owned();
    assert_eq!(
        value.validate_against(&envelope()).unwrap_err(),
        "capacity_budget_envelope_field_mismatch"
    );
}

#[test]
fn a_budget_measured_without_a_receipt_is_refused() {
    // The claim and the evidence are separate fields; a subject may only be reported as measured
    // when the receipt it is derived from is present.
    let value = budget(
        StorageCapacitySubject::EventFrame,
        BudgetOrigin::CapacityEnvelope,
        0,
        None,
    );
    assert!(!value.is_measured());
    assert_eq!(value.outcome(), StorageBudgetOutcome::Bounded);
    // A budget that claims a receipt it did not get is caught by the digest seal.
    let mut tampered = value.clone();
    tampered.measurement_receipt_digest = Some(measurement_receipt());
    assert_eq!(
        tampered.validate().unwrap_err(),
        "capacity_budget_digest_mismatch",
        "a receipt added after sealing is refused rather than adopted"
    );
}

// ---------------------------------------------------------------------------------------
// 先拒绝：长 reader 阻塞 writer
// ---------------------------------------------------------------------------------------

#[test]
fn a_long_reader_holding_the_writer_is_refused() {
    let observation = CapacityRefusalObservation::new(
        StorageCapacitySubject::Index,
        StorageErrorClass::Conflict,
        "projection_reader_holds_writer",
        3,
        3,
        true,
        true,
    )
    .expect("the observation is well formed");
    let report = StorageDegradationReport::evaluate(
        baseline(),
        all_budgets(),
        vec![observation],
        Vec::new(),
        0,
    )
    .expect("the report is well formed");
    assert_eq!(report.status, StorageDegradationStatus::Exceeded);
    assert_eq!(report.reason, "capacity_reader_blocked_writer");
    assert_eq!(report.over_budget, vec![StorageCapacitySubject::Index]);
    assert!(!report.budgets_in_force());
}

// ---------------------------------------------------------------------------------------
// 先拒绝：maintenance 挤占用户命令
// ---------------------------------------------------------------------------------------

#[test]
fn maintenance_above_its_share_is_refused() {
    // backup/prune may take at most its declared share of runtime; above that, user admission is no
    // longer observable, which is the card's third rejection.
    let report = StorageDegradationReport::evaluate(
        baseline(),
        all_budgets(),
        Vec::new(),
        Vec::new(),
        4_000,
    )
    .expect("the report is well formed");
    assert_eq!(report.status, StorageDegradationStatus::Exceeded);
    assert_eq!(report.reason, "capacity_maintenance_budget_exceeded");
    assert_eq!(
        report.over_budget,
        vec![StorageCapacitySubject::BackupPrune]
    );
    // The remediation names the action rather than a number.
    assert!(report.remediation.contains("defer maintenance"));
}

#[test]
fn a_maintenance_share_above_one_hundred_percent_is_refused() {
    let report = StorageDegradationReport::evaluate(
        baseline(),
        all_budgets(),
        Vec::new(),
        Vec::new(),
        MAX_MAINTENANCE_SHARE_BPS + 1,
    );
    assert_eq!(
        report.unwrap_err(),
        "storage_degradation_maintenance_share_invalid"
    );
}

// ---------------------------------------------------------------------------------------
// 先拒绝：拒绝不可诊断 / 事实被丢弃
// ---------------------------------------------------------------------------------------

#[test]
fn a_refusal_with_no_code_is_not_a_diagnosable_refusal() {
    // An operator who cannot tell "capacity" from "corrupt" from "locked" cannot act on it, so a
    // rejection without a stable code is refused rather than recorded as a clean rejection.
    let anonymous = CapacityRefusalObservation::new(
        StorageCapacitySubject::EventFrame,
        StorageErrorClass::Unavailable,
        "",
        5,
        5,
        false,
        true,
    );
    assert_eq!(
        anonymous.unwrap_err(),
        "capacity_refusal_observation_invalid"
    );
    // A capacity refusal may not be filed under the PD-30 reconciling classes either.
    let unknown_class = CapacityRefusalObservation::new(
        StorageCapacitySubject::EventFrame,
        StorageErrorClass::ResultUnknown,
        "eventlog_disk_limit",
        5,
        5,
        false,
        true,
    );
    assert_eq!(
        unknown_class.unwrap_err(),
        "capacity_refusal_observation_invalid",
        "a capacity refusal filed as a fault outcome is refused"
    );
}

#[test]
fn a_refusal_that_lost_a_committed_fact_is_refused() {
    // Bounded backpressure may refuse new work; it may not lose a fact it already accepted. That
    // is the difference between a degradation and a loss, and only one of them is allowed.
    let lossy = CapacityRefusalObservation::new(
        StorageCapacitySubject::WriterQueue,
        StorageErrorClass::Unavailable,
        "eventlog_worker_queue_full",
        9,
        9,
        false,
        false,
    );
    assert_eq!(lossy.unwrap_err(), "capacity_backpressure_dropped_facts");
    // Every backpressure strategy declares that it preserves accepted facts.
    for strategy in [
        CapacityBackpressure::BoundedReject,
        CapacityBackpressure::ShedNewest,
        CapacityBackpressure::DrainToCap,
    ] {
        assert!(strategy.preserves_accepted_facts());
    }
}

#[test]
fn a_refusal_for_an_undeclared_subject_is_refused() {
    // Reporting a refusal for a subject with no budget is a claim about capacity nobody bounded.
    let budgets = vec![budget(
        StorageCapacitySubject::EventFrame,
        BudgetOrigin::CapacityEnvelope,
        0,
        None,
    )];
    let observation = CapacityRefusalObservation::new(
        StorageCapacitySubject::Index,
        StorageErrorClass::Unavailable,
        "projection_page_limit",
        1,
        1,
        false,
        true,
    )
    .expect("the observation is well formed");
    let report =
        StorageDegradationReport::evaluate(baseline(), budgets, vec![observation], Vec::new(), 0)
            .expect("the report is well formed");
    assert_eq!(report.reason, "capacity_observation_subject_undeclared");
}

// ---------------------------------------------------------------------------------------
// 篡改 / 未接线的检查
// ---------------------------------------------------------------------------------------

#[test]
fn a_tampered_report_is_refused() {
    let mut report =
        StorageDegradationReport::evaluate(baseline(), all_budgets(), Vec::new(), Vec::new(), 0)
            .expect("the report is well formed");
    report.status = StorageDegradationStatus::Exceeded;
    assert_eq!(
        report.validate().unwrap_err(),
        "storage_degradation_report_binding_invalid",
        "an edited status is refused, not adopted"
    );
    let mut relabelled =
        StorageDegradationReport::evaluate(baseline(), all_budgets(), Vec::new(), Vec::new(), 0)
            .expect("the report is well formed");
    relabelled.measured_subjects = vec![StorageCapacitySubject::Artifact];
    assert_eq!(
        relabelled.validate().unwrap_err(),
        "capacity_budget_receipt_missing",
        "a subject may not be reported as measured without a receipt on its own budget"
    );
}

#[test]
fn a_malformed_benchmark_summary_is_refused_rather_than_dropped() {
    // A summary is the DEP-17 measurement record. Dropping a malformed one would make a report
    // look measured with fewer samples than it claims.
    let broken = BenchmarkSummary::new(BenchmarkOperation::Append, 10, 5, 20, 2, 1_024, 1)
        .expect("the summary is well formed")
        .clone();
    let mut tampered = broken;
    tampered.p95_micros = 1;
    tampered.summary_digest = String::new();
    let report = StorageDegradationReport::evaluate(
        baseline(),
        all_budgets(),
        Vec::new(),
        vec![tampered],
        0,
    );
    assert!(
        report.is_err(),
        "a summary whose digest no longer matches must not be silently ignored"
    );
}

#[test]
fn the_budgets_reuse_the_existing_capacity_vocabulary_rather_than_a_second_model() {
    // Every subject names the DEP-17 envelope field that bounds it, and a subject with no field
    // could not be constructed at all.
    for subject in StorageCapacitySubject::ALL {
        let field = subject.envelope_field();
        assert!(!field.is_empty());
        assert!(subject.is_bounded(&envelope()));
        assert_eq!(
            subject.envelope_bound(&envelope()),
            envelope_bound_for(field, &envelope())
        );
    }
    assert_eq!(StorageCapacitySubject::ALL.len(), 5);
    // The measurement method is fixed per subject, so two subjects cannot be measured by two
    // different means without the difference being visible in the report.
    let methods: BTreeSet<&str> = StorageCapacitySubject::ALL
        .into_iter()
        .map(|subject| BudgetMeasurement::for_subject(subject).as_str())
        .collect();
    assert_eq!(methods.len(), StorageCapacitySubject::ALL.len());
}

fn envelope_bound_for(field: &str, envelope: &CapacityEnvelope) -> u64 {
    match field {
        "max_frame_bytes" => envelope.max_frame_bytes,
        "max_artifact_bytes" => envelope.max_artifact_bytes,
        "max_page_events" => envelope.max_page_events,
        "journal_max_bytes" => envelope.journal_max_bytes,
        "max_batch_events" => envelope.max_batch_events,
        other => panic!("PD-34 must not introduce a new envelope field: {other}"),
    }
}
