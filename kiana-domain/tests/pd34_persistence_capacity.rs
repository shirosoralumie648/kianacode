use kiana_domain::{
    BenchmarkOperation, BenchmarkSummary, CapacityEnvelope, PersistenceCapacityBudget,
    PersistenceCapacityReport, PersistenceCapacityStatus, PersistencePressureObservation,
};

const BASELINE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn capacity() -> CapacityEnvelope {
    CapacityEnvelope {
        journal_max_bytes: 1_048_576,
        journal_max_events: 4_096,
        max_frame_bytes: 64 * 1024,
        max_event_bytes: 32 * 1024,
        max_batch_events: 256,
        max_page_events: 256,
        max_export_records: 1_024,
        observability_queue_capacity: 1_024,
        max_artifact_bytes: 16 * 1024 * 1024,
        high_cardinality_rejected: true,
        oversize_rejected: true,
        backpressure_preserves_facts: true,
    }
}

fn budget(operation: BenchmarkOperation) -> PersistenceCapacityBudget {
    PersistenceCapacityBudget {
        operation,
        max_p95_micros: 25,
        max_p99_micros: 35,
        max_queue_depth: 8,
        max_rejection_rate_bps: 1_000,
        max_maintenance_share_bps: 2_000,
    }
}

fn observation(
    operation: BenchmarkOperation,
    p95_micros: u64,
    degraded: bool,
) -> PersistencePressureObservation {
    PersistencePressureObservation::new(
        operation,
        BenchmarkSummary::new(operation, 100, 10, p95_micros, p95_micros + 10, 4_096, 100).unwrap(),
        2,
        0,
        degraded,
        degraded.then(|| "index_rebuild_degraded".to_owned()),
        true,
        1_000,
    )
    .unwrap()
}

fn report(p95_micros: u64) -> PersistenceCapacityReport {
    let operations = [
        BenchmarkOperation::Append,
        BenchmarkOperation::Flush,
        BenchmarkOperation::Project,
        BenchmarkOperation::Rebuild,
        BenchmarkOperation::Query,
        BenchmarkOperation::Export,
    ];
    let budgets = operations.into_iter().map(budget).collect::<Vec<_>>();
    let observations = operations
        .into_iter()
        .map(|operation| {
            observation(
                operation,
                p95_micros,
                operation == BenchmarkOperation::Query,
            )
        })
        .collect::<Vec<_>>();
    PersistenceCapacityReport::evaluate(BASELINE, capacity(), budgets, observations).unwrap()
}

#[test]
fn bounded_capacity_report_keeps_degradation_visible() {
    let report = report(20);
    assert_eq!(report.status, PersistenceCapacityStatus::Ready);
    assert_eq!(report.degraded_operations, vec![BenchmarkOperation::Query]);
    assert!(report.validate().is_ok());
}

#[test]
fn latency_budget_and_fact_loss_block_or_reject() {
    let blocked = report(30);
    assert_eq!(
        blocked.reason,
        "persistence_capacity_latency_budget_exceeded"
    );
    assert_eq!(blocked.status, PersistenceCapacityStatus::Blocked);

    let facts_lost = PersistencePressureObservation::new(
        BenchmarkOperation::Append,
        BenchmarkSummary::new(BenchmarkOperation::Append, 10, 1, 2, 3, 128, 10).unwrap(),
        1,
        0,
        false,
        None,
        false,
        0,
    );
    assert_eq!(
        facts_lost.unwrap_err(),
        "persistence_capacity_observation_invalid"
    );
}

#[test]
fn missing_operation_observation_is_rejected() {
    let budgets = vec![budget(BenchmarkOperation::Append)];
    let observations = vec![observation(BenchmarkOperation::Flush, 20, false)];
    assert_eq!(
        PersistenceCapacityReport::evaluate(BASELINE, capacity(), budgets, observations)
            .unwrap_err(),
        "persistence_capacity_budget_missing"
    );
}
