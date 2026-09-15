use kiana_domain::{
    AuditActionKind, AuditDecision, AuditRecord, DataClass, EventId, MetricKind, MetricPoint,
    MetricSource, ObservabilityRecord, SignalKind, SignalStatus, TraceStatus, TraceSummary,
};
use kiana_ports::{
    require_observability_capabilities, AuditQueryPort, AuditQueryRequest, HealthProbePort,
    JsonlObservabilitySink, MemoryObservabilitySink, MetricSink, ObservabilityPort,
    ObservabilityRequirements, ObservabilitySignalRecord, TraceSink,
};
use std::collections::BTreeMap;

fn event_id() -> EventId {
    EventId::new()
}

fn log_record(cursor: u64) -> ObservabilityRecord {
    ObservabilityRecord::new(
        format!("log-{cursor}"),
        SignalKind::Log,
        SignalStatus::Ok,
        "oa05",
        cursor,
        vec![event_id()],
        BTreeMap::new(),
    )
    .unwrap()
}

fn audit_record(cursor: u64) -> AuditRecord {
    AuditRecord::new(
        format!("audit-{cursor}"),
        AuditActionKind::Authorization,
        AuditDecision::Accepted,
        "server:control-plane",
        "run",
        format!("run-{cursor}"),
        cursor,
        vec![event_id()],
        1,
        1,
        DataClass::Internal,
        "audit",
    )
    .unwrap()
}

#[tokio::test]
async fn memory_sink_records_signals_and_fails_closed_on_capacity_failure_and_cancel() {
    let sink = MemoryObservabilitySink::new(8);
    let mut mismatched = log_record(1);
    mismatched.signal = SignalKind::Audit;
    assert_eq!(
        ObservabilitySignalRecord::Log(mismatched)
            .validate()
            .unwrap_err(),
        "observability_signal_mismatch"
    );
    let log = sink
        .append(ObservabilitySignalRecord::Log(log_record(1)))
        .await
        .unwrap();
    assert_eq!(log.signal, SignalKind::Log);

    let trace = TraceSummary::new(
        "trace-oa05",
        None,
        TraceStatus::Ok,
        1,
        1,
        2,
        vec![event_id()],
    )
    .unwrap();
    TraceSink::record_trace(&sink, trace).await.unwrap();
    let metric = MetricPoint::new(
        "kiana.test.accepted_total",
        MetricKind::Counter,
        1.0,
        "count",
        BTreeMap::new(),
        MetricSource::EventReducer,
        3,
        vec![event_id()],
    )
    .unwrap();
    MetricSink::record_metric(&sink, metric).await.unwrap();
    sink.append(ObservabilitySignalRecord::Audit(audit_record(4)))
        .await
        .unwrap();
    assert_eq!(sink.records().len(), 4);

    sink.fail_next("injected_export_failure");
    assert!(sink
        .append(ObservabilitySignalRecord::Log(log_record(5)))
        .await
        .unwrap_err()
        .to_string()
        .contains("injected_export_failure"));
    sink.cancel().await.unwrap();
    assert!(sink
        .append(ObservabilitySignalRecord::Log(log_record(5)))
        .await
        .unwrap_err()
        .to_string()
        .contains("observability_cancelled"));
    let (_, cancellation) = tokio::sync::watch::channel(true);
    assert!(sink
        .append_cancellable(ObservabilitySignalRecord::Log(log_record(5)), cancellation,)
        .await
        .unwrap_err()
        .to_string()
        .contains("observability_cancelled"));
}

#[tokio::test]
async fn sink_capabilities_flush_ack_and_audit_query_are_explicit() {
    let sink = MemoryObservabilitySink::new(2);
    require_observability_capabilities(
        ObservabilityPort::capabilities(&sink),
        ObservabilityRequirements {
            durable: true,
            flush: true,
            cancellation: true,
            min_records: 1,
        },
    )
    .unwrap_err();
    sink.append(ObservabilitySignalRecord::Audit(audit_record(1)))
        .await
        .unwrap();
    sink.append(ObservabilitySignalRecord::Log(log_record(2)))
        .await
        .unwrap();
    assert!(sink
        .append(ObservabilitySignalRecord::Log(log_record(3)))
        .await
        .unwrap_err()
        .to_string()
        .contains("observability_capacity_exceeded"));
    let ack = sink.flush().await.unwrap();
    assert_eq!(ack.flushed_records, 2);
    assert_eq!(ack.flush_sequence, 1);
    let page = sink
        .query(AuditQueryRequest {
            source_cursor: 2,
            after_cursor: 0,
            limit: 10,
            action_kind: Some(AuditActionKind::Authorization),
            decision: Some(AuditDecision::Accepted),
            actor_ref: Some("server:control-plane".to_owned()),
            target_kind: Some("run".to_owned()),
        })
        .await
        .unwrap();
    assert_eq!(page.records.len(), 1);
    page.validate().unwrap();
}

#[tokio::test]
async fn health_probe_never_infers_a_snapshot_from_empty_or_unsupported_state() {
    let sink = MemoryObservabilitySink::new(2);
    assert!(sink.probe().await.is_err());
    let snapshot = kiana_domain::HealthSnapshot::new(
        "eventlog",
        SignalStatus::Degraded,
        7,
        vec![event_id()],
        1,
    )
    .unwrap();
    sink.set_health(snapshot.clone()).unwrap();
    assert_eq!(sink.probe().await.unwrap(), snapshot);
    assert_eq!(
        require_observability_capabilities(
            kiana_ports::ObservabilityCapabilities::default(),
            ObservabilityRequirements::none(),
        ),
        Ok(())
    );
}

#[tokio::test]
async fn jsonl_fake_keeps_one_validated_signal_per_line_without_claiming_durability() {
    let sink = JsonlObservabilitySink::new(4);
    assert!(!ObservabilityPort::capabilities(&sink).durable);
    sink.append(ObservabilitySignalRecord::Audit(audit_record(1)))
        .await
        .unwrap();
    sink.append(ObservabilitySignalRecord::Log(log_record(2)))
        .await
        .unwrap();
    assert_eq!(sink.lines().len(), 2);
    for line in sink.lines() {
        let decoded: ObservabilitySignalRecord = serde_json::from_str(&line).unwrap();
        decoded.validate().unwrap();
    }
    let page = sink
        .query(AuditQueryRequest {
            source_cursor: 2,
            after_cursor: 0,
            limit: 4,
            action_kind: None,
            decision: None,
            actor_ref: None,
            target_kind: None,
        })
        .await
        .unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(sink.flush().await.unwrap().flushed_records, 2);
}
