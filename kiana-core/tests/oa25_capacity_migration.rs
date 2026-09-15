use kiana_core::{build_performance_baseline, percentile_micros, summarize_benchmark};
use kiana_domain::{
    json_digest, BenchmarkOperation, CapacityEnvelope, JournalFrame, JournalFramePayload,
    MigrationObservation, MigrationOperation, MigrationStatus, RequestId, RuntimeEvent,
    JOURNAL_WRITER_VERSION, MAX_JOURNAL_EVENTS, MAX_JOURNAL_EVENT_BYTES, MAX_JOURNAL_FRAME_BYTES,
    MAX_JOURNAL_LOG_BYTES, MAX_JOURNAL_PAGE_EVENTS, MAX_TRANSITION_EVENTS,
};
use serde_json::json;

fn capacity() -> CapacityEnvelope {
    CapacityEnvelope {
        journal_max_bytes: MAX_JOURNAL_LOG_BYTES,
        journal_max_events: MAX_JOURNAL_EVENTS as u64,
        max_frame_bytes: MAX_JOURNAL_FRAME_BYTES as u64,
        max_event_bytes: MAX_JOURNAL_EVENT_BYTES as u64,
        max_batch_events: MAX_TRANSITION_EVENTS as u64,
        max_page_events: MAX_JOURNAL_PAGE_EVENTS as u64,
        max_export_records: 1_000,
        observability_queue_capacity: 1_024,
        max_artifact_bytes: 16 * 1024 * 1024,
        high_cardinality_rejected: true,
        oversize_rejected: true,
        backpressure_preserves_facts: true,
    }
}

fn migration(
    operation: MigrationOperation,
    from_version: u32,
    to_version: u32,
    status: MigrationStatus,
    limitations: Vec<String>,
) -> MigrationObservation {
    MigrationObservation::new(
        operation,
        from_version,
        to_version,
        status,
        4,
        json_digest(&json!({"journal":"fixture","from":from_version,"to":to_version})),
        limitations,
    )
    .unwrap()
}

#[test]
fn all_benchmark_operations_have_bounded_percentile_summaries() {
    let samples = [7, 2, 11, 5, 3, 19, 13];
    assert_eq!(percentile_micros(&samples, 50).unwrap(), 7);
    assert_eq!(percentile_micros(&samples, 95).unwrap(), 19);
    assert_eq!(percentile_micros(&samples, 99).unwrap(), 19);
    assert!(percentile_micros(&[], 50).is_err());
    assert!(percentile_micros(&samples, 90).is_err());

    let operations = [
        BenchmarkOperation::Append,
        BenchmarkOperation::Flush,
        BenchmarkOperation::Project,
        BenchmarkOperation::Rebuild,
        BenchmarkOperation::Query,
        BenchmarkOperation::Export,
    ];
    let summaries = operations
        .into_iter()
        .map(|operation| summarize_benchmark(operation, &samples, 4096, 128).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(summaries.len(), 6);
    for summary in summaries {
        assert!(summary.p50_micros <= summary.p95_micros);
        assert!(summary.p95_micros <= summary.p99_micros);
        summary.validate().unwrap();
    }
}

#[test]
fn baseline_binds_capacity_rotation_archive_and_version_migration_evidence() {
    let samples = [
        BenchmarkOperation::Append,
        BenchmarkOperation::Flush,
        BenchmarkOperation::Project,
        BenchmarkOperation::Rebuild,
        BenchmarkOperation::Query,
        BenchmarkOperation::Export,
    ]
    .into_iter()
    .map(|operation| summarize_benchmark(operation, &[1, 2, 4, 8], 8192, 256).unwrap())
    .collect::<Vec<_>>();
    let migrations = vec![
        migration(
            MigrationOperation::Rotation,
            2,
            2,
            MigrationStatus::ReadOnlyVerified,
            vec![],
        ),
        migration(
            MigrationOperation::Archive,
            2,
            2,
            MigrationStatus::ReadOnlyVerified,
            vec![],
        ),
        migration(
            MigrationOperation::Upgrade,
            1,
            2,
            MigrationStatus::ReadOnlyVerified,
            vec![],
        ),
        migration(
            MigrationOperation::Downgrade,
            2,
            1,
            MigrationStatus::Rejected,
            vec!["downgrade_write_forbidden".to_owned()],
        ),
    ];
    let baseline = build_performance_baseline(
        4,
        json_digest(&json!({"events":4,"projection":1})),
        samples,
        capacity(),
        migrations,
        vec!["remote_ci_timing".to_owned()],
    )
    .unwrap();
    baseline.validate().unwrap();
    let encoded = serde_json::to_value(&baseline).unwrap();
    assert_eq!(encoded["schema"], "kiana.performance-baseline.v1");
    assert!(
        serde_json::from_value::<kiana_domain::PerformanceBaseline>({
            let mut value = encoded.clone();
            value["unexpected"] = json!(true);
            value
        })
        .is_err()
    );
}

#[test]
fn unknown_writer_version_and_missing_safety_guards_fail_closed() {
    let event = RuntimeEvent::new(RequestId::new(), 1, "run.accepted", json!({})).unwrap();
    let mut frame = JournalFrame::new(JournalFramePayload::Event { event }).unwrap();
    frame.writer_version = JOURNAL_WRITER_VERSION + 1;
    assert_eq!(
        frame.validate().unwrap_err(),
        "journal_writer_version_unsupported"
    );

    let mut envelope = capacity();
    envelope.high_cardinality_rejected = false;
    assert_eq!(
        envelope.validate().unwrap_err(),
        "performance_safety_guard_missing"
    );
    let unknown = migration(
        MigrationOperation::Upgrade,
        2,
        99,
        MigrationStatus::Unknown,
        vec!["unknown_writer_version".to_owned()],
    );
    assert_eq!(unknown.status, MigrationStatus::Unknown);
    unknown.validate().unwrap();
}
