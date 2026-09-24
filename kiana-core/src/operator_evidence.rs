//! Read-only operator evidence assembled from committed facts.
//!
//! The projection deliberately keeps health, metric and trace evidence separate while binding
//! their digests to one EventLog cursor. It reports what was observed and what is missing; it
//! never interprets a metric as an external effect receipt or grants an admission decision.

use kiana_domain::{
    json_digest, validate_secret_free, EventStoreCapabilities, HealthProbeKind,
    OperatorEvidenceSnapshot, RuntimeEvent, SignalStatus,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

use super::{project_health_snapshot, project_operational_metrics, ControlPlane, CoreError};
use crate::metrics::{
    ARTIFACT_BYTES_TOTAL, EVENTLOG_APPEND_LATENCY, EVENTLOG_DURABLE_CURSOR, EVENTLOG_FLUSH_LATENCY,
    PROJECTOR_REBUILD_LATENCY, RECOVERY_ORPHAN_TOTAL, RECOVERY_UNKNOWN_TOTAL,
};

const MAX_LIMITATIONS: usize = kiana_domain::MAX_HEALTH_LIMITATIONS;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OperatorEvidenceError {
    #[error("operator_evidence_source_empty")]
    SourceEmpty,
    #[error("operator_evidence_metrics_invalid:{0}")]
    MetricsInvalid(String),
    #[error("operator_evidence_health_invalid:{0}")]
    HealthInvalid(String),
    #[error("telemetry_secret_scan_blocks_publish")]
    SecretScanBlocked,
    #[error("operator_evidence_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

fn metric_value(metrics: &kiana_domain::MetricSnapshot, name: &str) -> Option<u64> {
    metrics
        .points
        .iter()
        .find(|point| point.name == name)
        .and_then(|point| {
            point
                .value
                .is_finite()
                .then_some(point.value.max(0.0) as u64)
        })
}

fn event_value(event: &RuntimeEvent, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| event.data.get(*name).and_then(Value::as_u64))
}

fn max_event_value(events: &[RuntimeEvent], names: &[&str]) -> Option<u64> {
    events
        .iter()
        .filter_map(|event| event_value(event, names))
        .max()
}

fn add_limitation(limitations: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !value.is_empty()
        && limitations.len() < MAX_LIMITATIONS
        && !limitations.iter().any(|item| item == &value)
    {
        limitations.push(value);
    }
}

fn ref_value(event: &RuntimeEvent, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        event
            .data
            .get(*name)
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

fn stop_unconfirmed(events: &[RuntimeEvent]) -> u64 {
    events
        .iter()
        .filter(|event| {
            let kind = event.kind.to_ascii_lowercase();
            let requested = event
                .data
                .get("stop_requested")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || event
                    .data
                    .get("cancellation_requested")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                || kind.contains("cancel")
                || kind.contains("stop");
            requested
                && event.data.get("not_executed") != Some(&Value::Bool(true))
                && event.data.get("stop_confirmed") != Some(&Value::Bool(true))
        })
        .count() as u64
}

/// Build one bounded operator evidence snapshot from committed events.
pub fn project_operator_evidence(
    events: &[RuntimeEvent],
    capabilities: &EventStoreCapabilities,
    probe: HealthProbeKind,
    observed_at_ms: u64,
    queue_depth: Option<u64>,
) -> Result<OperatorEvidenceSnapshot, OperatorEvidenceError> {
    if events.is_empty() {
        return Err(OperatorEvidenceError::SourceEmpty);
    }
    for event in events {
        validate_secret_free(&event.data).map_err(|_| OperatorEvidenceError::SecretScanBlocked)?;
    }
    let metrics = project_operational_metrics(events, None)
        .map_err(|error| OperatorEvidenceError::MetricsInvalid(error.to_string()))?;
    let health = project_health_snapshot(events, capabilities, probe, observed_at_ms)
        .map_err(|error| OperatorEvidenceError::HealthInvalid(error.to_string()))?;

    let mut limitations = metrics.limitations.clone();
    for limitation in &health.limitations {
        add_limitation(&mut limitations, limitation.clone());
    }
    let source_cursor = metrics.source_cursor;
    let append_latency_ms = metric_value(&metrics, EVENTLOG_APPEND_LATENCY)
        .or_else(|| max_event_value(events, &["append_latency_ms", "commit_latency_ms"]));
    let flush_latency_ms = metric_value(&metrics, EVENTLOG_FLUSH_LATENCY)
        .or_else(|| max_event_value(events, &["flush_latency_ms"]));
    let projector_latency_ms = metric_value(&metrics, PROJECTOR_REBUILD_LATENCY).or_else(|| {
        max_event_value(
            events,
            &["projector_latency_ms", "projector_rebuild_latency_ms"],
        )
    });
    let recovery_latency_ms =
        max_event_value(events, &["recovery_latency_ms", "reconcile_latency_ms"]);
    let queue_depth = queue_depth.or_else(|| max_event_value(events, &["queue_depth"]));
    let unknown_count = metric_value(&metrics, RECOVERY_UNKNOWN_TOTAL).unwrap_or_default();
    let orphan_count = metric_value(&metrics, RECOVERY_ORPHAN_TOTAL).unwrap_or_default();
    let stop_unconfirmed_count = stop_unconfirmed(events);
    let artifact_bytes = metric_value(&metrics, ARTIFACT_BYTES_TOTAL).unwrap_or_default();
    let last_durable_cursor =
        metric_value(&metrics, EVENTLOG_DURABLE_CURSOR).unwrap_or(metrics.source_cursor);

    if append_latency_ms.is_none() {
        add_limitation(&mut limitations, "append_latency_unobserved");
    }
    if flush_latency_ms.is_none() {
        add_limitation(&mut limitations, "flush_latency_unobserved");
    }
    if projector_latency_ms.is_none() {
        add_limitation(&mut limitations, "projector_latency_unobserved");
    }
    if recovery_latency_ms.is_none() {
        add_limitation(&mut limitations, "recovery_latency_unobserved");
    }
    if queue_depth.is_none() {
        add_limitation(&mut limitations, "queue_depth_unobserved");
    }
    if metrics.projector_cursor < source_cursor {
        add_limitation(&mut limitations, "projector_lag_present");
    }
    if unknown_count > 0 || orphan_count > 0 || stop_unconfirmed_count > 0 {
        add_limitation(&mut limitations, "effect_success_not_observed");
    }
    if health.status == SignalStatus::Unknown {
        add_limitation(&mut limitations, "health_unknown_is_not_healthy");
    }

    let mut correlation_refs = BTreeSet::new();
    let mut causation_refs = BTreeSet::new();
    let mut trace_correlation_digests = BTreeSet::new();
    for event in events {
        if let Some(value) = ref_value(event, &["correlation_id", "correlation_ref"]) {
            correlation_refs.insert(value);
        }
        if let Some(value) = ref_value(event, &["causation_id", "causation_ref"]) {
            causation_refs.insert(value);
        }
        let trace_id = ref_value(event, &["trace_id"]);
        let span_id = ref_value(event, &["span_id", "parent_span_id"]);
        if trace_id.is_some() || span_id.is_some() {
            trace_correlation_digests.insert(json_digest(&json!({
                "trace_id": trace_id,
                "span_id": span_id,
                "source_cursor": event.data.get("source_cursor"),
            })));
        }
    }
    let status = if health.status != SignalStatus::Ok
        || metrics.status != SignalStatus::Ok
        || metrics.projector_cursor < source_cursor
        || unknown_count > 0
        || orphan_count > 0
        || stop_unconfirmed_count > 0
        || !limitations.is_empty()
    {
        SignalStatus::Degraded
    } else {
        SignalStatus::Ok
    };
    OperatorEvidenceSnapshot::new(
        status,
        health.status,
        source_cursor,
        metrics.source_event_ids.clone(),
        metrics.snapshot_digest.clone(),
        health.snapshot_digest.clone(),
        trace_correlation_digests
            .into_iter()
            .take(kiana_domain::MAX_OPERATOR_EVIDENCE_REFS)
            .collect(),
        append_latency_ms,
        flush_latency_ms,
        projector_latency_ms,
        recovery_latency_ms,
        queue_depth,
        last_durable_cursor,
        unknown_count,
        orphan_count,
        stop_unconfirmed_count,
        artifact_bytes,
        correlation_refs
            .into_iter()
            .take(kiana_domain::MAX_OPERATOR_EVIDENCE_REFS)
            .collect(),
        causation_refs
            .into_iter()
            .take(kiana_domain::MAX_OPERATOR_EVIDENCE_REFS)
            .collect(),
        limitations,
    )
    .map_err(OperatorEvidenceError::SnapshotInvalid)
}

impl ControlPlane {
    /// Read-only operator evidence; metrics and health are projections, not effect receipts.
    pub async fn operator_evidence(
        &self,
        probe: HealthProbeKind,
    ) -> Result<OperatorEvidenceSnapshot, CoreError> {
        self.operator_evidence_with_queue(probe, None).await
    }

    /// Daemon callers may bind an in-process queue depth to the same source snapshot.
    pub async fn operator_evidence_with_queue(
        &self,
        probe: HealthProbeKind,
        queue_depth: Option<u64>,
    ) -> Result<OperatorEvidenceSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("operator_evidence_read_all_unsupported".to_owned())
        })?;
        let capabilities = self.events.capabilities();
        let observed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| {
                kiana_ports::PortError::Failed("operator_evidence_clock_invalid".to_owned())
            })?
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        project_operator_evidence(&events, &capabilities, probe, observed_at, queue_depth)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
