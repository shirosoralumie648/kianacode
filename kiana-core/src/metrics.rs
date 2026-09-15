//! Event-derived operational metrics.
//!
//! The reducer is deliberately read-only. It consumes the same committed EventLog facts as the
//! receipt, invocation and capability projections, and it reports gaps or unavailable evidence
//! as bounded limitations instead of manufacturing healthy zeroes.

use kiana_domain::{
    json_digest, EventCursor, EventId, MetricCatalog, MetricPoint, MetricSnapshot, RunId,
    RuntimeEvent, SignalStatus,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::{ControlPlane, CoreError};

const MAX_LIMITATIONS: usize = 16;
const MAX_SAMPLE_IDS: usize = 256;

pub const EVENTLOG_APPEND_LATENCY: &str = "kiana.eventlog.append_latency_ms";
pub const EVENTLOG_FLUSH_LATENCY: &str = "kiana.eventlog.flush_latency_ms";
pub const EVENTLOG_COMMIT_TOTAL: &str = "kiana.eventlog.commit_total";
pub const EVENTLOG_COMMIT_FAILURE_TOTAL: &str = "kiana.eventlog.commit_failure_total";
pub const EVENTLOG_DURABLE_CURSOR: &str = "kiana.eventlog.durable_cursor";
pub const PROJECTOR_CURSOR: &str = "kiana.projector.cursor";
pub const PROJECTOR_LAG_EVENTS: &str = "kiana.projector.lag_events";
pub const PROJECTOR_REBUILD_TOTAL: &str = "kiana.projector.rebuild_total";
pub const PROJECTOR_REBUILD_LATENCY: &str = "kiana.projector.rebuild_latency_ms";
pub const RECEIPT_QUERY_TOTAL: &str = "kiana.receipts.query_total";
pub const RECEIPT_QUERY_FAILURE_TOTAL: &str = "kiana.receipts.query_failure_total";
pub const RECEIPT_QUERY_LATENCY: &str = "kiana.receipts.query_latency_ms";
pub const ARTIFACT_BYTES_TOTAL: &str = "kiana.artifacts.bytes_total";
pub const ARTIFACT_READ_FAILURE_TOTAL: &str = "kiana.artifacts.read_failure_total";
pub const RECOVERY_ORPHAN_TOTAL: &str = "kiana.recovery.orphan_total";
pub const RECOVERY_UNKNOWN_TOTAL: &str = "kiana.recovery.unknown_total";
pub const RECOVERY_LAST_ERROR_PRESENT: &str = "kiana.recovery.last_error_present";

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MetricsProjectionError {
    #[error("metrics_source_empty")]
    SourceEmpty,
    #[error("metrics_source_cursor_invalid")]
    SourceCursorInvalid,
    #[error("metrics_source_cursor_gap")]
    SourceCursorGap,
    #[error("metrics_projector_cursor_invalid")]
    ProjectorCursorInvalid,
    #[error("metrics_duplicate_event_conflict")]
    DuplicateEventConflict,
    #[error("metrics_catalog_missing:{0}")]
    CatalogMissing(&'static str),
    #[error("metrics_point_invalid:{0}")]
    PointInvalid(String),
    #[error("metrics_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

#[derive(Clone, Copy)]
struct ObservedEvent<'a> {
    cursor: EventCursor,
    event: &'a RuntimeEvent,
}

fn source_cursor(
    event: &RuntimeEvent,
    index: usize,
) -> Result<EventCursor, MetricsProjectionError> {
    match event.data.get("source_cursor") {
        None => u64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(MetricsProjectionError::SourceCursorInvalid),
        Some(value) => value
            .as_u64()
            .filter(|cursor| *cursor > 0)
            .ok_or(MetricsProjectionError::SourceCursorInvalid),
    }
}

fn event_digest(event: &RuntimeEvent) -> String {
    json_digest(&serde_json::json!({
        "request_id": event.request_id,
        "sequence": event.sequence,
        "kind": event.kind,
        "data": event.data,
        "aggregate_type": event.aggregate_type,
        "aggregate_id": event.aggregate_id,
        "stream_version": event.stream_version,
    }))
}

fn bounded_ids(events: &[ObservedEvent<'_>]) -> Vec<EventId> {
    events
        .iter()
        .map(|observed| observed.event.event_id)
        .take(MAX_SAMPLE_IDS)
        .collect()
}

fn numeric(data: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| data.get(*name).and_then(Value::as_u64))
}

fn has_error(event: &RuntimeEvent) -> bool {
    event.data.get("error").is_some_and(|error| match error {
        Value::Null => false,
        Value::String(value) => !value.trim().is_empty(),
        _ => true,
    }) || event.kind.contains("failed")
        || event.kind.contains("failure")
}

fn unknown_effect(event: &RuntimeEvent) -> bool {
    event.kind.contains("result_unknown")
        || event
            .data
            .get("effect_known")
            .and_then(Value::as_bool)
            .is_some_and(|known| !known)
        || event
            .data
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error.contains("result_unknown"))
}

fn operation_key(event: &RuntimeEvent) -> Option<String> {
    let id = [
        event.data.get("execution_id"),
        event.data.get("invocation_id"),
        event.data.get("capability_request_id"),
        event.data.get("request_id"),
        event
            .data
            .get("permit")
            .and_then(|permit| permit.get("execution_id")),
        event
            .data
            .get("permit")
            .and_then(|permit| permit.get("invocation_id")),
        event
            .data
            .get("permit")
            .and_then(|permit| permit.get("request_id")),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)?;
    let attempt = event
        .data
        .get("attempt")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    Some(format!("{id}:{attempt}"))
}

fn is_dispatch(event: &RuntimeEvent) -> bool {
    matches!(
        event.kind.as_str(),
        "invocation.dispatching" | "invocation.executing"
    )
}

fn is_terminal(event: &RuntimeEvent) -> bool {
    matches!(
        event.kind.as_str(),
        "execution.result_committed"
            | "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "run.tool_result"
    )
}

fn is_query(event: &RuntimeEvent) -> bool {
    let kind = event.kind.as_str();
    kind.contains("query")
        || matches!(kind, "receipt.read" | "receipt.projected" | "run.receipt")
        || event
            .data
            .get("metric_area")
            .and_then(Value::as_str)
            .is_some_and(|area| area == "receipt")
}

fn is_artifact(event: &RuntimeEvent) -> bool {
    event.kind.contains("artifact")
        || event.data.get("artifact_id").is_some()
        || event.data.get("artifact_bytes").is_some()
}

fn add_limitation(limitations: &mut Vec<String>, limitation: &'static str) {
    if limitations.len() < MAX_LIMITATIONS && !limitations.iter().any(|item| item == limitation) {
        limitations.push(limitation.to_owned());
    }
}

fn metric_point(
    catalog: &MetricCatalog,
    name: &'static str,
    value: u64,
    source_cursor: EventCursor,
    source_events: &[ObservedEvent<'_>],
) -> Result<MetricPoint, MetricsProjectionError> {
    let definition = catalog
        .metric(name)
        .ok_or(MetricsProjectionError::CatalogMissing(name))?;
    MetricPoint::new(
        name,
        definition.kind,
        value as f64,
        definition.unit.clone(),
        BTreeMap::new(),
        definition.source,
        source_cursor,
        bounded_ids(source_events),
    )
    .map_err(MetricsProjectionError::PointInvalid)
}

fn sample_point(
    catalog: &MetricCatalog,
    name: &'static str,
    samples: &[(u64, EventId)],
    source_cursor: EventCursor,
    source_events: &[ObservedEvent<'_>],
) -> Result<Option<MetricPoint>, MetricsProjectionError> {
    if samples.is_empty() {
        return Ok(None);
    }
    let value = samples
        .iter()
        .map(|(sample, _)| *sample)
        .max()
        .unwrap_or_default();
    let point = metric_point(catalog, name, value, source_cursor, source_events)?;
    Ok(Some(point))
}

fn validate_cursor_order(events: &[ObservedEvent<'_>]) -> bool {
    let mut cursors = events.iter().map(|event| event.cursor).collect::<Vec<_>>();
    cursors.sort_unstable();
    cursors.dedup();
    cursors.windows(2).all(|window| {
        window[0]
            .checked_add(1)
            .is_some_and(|expected| expected == window[1])
    })
}

fn validate_stream_order(events: &[ObservedEvent<'_>]) -> bool {
    let mut streams: HashMap<(String, String), Vec<u64>> = HashMap::new();
    for observed in events {
        let Some((aggregate_type, aggregate_id, version)) = observed
            .event
            .aggregate_type
            .as_ref()
            .zip(observed.event.aggregate_id.as_ref())
            .zip(observed.event.stream_version)
            .map(|((kind, id), version)| (kind.clone(), id.clone(), version))
        else {
            continue;
        };
        if version == 0 {
            return false;
        }
        streams
            .entry((aggregate_type, aggregate_id))
            .or_default()
            .push(version);
    }
    streams.values_mut().all(|versions| {
        versions.sort_unstable();
        versions.dedup();
        versions.windows(2).all(|window| {
            window[0]
                .checked_add(1)
                .is_some_and(|expected| expected == window[1])
        })
    })
}

/// Fold committed EventLog facts into one bounded operational metric snapshot.
pub fn project_operational_metrics(
    events: &[RuntimeEvent],
    projector_cursor: Option<EventCursor>,
) -> Result<MetricSnapshot, MetricsProjectionError> {
    if events.is_empty() {
        return Err(MetricsProjectionError::SourceEmpty);
    }

    let mut observed = Vec::with_capacity(events.len());
    let mut seen = HashMap::<String, String>::new();
    for (index, event) in events.iter().enumerate() {
        let event_id = event.event_id.to_string();
        let digest = event_digest(event);
        if let Some(previous) = seen.insert(event_id, digest.clone()) {
            if previous != digest {
                return Err(MetricsProjectionError::DuplicateEventConflict);
            }
            continue;
        }
        observed.push(ObservedEvent {
            cursor: source_cursor(event, index)?,
            event,
        });
    }
    if observed.is_empty() {
        return Err(MetricsProjectionError::SourceEmpty);
    }

    let durable_cursor = observed
        .iter()
        .map(|event| event.cursor)
        .max()
        .ok_or(MetricsProjectionError::SourceEmpty)?;
    let mut limitations = Vec::new();
    if !validate_cursor_order(&observed) || !validate_stream_order(&observed) {
        add_limitation(&mut limitations, "eventlog_cursor_gap");
    }
    if observed.len() > MAX_SAMPLE_IDS {
        add_limitation(&mut limitations, "source_event_ids_truncated");
    }
    if observed
        .iter()
        .any(|event| event.event.data.get("cursor_gap").and_then(Value::as_bool) == Some(true))
    {
        add_limitation(&mut limitations, "eventlog_cursor_gap");
    }

    let projector_cursor_observed = projector_cursor.is_some()
        || observed
            .iter()
            .any(|event| event.event.data.get("projector_cursor").is_some());
    let projector_cursor = projector_cursor
        .or_else(|| {
            observed
                .iter()
                .filter_map(|event| {
                    event
                        .event
                        .data
                        .get("projector_cursor")
                        .and_then(Value::as_u64)
                })
                .max()
        })
        .unwrap_or(durable_cursor);
    if projector_cursor > durable_cursor {
        return Err(MetricsProjectionError::ProjectorCursorInvalid);
    }
    if !projector_cursor_observed {
        add_limitation(&mut limitations, "projector_cursor_inferred");
    }
    let projector_lag = durable_cursor.saturating_sub(projector_cursor);

    let catalog = MetricCatalog::builtin();
    let source_events = observed.as_slice();
    let source_ids = bounded_ids(source_events);
    let mut points = Vec::new();

    let mut commit_ids = BTreeSet::new();
    let mut commit_total = 0_u64;
    let mut commit_failures = 0_u64;
    let mut append_latency = Vec::new();
    let mut flush_latency = Vec::new();
    let mut rebuild_total = 0_u64;
    let mut rebuild_latency = Vec::new();
    let mut receipt_queries = 0_u64;
    let mut receipt_query_failures = 0_u64;
    let mut receipt_latency = Vec::new();
    let mut artifact_bytes = 0_u64;
    let mut artifact_read_failures = 0_u64;
    let mut artifact_observed = false;
    let mut orphan_candidates = HashSet::new();
    let mut terminal_operations = HashSet::new();
    let mut missing_operation_key = false;
    let mut unknown_events = HashSet::new();
    let mut last_error_present = false;

    for observed_event in source_events {
        let event = observed_event.event;
        let kind = event.kind.as_str();
        let commit_like = kind == "eventlog.commit"
            || kind == "eventlog.committed"
            || kind == "commit.committed"
            || kind == "transition.committed"
            || event.data.get("commit_id").is_some();
        if commit_like {
            let id = event
                .data
                .get("commit_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| event.event_id.to_string());
            if commit_ids.insert(id) {
                commit_total = commit_total.saturating_add(1);
            }
            if event.data.get("success").and_then(Value::as_bool) == Some(false)
                || has_error(event)
                || kind.contains("failure")
            {
                commit_failures = commit_failures.saturating_add(1);
                last_error_present = true;
            }
        }
        if kind.contains("eventlog") || kind.starts_with("commit.") {
            if let Some(value) = numeric(&event.data, &["append_latency_ms", "commit_latency_ms"]) {
                append_latency.push((value, event.event_id));
            }
            if let Some(value) = event.data.get("flush_latency_ms").and_then(Value::as_u64) {
                flush_latency.push((value, event.event_id));
            }
        }
        if kind.contains("rebuild") || kind == "projector.rebuilt" {
            rebuild_total = rebuild_total.saturating_add(1);
            if let Some(value) = numeric(&event.data, &["rebuild_latency_ms", "elapsed_ms"]) {
                rebuild_latency.push((value, event.event_id));
            }
        }
        if is_query(event) {
            receipt_queries = receipt_queries.saturating_add(1);
            if has_error(event) || event.data.get("success").and_then(Value::as_bool) == Some(false)
            {
                receipt_query_failures = receipt_query_failures.saturating_add(1);
                last_error_present = true;
            }
            if let Some(value) = numeric(&event.data, &["query_latency_ms", "elapsed_ms"]) {
                receipt_latency.push((value, event.event_id));
            }
        }
        if is_artifact(event) {
            if let Some(value) = numeric(
                &event.data,
                &["artifact_bytes", "size_bytes", "total_bytes"],
            ) {
                artifact_bytes = artifact_bytes.saturating_add(value);
                artifact_observed = true;
            }
            if has_error(event) {
                artifact_read_failures = artifact_read_failures.saturating_add(1);
                last_error_present = true;
            }
        }
        if is_dispatch(event) {
            if let Some(key) = operation_key(event) {
                orphan_candidates.insert(key);
            } else {
                missing_operation_key = true;
            }
        }
        if is_terminal(event) {
            if let Some(key) = operation_key(event) {
                terminal_operations.insert(key);
            }
        }
        if unknown_effect(event) {
            unknown_events.insert(event.event_id.to_string());
        }
        if has_error(event) {
            last_error_present = true;
        }
    }
    if !append_latency.is_empty() && append_latency.iter().all(|(value, _)| *value == 0) {
        add_limitation(&mut limitations, "append_latency_zero_sample");
    }
    if append_latency.is_empty() {
        add_limitation(&mut limitations, "append_latency_unobserved");
    }
    if flush_latency.is_empty() {
        add_limitation(&mut limitations, "flush_latency_unobserved");
    }
    if commit_total == 0 {
        add_limitation(&mut limitations, "commit_receipt_unobserved");
    }
    if missing_operation_key {
        add_limitation(&mut limitations, "orphan_correlation_missing");
    }
    let orphan_total = orphan_candidates.difference(&terminal_operations).count() as u64;
    if orphan_total > 0 {
        add_limitation(&mut limitations, "orphan_dispatch_detected");
    }
    let unknown_total = unknown_events.len() as u64;
    if unknown_total > 0 {
        add_limitation(&mut limitations, "effect_unknown_detected");
    }
    if artifact_bytes == 0
        && !artifact_observed
        && source_events.iter().any(|event| is_artifact(event.event))
    {
        add_limitation(&mut limitations, "artifact_bytes_unobserved");
    }
    if artifact_read_failures > 0 {
        add_limitation(&mut limitations, "artifact_read_failed");
    }

    points.push(metric_point(
        &catalog,
        EVENTLOG_COMMIT_TOTAL,
        commit_total,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        EVENTLOG_COMMIT_FAILURE_TOTAL,
        commit_failures,
        durable_cursor,
        source_events,
    )?);
    for point in [
        sample_point(
            &catalog,
            EVENTLOG_APPEND_LATENCY,
            &append_latency,
            durable_cursor,
            source_events,
        )?,
        sample_point(
            &catalog,
            EVENTLOG_FLUSH_LATENCY,
            &flush_latency,
            durable_cursor,
            source_events,
        )?,
        sample_point(
            &catalog,
            PROJECTOR_REBUILD_LATENCY,
            &rebuild_latency,
            durable_cursor,
            source_events,
        )?,
        sample_point(
            &catalog,
            RECEIPT_QUERY_LATENCY,
            &receipt_latency,
            durable_cursor,
            source_events,
        )?,
    ]
    .into_iter()
    .flatten()
    {
        points.push(point);
    }
    points.push(metric_point(
        &catalog,
        EVENTLOG_DURABLE_CURSOR,
        durable_cursor,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        PROJECTOR_CURSOR,
        projector_cursor,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        PROJECTOR_LAG_EVENTS,
        projector_lag,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        PROJECTOR_REBUILD_TOTAL,
        rebuild_total,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        RECEIPT_QUERY_TOTAL,
        receipt_queries,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        RECEIPT_QUERY_FAILURE_TOTAL,
        receipt_query_failures,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        ARTIFACT_BYTES_TOTAL,
        artifact_bytes,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        ARTIFACT_READ_FAILURE_TOTAL,
        artifact_read_failures,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        RECOVERY_ORPHAN_TOTAL,
        orphan_total,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        RECOVERY_UNKNOWN_TOTAL,
        unknown_total,
        durable_cursor,
        source_events,
    )?);
    points.push(metric_point(
        &catalog,
        RECOVERY_LAST_ERROR_PRESENT,
        u64::from(last_error_present),
        durable_cursor,
        source_events,
    )?);

    let status = if limitations.is_empty() {
        SignalStatus::Ok
    } else {
        SignalStatus::Degraded
    };
    MetricSnapshot::new(
        status,
        durable_cursor,
        projector_cursor,
        source_ids,
        points,
        limitations,
    )
    .map_err(MetricsProjectionError::SnapshotInvalid)
}

/// Convenience projection with no claimed durable projector checkpoint.
pub fn project_metrics(events: &[RuntimeEvent]) -> Result<MetricSnapshot, MetricsProjectionError> {
    project_operational_metrics(events, None)
}

/// Run-scoped metrics are useful to callers that already have a filtered EventLog slice. The
/// reducer itself remains global and does not use the run ID as a metric label.
pub fn project_run_metrics(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<MetricSnapshot, MetricsProjectionError> {
    let run_id = run_id.to_string();
    let filtered = events
        .iter()
        .filter(|event| {
            event.data.get("run_id").and_then(Value::as_str) == Some(run_id.as_str())
                || (event.aggregate_type.as_deref() == Some("run")
                    && event.aggregate_id.as_deref() == Some(run_id.as_str()))
        })
        .cloned()
        .collect::<Vec<_>>();
    project_operational_metrics(&filtered, None)
}

impl ControlPlane {
    /// Rebuild operational metrics from the EventLog. An adapter without global reads is
    /// explicitly unavailable; a run stream is not silently presented as system-wide health.
    pub async fn operational_metrics(&self) -> Result<MetricSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("metrics_read_all_unsupported".to_owned())
        })?;
        project_operational_metrics(&events, None)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }

    /// Compatibility alias for metric-oriented callers.
    pub async fn metrics(&self) -> Result<MetricSnapshot, CoreError> {
        self.operational_metrics().await
    }
}
