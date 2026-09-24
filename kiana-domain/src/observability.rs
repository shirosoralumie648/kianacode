//! Versioned domain contracts for observability and audit signals.
//!
//! These values describe committed facts or bounded projections; they do not write to an
//! `EventStore`, grant authority, or perform an export. The contracts deliberately keep
//! correlation and trace identifiers as opaque references until OA-02 introduces their typed
//! relationship.

use crate::{
    canonical_journal_bytes, json_digest, DataClass, EventCursor, EventId, ExecutionId,
    InvocationId, ModelAttemptId, ModelFinish, ModelPurpose, ModelRetryClass, ModelUsage,
    RedactionProfile, RedactionSignal, RequestId, RunId, RuntimeEvent, SchemaVersion, SpanId,
    StepId, TraceId, TurnId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const OBSERVABILITY_SCHEMA: &str = "kiana.observability.v1";
pub const AUDIT_RECORD_SCHEMA: &str = "kiana.audit-record.v1";
pub const AUDIT_EVENT_SCHEMA: &str = "kiana.audit-event.v1";
pub const AUDIT_EVENT_KIND: &str = "audit.record";
pub const AUDIT_PROJECTION_SCHEMA: &str = "kiana.audit-projection.v1";
pub const AUDIT_PROJECTION_CHECKPOINT_SCHEMA: &str = "kiana.audit-projection-checkpoint.v1";
pub const AUDIT_QUERY_CURSOR_SCHEMA: &str = "kiana.audit-query-cursor.v1";
pub const AUDIT_EXPORT_SCHEMA: &str = "kiana.audit-export.v1";
pub const AUDIT_DELIVERY_RECEIPT_SCHEMA: &str = "kiana.audit-delivery-receipt.v1";
pub const OBSERVABILITY_ALERT_SCHEMA: &str = "kiana.observability-alert.v1";
pub const OBSERVABILITY_INCIDENT_SCHEMA: &str = "kiana.observability-incident.v1";
pub const OBSERVABILITY_INCIDENT_SNAPSHOT_SCHEMA: &str = "kiana.observability-incident-snapshot.v1";
pub const REPLAY_DIAGNOSTIC_SCHEMA: &str = "kiana.replay-diagnostic.v1";
pub const REPLAY_DIAGNOSTIC_SNAPSHOT_SCHEMA: &str = "kiana.replay-diagnostic-snapshot.v1";
pub const METRIC_CATALOG_SCHEMA: &str = "kiana.metric-catalog.v1";
pub const TRACE_SUMMARY_SCHEMA: &str = "kiana.trace-summary.v1";
pub const TRACE_EXPORT_SPAN_SCHEMA: &str = "kiana.trace-export-span.v1";
pub const METRIC_SNAPSHOT_SCHEMA: &str = "kiana.metric-snapshot.v1";
pub const HEALTH_SNAPSHOT_SCHEMA: &str = "kiana.health-snapshot.v1";
pub const OPERATOR_EVIDENCE_SCHEMA: &str = "kiana.operator-evidence.v1";
pub const SPAN_LIFECYCLE_SCHEMA: &str = "kiana.span-lifecycle.v1";
pub const OBSERVABILITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const AUDIT_RECORD_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const AUDIT_EVENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const AUDIT_PROJECTION_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const AUDIT_PROJECTION_CHECKPOINT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const AUDIT_QUERY_CURSOR_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const AUDIT_EXPORT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const AUDIT_DELIVERY_RECEIPT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const OBSERVABILITY_ALERT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const OBSERVABILITY_INCIDENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const OBSERVABILITY_INCIDENT_SNAPSHOT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const REPLAY_DIAGNOSTIC_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const REPLAY_DIAGNOSTIC_SNAPSHOT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const METRIC_CATALOG_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const TRACE_SUMMARY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const TRACE_EXPORT_SPAN_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const METRIC_SNAPSHOT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const HEALTH_SNAPSHOT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const OPERATOR_EVIDENCE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const SPAN_LIFECYCLE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MODEL_ATTEMPT_SCHEMA: &str = "kiana.model-attempt.v1";
pub const MODEL_ATTEMPT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const CAPABILITY_ATTEMPT_SCHEMA: &str = "kiana.capability-attempt.v1";
pub const CAPABILITY_ATTEMPT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

pub const MAX_SIGNAL_ATTRIBUTES: usize = 32;
pub const MAX_ATTRIBUTE_KEY_BYTES: usize = 64;
pub const MAX_ATTRIBUTE_VALUE_BYTES: usize = 256;
pub const MAX_METRIC_NAME_BYTES: usize = 128;
pub const MAX_METRIC_UNIT_BYTES: usize = 32;
pub const MAX_METRIC_DESCRIPTION_BYTES: usize = 512;
pub const MAX_SOURCE_EVENT_IDS: usize = 256;
pub const MAX_METRIC_POINTS: usize = 128;
pub const MAX_METRIC_SERIES: usize = 1_024;
pub const MAX_METRIC_LABEL_VALUES: usize = 64;
pub const MAX_AUDIT_PROJECTION_RECORDS: usize = 4_096;
pub const MAX_AUDIT_EXPORT_RECORDS: usize = 4_096;
pub const MAX_OBSERVABILITY_ALERTS: usize = 128;
pub const MAX_OBSERVABILITY_INCIDENTS: usize = 128;
pub const MAX_REPLAY_DIAGNOSTICS: usize = 256;
pub const MAX_TRACE_SPANS: u32 = 4_096;
pub const MAX_HEALTH_LIMITATIONS: usize = 16;
pub const MAX_HEALTH_CAPABILITIES: usize = 32;
pub const MAX_OPERATOR_EVIDENCE_REFS: usize = 64;
pub const MAX_MODEL_PROVIDER_BYTES: usize = 128;
pub const MAX_MODEL_ID_BYTES: usize = 256;
pub const MAX_MODEL_ERROR_CODE_BYTES: usize = 128;
pub const MAX_MODEL_USAGE_TOKENS: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Log,
    Metric,
    Trace,
    Audit,
    Health,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalStatus {
    Ok,
    Error,
    Unknown,
    Degraded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanEntityKind {
    Run,
    Turn,
    Invocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanLifecyclePhase {
    Started,
    Paused,
    Resumed,
    Checkpointed,
    Ended,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditActionKind {
    Command,
    Authorization,
    Approval,
    Capability,
    Credential,
    Recovery,
    Query,
    Export,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDecision {
    Accepted,
    Denied,
    Staged,
    Approved,
    Consumed,
    Failed,
    Unknown,
    Queried,
    Exported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricSource {
    EventReducer,
    RuntimeGauge,
    Derived,
}

/// Measurement quality is explicit so an estimate can never be consumed as a measured value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricQuality {
    Measured,
    Estimated,
    Unknown,
}

impl Default for MetricQuality {
    fn default() -> Self {
        Self::Measured
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricStability {
    Experimental,
    Stable,
    Deprecated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Ok,
    Error,
    Unknown,
    Degraded,
}

fn validate_header(
    schema: &str,
    version: SchemaVersion,
    expected_schema: &str,
    expected_version: SchemaVersion,
) -> Result<(), String> {
    if schema != expected_schema {
        return Err(format!("schema_mismatch:{schema}"));
    }
    if !expected_version.is_compatible_with(&version) {
        return Err(format!(
            "schema_version_incompatible:{schema}:expected_major={}:incoming_major={}",
            expected_version.major, version.major
        ));
    }
    Ok(())
}

fn validate_nonempty(value: &str, field: &str, max_bytes: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field}_required"));
    }
    if value.len() > max_bytes {
        return Err(format!("{field}_too_long"));
    }
    Ok(())
}

fn validate_attributes(attributes: &BTreeMap<String, String>) -> Result<(), String> {
    if attributes.len() > MAX_SIGNAL_ATTRIBUTES {
        return Err("observability_attribute_limit".to_owned());
    }
    for (key, value) in attributes {
        validate_nonempty(key, "attribute_key", MAX_ATTRIBUTE_KEY_BYTES)?;
        if value.len() > MAX_ATTRIBUTE_VALUE_BYTES {
            return Err("observability_attribute_value_too_long".to_owned());
        }
    }
    Ok(())
}

fn validate_cursor(cursor: u64, source_event_ids: &[EventId]) -> Result<(), String> {
    if cursor == 0 {
        return Err("observability_source_cursor_required".to_owned());
    }
    if source_event_ids.is_empty() {
        return Err("observability_source_event_ids_required".to_owned());
    }
    if source_event_ids.len() > MAX_SOURCE_EVENT_IDS {
        return Err("observability_source_event_ids_limit".to_owned());
    }
    let mut unique = BTreeSet::new();
    if source_event_ids
        .iter()
        .any(|id| !unique.insert(id.to_string()))
    {
        return Err("observability_source_event_ids_duplicate".to_owned());
    }
    Ok(())
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn value_without_digest<T: Serialize>(value: &T, field: &str) -> Result<serde_json::Value, String> {
    let mut value =
        serde_json::to_value(value).map_err(|_| "observability_encode_failed".to_owned())?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "observability_object_required".to_owned())?;
    object.insert(field.to_owned(), serde_json::Value::String(String::new()));
    Ok(value)
}

fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    canonical_journal_bytes(value).map_err(|error| format!("observability_canonical:{error}"))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub signal_id: String,
    pub signal: SignalKind,
    pub status: SignalStatus,
    pub component: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub payload_digest: String,
}

impl ObservabilityRecord {
    pub fn new(
        signal_id: impl Into<String>,
        signal: SignalKind,
        status: SignalStatus,
        component: impl Into<String>,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        attributes: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut record = Self {
            schema: OBSERVABILITY_SCHEMA.to_owned(),
            version: OBSERVABILITY_SCHEMA_VERSION,
            signal_id: signal_id.into(),
            signal,
            status,
            component: component.into(),
            source_cursor,
            source_event_ids,
            attributes,
            payload_digest: String::new(),
        };
        record.payload_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            OBSERVABILITY_SCHEMA,
            OBSERVABILITY_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.signal_id, "observability_signal_id", 128)?;
        validate_nonempty(&self.component, "observability_component", 128)?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        validate_attributes(&self.attributes)?;
        validate_digest(&self.payload_digest, "observability_payload_digest")?;
        let expected = self.digest();
        if self.payload_digest != expected {
            return Err("observability_payload_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "payload_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDefinition {
    pub name: String,
    pub kind: MetricKind,
    pub unit: String,
    pub description: String,
    pub stability: MetricStability,
    #[serde(default)]
    pub allowed_labels: Vec<String>,
    pub privacy_class: DataClass,
    pub source: MetricSource,
}

impl MetricDefinition {
    pub fn counter(name: impl Into<String>, unit: impl Into<String>) -> Self {
        Self::with_kind(name, MetricKind::Counter, unit)
    }

    pub fn gauge(name: impl Into<String>, unit: impl Into<String>) -> Self {
        Self::with_kind(name, MetricKind::Gauge, unit)
    }

    pub fn histogram(name: impl Into<String>, unit: impl Into<String>) -> Self {
        Self::with_kind(name, MetricKind::Histogram, unit)
    }

    fn with_kind(name: impl Into<String>, kind: MetricKind, unit: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind,
            unit: unit.into(),
            description: "registered runtime metric".to_owned(),
            stability: MetricStability::Stable,
            allowed_labels: Vec::new(),
            privacy_class: DataClass::Internal,
            source: MetricSource::EventReducer,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_nonempty(&self.name, "metric_name", MAX_METRIC_NAME_BYTES)?;
        if !self.name.starts_with("kiana.")
            || !self.name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || ".-_".contains(byte as char)
            })
        {
            return Err("metric_name_invalid".to_owned());
        }
        validate_nonempty(&self.unit, "metric_unit", MAX_METRIC_UNIT_BYTES)?;
        validate_nonempty(
            &self.description,
            "metric_description",
            MAX_METRIC_DESCRIPTION_BYTES,
        )?;
        if self.allowed_labels.len() > MAX_SIGNAL_ATTRIBUTES {
            return Err("metric_allowed_label_limit".to_owned());
        }
        let mut labels = BTreeSet::new();
        for label in &self.allowed_labels {
            validate_nonempty(label, "metric_label", MAX_ATTRIBUTE_KEY_BYTES)?;
            if !labels.insert(label) {
                return Err("metric_allowed_label_duplicate".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricCatalog {
    pub schema: String,
    pub version: SchemaVersion,
    pub metrics: Vec<MetricDefinition>,
    pub catalog_digest: String,
}

impl MetricCatalog {
    pub fn new(metrics: Vec<MetricDefinition>) -> Result<Self, String> {
        Self::with_version(METRIC_CATALOG_SCHEMA_VERSION, metrics)
    }

    pub fn with_version(
        version: SchemaVersion,
        mut metrics: Vec<MetricDefinition>,
    ) -> Result<Self, String> {
        metrics.sort_by(|left, right| left.name.cmp(&right.name));
        let mut catalog = Self {
            schema: METRIC_CATALOG_SCHEMA.to_owned(),
            version,
            metrics,
            catalog_digest: String::new(),
        };
        catalog.catalog_digest = catalog.digest();
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn builtin() -> Self {
        const NAMES: &[&str] = &[
            "kiana.commands.accepted_total",
            "kiana.commands.denied_total",
            "kiana.runs.started_total",
            "kiana.runs.completed_total",
            "kiana.runs.failed_total",
            "kiana.runs.cancelled_total",
            "kiana.runs.result_unknown_total",
            "kiana.invocations.attempt_total",
            "kiana.invocations.effect_unknown_total",
            "kiana.approvals.requested_total",
            "kiana.approvals.decided_total",
            "kiana.eventlog.commit_total",
            "kiana.eventlog.commit_failure_total",
            "kiana.eventlog.append_latency_ms",
            "kiana.eventlog.flush_latency_ms",
            "kiana.eventlog.durable_cursor",
            "kiana.projector.cursor",
            "kiana.projector.lag_events",
            "kiana.projector.rebuild_total",
            "kiana.projector.rebuild_latency_ms",
            "kiana.metrics.cardinality_overflow_total",
            "kiana.receipts.query_total",
            "kiana.receipts.query_failure_total",
            "kiana.receipts.query_latency_ms",
            "kiana.artifacts.bytes_total",
            "kiana.artifacts.read_failure_total",
            "kiana.recovery.orphan_total",
            "kiana.recovery.unknown_total",
            "kiana.recovery.last_error_present",
            "kiana.observability.queue_depth",
            "kiana.observability.dropped_best_effort_total",
            "kiana.observability.export_failure_total",
            "kiana.provider.request_total",
            "kiana.provider.retry_total",
            "kiana.security.redaction_total",
            "kiana.security.redaction_failure_total",
            "kiana.audit.query_total",
            "kiana.audit.query_denied_total",
            "kiana.health.degraded_total",
            "kiana.incidents.open_total",
        ];
        let metrics = NAMES
            .iter()
            .map(|name| {
                if name.ends_with("_latency_ms") {
                    MetricDefinition::histogram(*name, "ms")
                } else if name.ends_with("_cursor")
                    || name.ends_with("_lag_events")
                    || name.ends_with("_bytes_total")
                    || name.ends_with("_last_error_present")
                {
                    MetricDefinition::gauge(
                        *name,
                        if name.ends_with("_bytes_total") {
                            "bytes"
                        } else if name.ends_with("_last_error_present") {
                            "bool"
                        } else {
                            "events"
                        },
                    )
                } else {
                    MetricDefinition::counter(*name, "count")
                }
            })
            .collect();
        Self::new(metrics).expect("built-in metric catalog is valid")
    }

    pub fn metric(&self, name: &str) -> Option<&MetricDefinition> {
        self.metrics.iter().find(|metric| metric.name == name)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            METRIC_CATALOG_SCHEMA,
            METRIC_CATALOG_SCHEMA_VERSION,
        )?;
        if self.metrics.is_empty() {
            return Err("metric_catalog_empty".to_owned());
        }
        let mut names = BTreeSet::new();
        for metric in &self.metrics {
            metric.validate()?;
            if !names.insert(&metric.name) {
                return Err("metric_catalog_duplicate".to_owned());
            }
        }
        validate_digest(&self.catalog_digest, "metric_catalog_digest")?;
        if self.catalog_digest != self.digest() {
            return Err("metric_catalog_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "catalog_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricPoint {
    pub schema: String,
    pub version: SchemaVersion,
    pub name: String,
    pub kind: MetricKind,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub quality: MetricQuality,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub source: MetricSource,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub point_digest: String,
}

impl MetricPoint {
    pub fn new(
        name: impl Into<String>,
        kind: MetricKind,
        value: f64,
        unit: impl Into<String>,
        labels: BTreeMap<String, String>,
        source: MetricSource,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        Self::with_version(
            METRIC_CATALOG_SCHEMA_VERSION,
            name,
            kind,
            value,
            unit,
            labels,
            source,
            source_cursor,
            source_event_ids,
        )
    }

    pub fn with_version(
        version: SchemaVersion,
        name: impl Into<String>,
        kind: MetricKind,
        value: f64,
        unit: impl Into<String>,
        labels: BTreeMap<String, String>,
        source: MetricSource,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        Self::with_version_and_quality(
            version,
            name,
            kind,
            value,
            unit,
            MetricQuality::Measured,
            labels,
            source,
            source_cursor,
            source_event_ids,
        )
    }

    pub fn new_with_quality(
        name: impl Into<String>,
        kind: MetricKind,
        value: f64,
        unit: impl Into<String>,
        quality: MetricQuality,
        labels: BTreeMap<String, String>,
        source: MetricSource,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        Self::with_version_and_quality(
            METRIC_CATALOG_SCHEMA_VERSION,
            name,
            kind,
            value,
            unit,
            quality,
            labels,
            source,
            source_cursor,
            source_event_ids,
        )
    }

    pub fn with_version_and_quality(
        version: SchemaVersion,
        name: impl Into<String>,
        kind: MetricKind,
        value: f64,
        unit: impl Into<String>,
        quality: MetricQuality,
        labels: BTreeMap<String, String>,
        source: MetricSource,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        let mut point = Self {
            schema: METRIC_CATALOG_SCHEMA.to_owned(),
            version,
            name: name.into(),
            kind,
            value,
            unit: unit.into(),
            quality,
            labels,
            source,
            source_cursor,
            source_event_ids,
            point_digest: String::new(),
        };
        point.point_digest = point.digest();
        point.validate()?;
        Ok(point)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            METRIC_CATALOG_SCHEMA,
            METRIC_CATALOG_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.name, "metric_name", MAX_METRIC_NAME_BYTES)?;
        if !self.value.is_finite() {
            return Err("metric_value_invalid".to_owned());
        }
        validate_nonempty(&self.unit, "metric_unit", MAX_METRIC_UNIT_BYTES)?;
        validate_attributes(&self.labels)?;
        if self.quality == MetricQuality::Unknown {
            return Err("metric_quality_unknown".to_owned());
        }
        if self.name.ends_with("_measured_total") && self.quality != MetricQuality::Measured {
            return Err("metric_measured_quality_conflict".to_owned());
        }
        if self.name.ends_with("_estimated_total") && self.quality != MetricQuality::Estimated {
            return Err("metric_estimated_quality_required".to_owned());
        }
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        validate_digest(&self.point_digest, "metric_point_digest")?;
        if self.point_digest != self.digest() {
            return Err("metric_point_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_with_catalog(&self, catalog: &MetricCatalog) -> Result<(), String> {
        self.validate()?;
        catalog.validate()?;
        let definition = catalog
            .metric(&self.name)
            .ok_or_else(|| "metric_unregistered".to_owned())?;
        if definition.kind != self.kind
            || definition.unit != self.unit
            || definition.source != self.source
        {
            return Err("metric_definition_mismatch".to_owned());
        }
        if self.labels.keys().any(|label| {
            !definition
                .allowed_labels
                .iter()
                .any(|allowed| allowed == label)
        }) {
            return Err("metric_label_not_registered".to_owned());
        }
        if self.quality == MetricQuality::Estimated && definition.source != MetricSource::Derived {
            return Err("metric_estimate_source_conflict".to_owned());
        }
        if self.kind == MetricKind::Counter && self.value < 0.0 {
            return Err("metric_counter_negative".to_owned());
        }
        Ok(())
    }

    pub fn with_quality(mut self, quality: MetricQuality) -> Result<Self, String> {
        self.quality = quality;
        self.point_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "point_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

/// A bounded, replayable collection of metric points at one EventLog cursor.
///
/// The snapshot is a projection boundary: it reports the cursor and limitations used to
/// calculate the points, but it never upgrades a lagging projector, missing artifact or unknown
/// effect into a healthy result. Runtime gauges may be folded into the same shape later, but they
/// must still carry a source cursor and an explicit limitation when no durable checkpoint exists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: SignalStatus,
    pub source_cursor: u64,
    pub projector_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    #[serde(default)]
    pub catalog_digest: Option<String>,
    pub points: Vec<MetricPoint>,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub snapshot_digest: String,
}

impl MetricSnapshot {
    pub fn new(
        status: SignalStatus,
        source_cursor: u64,
        projector_cursor: u64,
        source_event_ids: Vec<EventId>,
        points: Vec<MetricPoint>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: METRIC_SNAPSHOT_SCHEMA.to_owned(),
            version: METRIC_SNAPSHOT_SCHEMA_VERSION,
            status,
            source_cursor,
            projector_cursor,
            source_event_ids,
            catalog_digest: None,
            points,
            limitations,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            METRIC_SNAPSHOT_SCHEMA,
            METRIC_SNAPSHOT_SCHEMA_VERSION,
        )?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.projector_cursor > self.source_cursor {
            return Err("metric_projector_cursor_ahead".to_owned());
        }
        if self.points.is_empty() {
            return Err("metric_snapshot_points_required".to_owned());
        }
        if self.points.len() > MAX_METRIC_POINTS {
            return Err("metric_snapshot_points_limit".to_owned());
        }
        if let Some(catalog_digest) = &self.catalog_digest {
            validate_digest(catalog_digest, "metric_snapshot_catalog_digest")?;
        }
        let mut identities = BTreeSet::new();
        for point in &self.points {
            point.validate()?;
            if point.source_cursor > self.source_cursor {
                return Err("metric_point_cursor_ahead".to_owned());
            }
            let identity = serde_json::to_string(&(point.name.as_str(), &point.labels))
                .map_err(|_| "metric_snapshot_identity_encode_failed".to_owned())?;
            if !identities.insert(identity) {
                return Err("metric_snapshot_duplicate_point".to_owned());
            }
        }
        if self.limitations.len() > MAX_HEALTH_LIMITATIONS {
            return Err("metric_snapshot_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            validate_nonempty(
                limitation,
                "metric_snapshot_limitation",
                MAX_ATTRIBUTE_VALUE_BYTES,
            )?;
        }
        validate_digest(&self.snapshot_digest, "metric_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("metric_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn with_catalog_digest(
        mut self,
        catalog_digest: impl Into<String>,
    ) -> Result<Self, String> {
        self.catalog_digest = Some(catalog_digest.into());
        self.snapshot_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "snapshot_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub audit_id: String,
    pub action_kind: AuditActionKind,
    pub decision: AuditDecision,
    pub actor_ref: String,
    pub target_kind: String,
    pub target_ref: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    #[serde(default)]
    pub request_id: Option<RequestId>,
    #[serde(default)]
    pub command_id: Option<RequestId>,
    #[serde(default)]
    pub correlation_ref: Option<String>,
    #[serde(default)]
    pub causation_ref: Option<String>,
    #[serde(default)]
    pub action_digest: Option<String>,
    #[serde(default)]
    pub input_digest: Option<String>,
    #[serde(default)]
    pub reason_code: Option<String>,
    /// A mandatory, redaction-safe explanation. `reason_code` is retained as a compatibility
    /// alias for older projections; new records always carry this normalized value.
    #[serde(default = "default_audit_reason")]
    pub reason: String,
    pub data_class: DataClass,
    pub retention_class: String,
    /// Digest of the profile that was applied before this record crossed the audit boundary.
    #[serde(default = "default_audit_redaction_profile")]
    pub redaction_profile: String,
    /// Audit records never carry recoverable raw payload bytes.
    #[serde(default)]
    pub payload_recoverable: bool,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub record_digest: String,
}

fn default_audit_reason() -> String {
    "reason_unspecified".to_owned()
}

fn default_audit_redaction_profile() -> String {
    RedactionProfile::for_signal(RedactionSignal::Audit).profile_digest
}

fn ensure_audit_redacted_text(value: &str, field: &str) -> Result<(), String> {
    let profile = RedactionProfile::for_signal(RedactionSignal::Audit);
    let redacted = crate::redact_text_with_profile(&profile, value)?;
    if redacted != value {
        return Err(format!("audit_{field}_unredacted"));
    }
    Ok(())
}

impl AuditRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        audit_id: impl Into<String>,
        action_kind: AuditActionKind,
        decision: AuditDecision,
        actor_ref: impl Into<String>,
        target_kind: impl Into<String>,
        target_ref: impl Into<String>,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        authority_epoch: u64,
        data_epoch: u64,
        data_class: DataClass,
        retention_class: impl Into<String>,
    ) -> Result<Self, String> {
        let mut record = Self {
            schema: AUDIT_RECORD_SCHEMA.to_owned(),
            version: AUDIT_RECORD_SCHEMA_VERSION,
            audit_id: audit_id.into(),
            action_kind,
            decision,
            actor_ref: actor_ref.into(),
            target_kind: target_kind.into(),
            target_ref: target_ref.into(),
            source_cursor,
            source_event_ids,
            authority_epoch,
            data_epoch,
            request_id: None,
            command_id: None,
            correlation_ref: None,
            causation_ref: None,
            action_digest: None,
            input_digest: None,
            reason_code: None,
            reason: default_audit_reason(),
            data_class,
            retention_class: retention_class.into(),
            redaction_profile: default_audit_redaction_profile(),
            payload_recoverable: false,
            attributes: BTreeMap::new(),
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            AUDIT_RECORD_SCHEMA,
            AUDIT_RECORD_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.audit_id, "audit_id", 128)?;
        validate_nonempty(&self.actor_ref, "audit_actor_ref", 256)?;
        validate_nonempty(&self.target_kind, "audit_target_kind", 128)?;
        validate_nonempty(&self.target_ref, "audit_target_ref", 512)?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.authority_epoch == 0 || self.data_epoch == 0 {
            return Err("audit_epoch_required".to_owned());
        }
        validate_nonempty(&self.retention_class, "audit_retention_class", 64)?;
        validate_attributes(&self.attributes)?;
        for (digest, field) in [
            (self.action_digest.as_deref(), "audit_action_digest"),
            (self.input_digest.as_deref(), "audit_input_digest"),
        ] {
            if let Some(digest) = digest {
                validate_digest(digest, field)?;
            }
        }
        if let Some(reason) = &self.reason_code {
            validate_nonempty(reason, "audit_reason_code", 128)?;
            ensure_audit_redacted_text(reason, "reason_code")?;
        }
        validate_nonempty(&self.reason, "audit_reason", 128)?;
        ensure_audit_redacted_text(&self.reason, "reason")?;
        let profile = RedactionProfile::for_signal(RedactionSignal::Audit);
        if self.redaction_profile != profile.profile_digest {
            return Err("audit_redaction_profile_invalid".to_owned());
        }
        if self.payload_recoverable {
            return Err("audit_payload_recoverable".to_owned());
        }
        if let Some(correlation) = &self.correlation_ref {
            validate_nonempty(correlation, "audit_correlation_ref", 256)?;
            ensure_audit_redacted_text(correlation, "correlation_ref")?;
        }
        if let Some(causation) = &self.causation_ref {
            validate_nonempty(causation, "audit_causation_ref", 256)?;
            ensure_audit_redacted_text(causation, "causation_ref")?;
        }
        ensure_audit_redacted_text(&self.actor_ref, "actor_ref")?;
        ensure_audit_redacted_text(&self.target_kind, "target_kind")?;
        ensure_audit_redacted_text(&self.target_ref, "target_ref")?;
        ensure_audit_redacted_text(&self.retention_class, "retention_class")?;
        for (key, value) in &self.attributes {
            ensure_audit_redacted_text(key, "attribute_key")?;
            ensure_audit_redacted_text(value, "attribute_value")?;
        }
        validate_digest(&self.record_digest, "audit_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("audit_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "record_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

/// Versioned append-only EventLog envelope for a server-derived AuditRecord.
///
/// The envelope repeats source cursor and redaction metadata deliberately: an EventStore adapter
/// can reject a forged or partially copied record before it mutates storage. Corrections must be
/// represented by a later envelope/event; this contract has no update operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditRecordEvent {
    pub schema: String,
    pub version: SchemaVersion,
    pub event_kind: String,
    pub append_only: bool,
    pub record: AuditRecord,
    pub source_cursor: EventCursor,
    pub source_event_ids: Vec<EventId>,
    pub redaction_profile: String,
    pub event_digest: String,
}

impl AuditRecordEvent {
    pub fn new(record: AuditRecord) -> Result<Self, String> {
        record.validate()?;
        let mut event = Self {
            schema: AUDIT_EVENT_SCHEMA.to_owned(),
            version: AUDIT_EVENT_SCHEMA_VERSION,
            event_kind: AUDIT_EVENT_KIND.to_owned(),
            append_only: true,
            source_cursor: record.source_cursor,
            source_event_ids: record.source_event_ids.clone(),
            redaction_profile: record.redaction_profile.clone(),
            record,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_EVENT_SCHEMA {
            return Err("audit_event_schema_mismatch".to_owned());
        }
        if !AUDIT_EVENT_SCHEMA_VERSION.is_compatible_with(&self.version) {
            return Err("audit_event_schema_version_incompatible".to_owned());
        }
        if self.event_kind != AUDIT_EVENT_KIND {
            return Err("audit_event_kind_invalid".to_owned());
        }
        if !self.append_only {
            return Err("audit_event_append_only_required".to_owned());
        }
        self.record.validate()?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.source_cursor != self.record.source_cursor
            || self.source_event_ids != self.record.source_event_ids
        {
            return Err("audit_event_source_binding_mismatch".to_owned());
        }
        if self.redaction_profile != self.record.redaction_profile {
            return Err("audit_event_redaction_binding_mismatch".to_owned());
        }
        validate_digest(&self.event_digest, "audit_event_digest")?;
        if self.event_digest != self.digest() {
            return Err("audit_event_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "event_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }

    /// Build the only RuntimeEvent shape accepted by the EventLog audit boundary.
    pub fn into_runtime_event(
        &self,
        request_id: RequestId,
        sequence: u64,
    ) -> Result<RuntimeEvent, String> {
        self.validate()?;
        let data =
            serde_json::to_value(self).map_err(|_| "audit_event_encode_failed".to_owned())?;
        RuntimeEvent::new(request_id, sequence, AUDIT_EVENT_KIND, data)
            .map(|event| {
                event
                    .with_redaction_metadata(
                        self.redaction_profile.clone(),
                        false,
                        Some(self.record.data_epoch),
                        Vec::new(),
                    )
                    .with_idempotency_key(format!("audit-record:{}", self.record.audit_id))
            })
            .map_err(|error| error.to_string())
    }
}

/// Checkpoint metadata for an append-only audit projection. It can be persisted by a higher
/// layer, but the EventLog remains the source of truth and is never rewritten by this object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionCheckpoint {
    pub schema: String,
    pub version: SchemaVersion,
    pub projection_version: u64,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub record_ids: Vec<String>,
    pub records_digest: String,
    pub checkpoint_digest: String,
}

impl AuditProjectionCheckpoint {
    pub fn new(
        projection_version: u64,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        record_ids: Vec<String>,
        records_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut checkpoint = Self {
            schema: AUDIT_PROJECTION_CHECKPOINT_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_CHECKPOINT_SCHEMA_VERSION,
            projection_version,
            source_cursor,
            source_event_ids,
            record_ids,
            records_digest: records_digest.into(),
            checkpoint_digest: String::new(),
        };
        checkpoint.checkpoint_digest = checkpoint.digest();
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            AUDIT_PROJECTION_CHECKPOINT_SCHEMA,
            AUDIT_PROJECTION_CHECKPOINT_SCHEMA_VERSION,
        )?;
        if self.projection_version == 0 {
            return Err("audit_projection_version_required".to_owned());
        }
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.record_ids.len() > MAX_AUDIT_PROJECTION_RECORDS {
            return Err("audit_projection_record_limit".to_owned());
        }
        let mut ids = BTreeSet::new();
        for record_id in &self.record_ids {
            validate_nonempty(record_id, "audit_projection_record_id", 256)?;
            if !ids.insert(record_id) {
                return Err("audit_projection_record_duplicate".to_owned());
            }
        }
        validate_digest(&self.records_digest, "audit_projection_records_digest")?;
        validate_digest(
            &self.checkpoint_digest,
            "audit_projection_checkpoint_digest",
        )?;
        if self.checkpoint_digest != self.digest() {
            return Err("audit_projection_checkpoint_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "checkpoint_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

/// Complete audit projection plus its verifiable checkpoint and bounded limitations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub projection_version: u64,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub records: Vec<AuditRecord>,
    pub checkpoint: AuditProjectionCheckpoint,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub projection_digest: String,
}

impl AuditProjectionSnapshot {
    pub fn new(
        projection_version: u64,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        records: Vec<AuditRecord>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let record_ids = records
            .iter()
            .map(|record| record.audit_id.clone())
            .collect::<Vec<_>>();
        let records_digest = json_digest(
            &serde_json::to_value(&records)
                .map_err(|_| "audit_projection_records_encode_failed".to_owned())?,
        );
        let checkpoint = AuditProjectionCheckpoint::new(
            projection_version,
            source_cursor,
            source_event_ids.clone(),
            record_ids,
            records_digest,
        )?;
        let mut snapshot = Self {
            schema: AUDIT_PROJECTION_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_SCHEMA_VERSION,
            projection_version,
            source_cursor,
            source_event_ids,
            records,
            checkpoint,
            limitations,
            projection_digest: String::new(),
        };
        snapshot.projection_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            AUDIT_PROJECTION_SCHEMA,
            AUDIT_PROJECTION_SCHEMA_VERSION,
        )?;
        if self.projection_version == 0 {
            return Err("audit_projection_version_required".to_owned());
        }
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.records.len() > MAX_AUDIT_PROJECTION_RECORDS {
            return Err("audit_projection_record_limit".to_owned());
        }
        let mut record_ids = BTreeSet::new();
        let mut record_ids_in_order = Vec::with_capacity(self.records.len());
        for record in &self.records {
            record.validate()?;
            if record.source_cursor > self.source_cursor {
                return Err("audit_projection_record_cursor_ahead".to_owned());
            }
            if !record_ids.insert(record.audit_id.clone()) {
                return Err("audit_projection_record_duplicate".to_owned());
            }
            record_ids_in_order.push(record.audit_id.clone());
        }
        self.checkpoint.validate()?;
        if self.checkpoint.projection_version != self.projection_version
            || self.checkpoint.source_cursor != self.source_cursor
            || self.checkpoint.source_event_ids != self.source_event_ids
            || self.checkpoint.record_ids != record_ids_in_order
        {
            return Err("audit_projection_checkpoint_binding_mismatch".to_owned());
        }
        let records_digest = json_digest(
            &serde_json::to_value(&self.records)
                .map_err(|_| "audit_projection_records_encode_failed".to_owned())?,
        );
        if self.checkpoint.records_digest != records_digest {
            return Err("audit_projection_records_digest_mismatch".to_owned());
        }
        if self.limitations.len() > MAX_HEALTH_LIMITATIONS {
            return Err("audit_projection_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            validate_nonempty(
                limitation,
                "audit_projection_limitation",
                MAX_ATTRIBUTE_VALUE_BYTES,
            )?;
        }
        validate_digest(&self.projection_digest, "audit_projection_digest")?;
        if self.projection_digest != self.digest() {
            return Err("audit_projection_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "projection_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

/// A page cursor bound to one immutable audit projection snapshot and filter digest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditQueryCursor {
    pub schema: String,
    pub version: SchemaVersion,
    pub epoch: String,
    pub projection_version: u64,
    pub source_cursor: u64,
    pub after_cursor: u64,
    pub filter_digest: String,
    pub cursor_digest: String,
}

impl AuditQueryCursor {
    pub fn new(
        epoch: impl Into<String>,
        projection_version: u64,
        source_cursor: u64,
        after_cursor: u64,
        filter_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut cursor = Self {
            schema: AUDIT_QUERY_CURSOR_SCHEMA.to_owned(),
            version: AUDIT_QUERY_CURSOR_SCHEMA_VERSION,
            epoch: epoch.into(),
            projection_version,
            source_cursor,
            after_cursor,
            filter_digest: filter_digest.into(),
            cursor_digest: String::new(),
        };
        cursor.cursor_digest = cursor.digest();
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            AUDIT_QUERY_CURSOR_SCHEMA,
            AUDIT_QUERY_CURSOR_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.epoch, "audit_query_epoch", 128)?;
        if self.projection_version == 0 || self.source_cursor == 0 {
            return Err("audit_query_cursor_metadata_required".to_owned());
        }
        if self.after_cursor > self.source_cursor {
            return Err("audit_query_cursor_out_of_range".to_owned());
        }
        validate_digest(&self.filter_digest, "audit_query_filter_digest")?;
        validate_digest(&self.cursor_digest, "audit_query_cursor_digest")?;
        if self.cursor_digest != self.digest() {
            return Err("audit_query_cursor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "cursor_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditExportFormat {
    Jsonl,
    Json,
    Csv,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditExportManifest {
    pub schema: String,
    pub version: SchemaVersion,
    pub export_id: String,
    pub query_digest: String,
    pub source_cursor: u64,
    pub projection_version: u64,
    pub record_count: u32,
    pub format: AuditExportFormat,
    pub purpose: String,
    pub recipient: String,
    pub retention_class: String,
    pub artifact_digest: String,
    pub source_event_ids: Vec<EventId>,
    pub manifest_digest: String,
}

impl AuditExportManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        export_id: impl Into<String>,
        query_digest: impl Into<String>,
        source_cursor: u64,
        projection_version: u64,
        record_count: u32,
        format: AuditExportFormat,
        purpose: impl Into<String>,
        recipient: impl Into<String>,
        retention_class: impl Into<String>,
        artifact_digest: impl Into<String>,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        let mut manifest = Self {
            schema: AUDIT_EXPORT_SCHEMA.to_owned(),
            version: AUDIT_EXPORT_SCHEMA_VERSION,
            export_id: export_id.into(),
            query_digest: query_digest.into(),
            source_cursor,
            projection_version,
            record_count,
            format,
            purpose: purpose.into(),
            recipient: recipient.into(),
            retention_class: retention_class.into(),
            artifact_digest: artifact_digest.into(),
            source_event_ids,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            AUDIT_EXPORT_SCHEMA,
            AUDIT_EXPORT_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.export_id, "audit_export_id", 128)?;
        validate_digest(&self.query_digest, "audit_export_query_digest")?;
        if self.source_cursor == 0 || self.projection_version == 0 {
            return Err("audit_export_cursor_required".to_owned());
        }
        if usize::try_from(self.record_count).unwrap_or(usize::MAX) > MAX_AUDIT_EXPORT_RECORDS {
            return Err("audit_export_record_limit".to_owned());
        }
        validate_nonempty(&self.purpose, "audit_export_purpose", 256)?;
        validate_nonempty(&self.recipient, "audit_export_recipient", 256)?;
        validate_nonempty(&self.retention_class, "audit_export_retention_class", 64)?;
        validate_digest(&self.artifact_digest, "audit_export_artifact_digest")?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        validate_digest(&self.manifest_digest, "audit_export_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("audit_export_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "manifest_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDeliveryState {
    Delivered,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditDeliveryReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub delivery_id: String,
    pub export_id: String,
    pub manifest_digest: String,
    pub artifact_digest: String,
    pub recipient: String,
    pub state: AuditDeliveryState,
    #[serde(default)]
    pub confirmation_digest: Option<String>,
    pub source_cursor: u64,
    pub delivery_digest: String,
}

impl AuditDeliveryReceipt {
    pub fn new(
        delivery_id: impl Into<String>,
        export_id: impl Into<String>,
        manifest_digest: impl Into<String>,
        artifact_digest: impl Into<String>,
        recipient: impl Into<String>,
        state: AuditDeliveryState,
        confirmation_digest: Option<String>,
        source_cursor: u64,
    ) -> Result<Self, String> {
        let mut receipt = Self {
            schema: AUDIT_DELIVERY_RECEIPT_SCHEMA.to_owned(),
            version: AUDIT_DELIVERY_RECEIPT_SCHEMA_VERSION,
            delivery_id: delivery_id.into(),
            export_id: export_id.into(),
            manifest_digest: manifest_digest.into(),
            artifact_digest: artifact_digest.into(),
            recipient: recipient.into(),
            state,
            confirmation_digest,
            source_cursor,
            delivery_digest: String::new(),
        };
        receipt.delivery_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            AUDIT_DELIVERY_RECEIPT_SCHEMA,
            AUDIT_DELIVERY_RECEIPT_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.delivery_id, "audit_delivery_id", 128)?;
        validate_nonempty(&self.export_id, "audit_delivery_export_id", 128)?;
        validate_digest(&self.manifest_digest, "audit_delivery_manifest_digest")?;
        validate_digest(&self.artifact_digest, "audit_delivery_artifact_digest")?;
        validate_nonempty(&self.recipient, "audit_delivery_recipient", 256)?;
        if self.source_cursor == 0 {
            return Err("audit_delivery_cursor_required".to_owned());
        }
        if let Some(confirmation) = &self.confirmation_digest {
            validate_digest(confirmation, "audit_delivery_confirmation_digest")?;
        }
        if self.state == AuditDeliveryState::Delivered && self.confirmation_digest.is_none() {
            return Err("audit_delivery_confirmation_required".to_owned());
        }
        if self.state != AuditDeliveryState::Delivered && self.confirmation_digest.is_some() {
            return Err("audit_delivery_confirmation_state_conflict".to_owned());
        }
        validate_digest(&self.delivery_digest, "audit_delivery_digest")?;
        if self.delivery_digest != self.digest() {
            return Err("audit_delivery_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "delivery_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertState {
    Open,
    Acknowledged,
    Suppressed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityIncidentCategory {
    ProjectorLag,
    AuditLoss,
    RedactionFailure,
    QueueOverflow,
    JournalCorruption,
    EffectUnknown,
    OrphanDispatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityIncidentState {
    Open,
    Contained,
    Recovering,
    Verified,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityAlert {
    pub schema: String,
    pub version: SchemaVersion,
    pub alert_id: String,
    pub fingerprint: String,
    pub rule: String,
    pub severity: AlertSeverity,
    pub state: AlertState,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub message: String,
    #[serde(default)]
    pub incident_id: Option<String>,
    pub alert_digest: String,
}

impl ObservabilityAlert {
    pub fn new(
        alert_id: impl Into<String>,
        fingerprint: impl Into<String>,
        rule: impl Into<String>,
        severity: AlertSeverity,
        state: AlertState,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        message: impl Into<String>,
        incident_id: Option<String>,
    ) -> Result<Self, String> {
        let mut alert = Self {
            schema: OBSERVABILITY_ALERT_SCHEMA.to_owned(),
            version: OBSERVABILITY_ALERT_SCHEMA_VERSION,
            alert_id: alert_id.into(),
            fingerprint: fingerprint.into(),
            rule: rule.into(),
            severity,
            state,
            source_cursor,
            source_event_ids,
            message: message.into(),
            incident_id,
            alert_digest: String::new(),
        };
        alert.alert_digest = alert.digest();
        alert.validate()?;
        Ok(alert)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            OBSERVABILITY_ALERT_SCHEMA,
            OBSERVABILITY_ALERT_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.alert_id, "observability_alert_id", 128)?;
        validate_digest(&self.fingerprint, "observability_alert_fingerprint")?;
        validate_nonempty(&self.rule, "observability_alert_rule", 128)?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        validate_nonempty(
            &self.message,
            "observability_alert_message",
            MAX_ATTRIBUTE_VALUE_BYTES,
        )?;
        if let Some(incident_id) = &self.incident_id {
            validate_nonempty(incident_id, "observability_alert_incident_id", 128)?;
        }
        validate_digest(&self.alert_digest, "observability_alert_digest")?;
        if self.alert_digest != self.digest() {
            return Err("observability_alert_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "alert_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityIncident {
    pub schema: String,
    pub version: SchemaVersion,
    pub incident_id: String,
    pub fingerprint: String,
    pub category: ObservabilityIncidentCategory,
    pub severity: AlertSeverity,
    pub state: ObservabilityIncidentState,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub requires_reconciliation: bool,
    pub recovery_plan: Vec<String>,
    pub alert_ids: Vec<String>,
    pub incident_digest: String,
}

impl ObservabilityIncident {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        incident_id: impl Into<String>,
        fingerprint: impl Into<String>,
        category: ObservabilityIncidentCategory,
        severity: AlertSeverity,
        state: ObservabilityIncidentState,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        requires_reconciliation: bool,
        recovery_plan: Vec<String>,
        alert_ids: Vec<String>,
    ) -> Result<Self, String> {
        let mut incident = Self {
            schema: OBSERVABILITY_INCIDENT_SCHEMA.to_owned(),
            version: OBSERVABILITY_INCIDENT_SCHEMA_VERSION,
            incident_id: incident_id.into(),
            fingerprint: fingerprint.into(),
            category,
            severity,
            state,
            source_cursor,
            source_event_ids,
            requires_reconciliation,
            recovery_plan,
            alert_ids,
            incident_digest: String::new(),
        };
        incident.incident_digest = incident.digest();
        incident.validate()?;
        Ok(incident)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            OBSERVABILITY_INCIDENT_SCHEMA,
            OBSERVABILITY_INCIDENT_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.incident_id, "observability_incident_id", 128)?;
        validate_digest(&self.fingerprint, "observability_incident_fingerprint")?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.recovery_plan.len() > MAX_HEALTH_LIMITATIONS {
            return Err("observability_incident_recovery_plan_limit".to_owned());
        }
        for step in &self.recovery_plan {
            validate_nonempty(
                step,
                "observability_incident_recovery_step",
                MAX_ATTRIBUTE_VALUE_BYTES,
            )?;
        }
        if self.requires_reconciliation && self.recovery_plan.is_empty() {
            return Err("observability_incident_recovery_plan_required".to_owned());
        }
        if self.requires_reconciliation
            && matches!(
                self.state,
                ObservabilityIncidentState::Verified | ObservabilityIncidentState::Closed
            )
        {
            return Err("observability_incident_unknown_cannot_close".to_owned());
        }
        if self.alert_ids.len() > MAX_OBSERVABILITY_ALERTS {
            return Err("observability_incident_alert_limit".to_owned());
        }
        let mut alert_ids = BTreeSet::new();
        for alert_id in &self.alert_ids {
            validate_nonempty(alert_id, "observability_incident_alert_id", 128)?;
            if !alert_ids.insert(alert_id) {
                return Err("observability_incident_alert_duplicate".to_owned());
            }
        }
        validate_digest(&self.incident_digest, "observability_incident_digest")?;
        if self.incident_digest != self.digest() {
            return Err("observability_incident_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "incident_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityIncidentSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub alerts: Vec<ObservabilityAlert>,
    pub incidents: Vec<ObservabilityIncident>,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub snapshot_digest: String,
}

impl ObservabilityIncidentSnapshot {
    pub fn new(
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        alerts: Vec<ObservabilityAlert>,
        incidents: Vec<ObservabilityIncident>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: OBSERVABILITY_INCIDENT_SNAPSHOT_SCHEMA.to_owned(),
            version: OBSERVABILITY_INCIDENT_SNAPSHOT_SCHEMA_VERSION,
            source_cursor,
            source_event_ids,
            alerts,
            incidents,
            limitations,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            OBSERVABILITY_INCIDENT_SNAPSHOT_SCHEMA,
            OBSERVABILITY_INCIDENT_SNAPSHOT_SCHEMA_VERSION,
        )?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.alerts.len() > MAX_OBSERVABILITY_ALERTS {
            return Err("observability_snapshot_alert_limit".to_owned());
        }
        if self.incidents.len() > MAX_OBSERVABILITY_INCIDENTS {
            return Err("observability_snapshot_incident_limit".to_owned());
        }
        let mut alert_ids = BTreeSet::new();
        for alert in &self.alerts {
            alert.validate()?;
            if !alert_ids.insert(alert.alert_id.clone()) {
                return Err("observability_snapshot_alert_duplicate".to_owned());
            }
        }
        let mut incident_ids = BTreeSet::new();
        for incident in &self.incidents {
            incident.validate()?;
            if !incident_ids.insert(incident.incident_id.clone()) {
                return Err("observability_snapshot_incident_duplicate".to_owned());
            }
        }
        if self.limitations.len() > MAX_HEALTH_LIMITATIONS {
            return Err("observability_snapshot_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            validate_nonempty(
                limitation,
                "observability_snapshot_limitation",
                MAX_ATTRIBUTE_VALUE_BYTES,
            )?;
        }
        validate_digest(&self.snapshot_digest, "observability_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("observability_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "snapshot_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayDivergenceKind {
    SourceGap,
    SchemaConflict,
    TerminalConflict,
    ProjectionError,
    StatusMismatch,
    UnknownEffect,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayDiagnostic {
    pub schema: String,
    pub version: SchemaVersion,
    #[serde(default)]
    pub invocation_id: Option<InvocationId>,
    #[serde(default)]
    pub attempt: Option<u32>,
    #[serde(default)]
    pub input_digest: Option<String>,
    #[serde(default)]
    pub expected_status: Option<TraceStatus>,
    #[serde(default)]
    pub observed_status: Option<TraceStatus>,
    pub divergence: ReplayDivergenceKind,
    pub error_code: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub diagnostic_digest: String,
}

impl ReplayDiagnostic {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        invocation_id: Option<InvocationId>,
        attempt: Option<u32>,
        input_digest: Option<String>,
        expected_status: Option<TraceStatus>,
        observed_status: Option<TraceStatus>,
        divergence: ReplayDivergenceKind,
        error_code: impl Into<String>,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        let mut diagnostic = Self {
            schema: REPLAY_DIAGNOSTIC_SCHEMA.to_owned(),
            version: REPLAY_DIAGNOSTIC_SCHEMA_VERSION,
            invocation_id,
            attempt,
            input_digest,
            expected_status,
            observed_status,
            divergence,
            error_code: error_code.into(),
            source_cursor,
            source_event_ids,
            diagnostic_digest: String::new(),
        };
        diagnostic.diagnostic_digest = diagnostic.digest();
        diagnostic.validate()?;
        Ok(diagnostic)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            REPLAY_DIAGNOSTIC_SCHEMA,
            REPLAY_DIAGNOSTIC_SCHEMA_VERSION,
        )?;
        if self.attempt.is_some_and(|attempt| attempt == 0) {
            return Err("replay_attempt_invalid".to_owned());
        }
        if let Some(input_digest) = &self.input_digest {
            validate_digest(input_digest, "replay_input_digest")?;
        }
        validate_nonempty(&self.error_code, "replay_error_code", 128)?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        validate_digest(&self.diagnostic_digest, "replay_diagnostic_digest")?;
        if self.diagnostic_digest != self.digest() {
            return Err("replay_diagnostic_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "diagnostic_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayDiagnosticSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub status: TraceStatus,
    pub projection_digests: BTreeMap<String, String>,
    pub diagnostics: Vec<ReplayDiagnostic>,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub snapshot_digest: String,
}

impl ReplayDiagnosticSnapshot {
    pub fn new(
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        status: TraceStatus,
        projection_digests: BTreeMap<String, String>,
        diagnostics: Vec<ReplayDiagnostic>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: REPLAY_DIAGNOSTIC_SNAPSHOT_SCHEMA.to_owned(),
            version: REPLAY_DIAGNOSTIC_SNAPSHOT_SCHEMA_VERSION,
            source_cursor,
            source_event_ids,
            status,
            projection_digests,
            diagnostics,
            limitations,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            REPLAY_DIAGNOSTIC_SNAPSHOT_SCHEMA,
            REPLAY_DIAGNOSTIC_SNAPSHOT_SCHEMA_VERSION,
        )?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.diagnostics.len() > MAX_REPLAY_DIAGNOSTICS {
            return Err("replay_diagnostic_limit".to_owned());
        }
        for diagnostic in &self.diagnostics {
            diagnostic.validate()?;
            if diagnostic.source_cursor > self.source_cursor {
                return Err("replay_diagnostic_cursor_ahead".to_owned());
            }
        }
        if self.projection_digests.len() > MAX_SIGNAL_ATTRIBUTES {
            return Err("replay_projection_digest_limit".to_owned());
        }
        for (name, digest) in &self.projection_digests {
            validate_nonempty(name, "replay_projection_name", 128)?;
            validate_digest(digest, "replay_projection_digest")?;
        }
        if self.limitations.len() > MAX_HEALTH_LIMITATIONS {
            return Err("replay_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            validate_nonempty(limitation, "replay_limitation", MAX_ATTRIBUTE_VALUE_BYTES)?;
        }
        validate_digest(&self.snapshot_digest, "replay_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("replay_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "snapshot_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceSummary {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: String,
    #[serde(default)]
    pub root_span_ref: Option<String>,
    pub status: TraceStatus,
    pub span_count: u32,
    pub event_count: u32,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub input_digest: Option<String>,
    #[serde(default)]
    pub output_digest: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub summary_digest: String,
}

impl TraceSummary {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trace_id: impl Into<String>,
        root_span_ref: Option<String>,
        status: TraceStatus,
        span_count: u32,
        event_count: u32,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        let mut summary = Self {
            schema: TRACE_SUMMARY_SCHEMA.to_owned(),
            version: TRACE_SUMMARY_SCHEMA_VERSION,
            trace_id: trace_id.into(),
            root_span_ref,
            status,
            span_count,
            event_count,
            source_cursor,
            source_event_ids,
            duration_ms: None,
            input_digest: None,
            output_digest: None,
            attributes: BTreeMap::new(),
            summary_digest: String::new(),
        };
        summary.summary_digest = summary.digest();
        summary.validate()?;
        Ok(summary)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            TRACE_SUMMARY_SCHEMA,
            TRACE_SUMMARY_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.trace_id, "trace_id", 256)?;
        if let Some(root) = &self.root_span_ref {
            validate_nonempty(root, "trace_root_span_ref", 256)?;
        }
        if self.span_count == 0 || self.span_count > MAX_TRACE_SPANS {
            return Err("trace_span_count_invalid".to_owned());
        }
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        validate_attributes(&self.attributes)?;
        for (digest, field) in [
            (self.input_digest.as_deref(), "trace_input_digest"),
            (self.output_digest.as_deref(), "trace_output_digest"),
        ] {
            if let Some(digest) = digest {
                validate_digest(digest, field)?;
            }
        }
        validate_digest(&self.summary_digest, "trace_summary_digest")?;
        if self.summary_digest != self.digest() {
            return Err("trace_summary_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "summary_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

/// Bounded local/export representation of one span. Foreign W3C context is retained only as an
/// optional parent ID; it never carries actor, scope or authority information.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceExportSpan {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    #[serde(default)]
    pub parent_span_id: Option<SpanId>,
    pub name: String,
    pub status: TraceStatus,
    pub sampled: bool,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub export_digest: String,
}

impl TraceExportSpan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trace_id: TraceId,
        span_id: SpanId,
        parent_span_id: Option<SpanId>,
        name: impl Into<String>,
        status: TraceStatus,
        sampled: bool,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        duration_ms: Option<u64>,
        attributes: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut span = Self {
            schema: TRACE_EXPORT_SPAN_SCHEMA.to_owned(),
            version: TRACE_EXPORT_SPAN_SCHEMA_VERSION,
            trace_id,
            span_id,
            parent_span_id,
            name: name.into(),
            status,
            sampled,
            source_cursor,
            source_event_ids,
            duration_ms,
            attributes,
            export_digest: String::new(),
        };
        span.export_digest = span.digest();
        span.validate()?;
        Ok(span)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            TRACE_EXPORT_SPAN_SCHEMA,
            TRACE_EXPORT_SPAN_SCHEMA_VERSION,
        )?;
        TraceId::parse(self.trace_id.as_str())?;
        SpanId::parse(self.span_id.as_str())?;
        if self.parent_span_id.as_ref() == Some(&self.span_id) {
            return Err("trace_export_parent_self".to_owned());
        }
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || !self.name.starts_with("kiana.")
            || !self.name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || ".-_".contains(byte as char)
            })
        {
            return Err("trace_export_name_invalid".to_owned());
        }
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        validate_attributes(&self.attributes)?;
        const ALLOWED: &[&str] = &[
            "provider_id",
            "model_id",
            "capability_id",
            "operation",
            "sandbox_profile",
            "environment",
            "component",
            "schema_version",
            "outcome",
            "reason_class",
            "retryable",
            "approval_required",
            "effect_known",
            "stop_confirmed",
        ];
        if self
            .attributes
            .keys()
            .any(|key| !ALLOWED.iter().any(|allowed| allowed == key))
        {
            return Err("trace_export_attribute_not_allowed".to_owned());
        }
        validate_digest(&self.export_digest, "trace_export_digest")?;
        if self.export_digest != self.digest() {
            return Err("trace_export_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "export_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

/// Bounded health/readiness observation returned by a server-side probe.
///
/// A health snapshot is a projection at a committed EventLog cursor. It never grants authority,
/// claims that an external provider is healthy, or substitutes for an execution receipt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    #[serde(default)]
    pub probe: HealthProbeKind,
    pub component: String,
    pub status: SignalStatus,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub observed_at_ms: u64,
    #[serde(default)]
    pub capabilities: BTreeMap<String, bool>,
    #[serde(default)]
    pub components: BTreeMap<String, ComponentHealth>,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub snapshot_digest: String,
}

/// One committed lifecycle transition for a Run, Turn or Invocation span.
///
/// This is a projection record, not an authorization token. A terminal span record can only be
/// derived from a terminal EventLog fact; ending a span never creates or changes that fact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpanLifecycleRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub entity: SpanEntityKind,
    pub run_id: RunId,
    #[serde(default)]
    pub turn_id: Option<TurnId>,
    #[serde(default)]
    pub invocation_id: Option<InvocationId>,
    #[serde(default)]
    pub execution_id: Option<ExecutionId>,
    #[serde(default)]
    pub attempt: Option<u32>,
    pub phase: SpanLifecyclePhase,
    pub status: TraceStatus,
    pub event_kind: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub record_digest: String,
}

impl SpanLifecycleRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trace_id: TraceId,
        span_id: SpanId,
        entity: SpanEntityKind,
        run_id: RunId,
        turn_id: Option<TurnId>,
        invocation_id: Option<InvocationId>,
        execution_id: Option<ExecutionId>,
        attempt: Option<u32>,
        phase: SpanLifecyclePhase,
        status: TraceStatus,
        event_kind: impl Into<String>,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        error_code: Option<String>,
    ) -> Result<Self, String> {
        let mut record = Self {
            schema: SPAN_LIFECYCLE_SCHEMA.to_owned(),
            version: SPAN_LIFECYCLE_SCHEMA_VERSION,
            trace_id,
            span_id,
            entity,
            run_id,
            turn_id,
            invocation_id,
            execution_id,
            attempt,
            phase,
            status,
            event_kind: event_kind.into(),
            source_cursor,
            source_event_ids,
            error_code,
            attributes: BTreeMap::new(),
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            SPAN_LIFECYCLE_SCHEMA,
            SPAN_LIFECYCLE_SCHEMA_VERSION,
        )?;
        TraceId::parse(self.trace_id.as_str())?;
        SpanId::parse(self.span_id.as_str())?;
        match self.entity {
            SpanEntityKind::Run => {
                if self.turn_id.is_some()
                    || self.invocation_id.is_some()
                    || self.execution_id.is_some()
                {
                    return Err("span_run_identity_conflict".to_owned());
                }
            }
            SpanEntityKind::Turn => {
                if self.turn_id.is_none() {
                    return Err("span_turn_required".to_owned());
                }
                if self.invocation_id.is_some() || self.execution_id.is_some() {
                    return Err("span_turn_identity_conflict".to_owned());
                }
            }
            SpanEntityKind::Invocation => {
                if self.turn_id.is_none() {
                    return Err("span_invocation_turn_required".to_owned());
                }
                if self.invocation_id.is_none() {
                    return Err("span_invocation_required".to_owned());
                }
            }
        }
        if self.invocation_id.is_some() && !matches!(self.entity, SpanEntityKind::Invocation) {
            return Err("span_invocation_entity_conflict".to_owned());
        }
        if self.execution_id.is_some() && self.invocation_id.is_none() {
            return Err("span_execution_invocation_required".to_owned());
        }
        if self.attempt.is_some_and(|attempt| attempt == 0) {
            return Err("span_attempt_invalid".to_owned());
        }
        validate_nonempty(&self.event_kind, "span_event_kind", 128)?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.source_event_ids.len() != 1 {
            return Err("span_source_event_required".to_owned());
        }
        if let Some(error_code) = &self.error_code {
            validate_nonempty(error_code, "span_error_code", 256)?;
        }
        validate_attributes(&self.attributes)?;
        validate_digest(&self.record_digest, "span_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("span_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "record_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

/// Provider cache information is intentionally a low-cardinality enum.  Raw cache keys,
/// provider headers and response metadata never cross the model-attempt projection boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCacheUsage {
    Hit,
    Miss,
    NotRequested,
    Unknown,
}

/// Admission evidence for one capability attempt.  `Allowed` means a committed permit was
/// issued; it does not by itself prove that a handler started or that an external effect
/// succeeded.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAdmissionState {
    Unknown,
    Allowed,
    Denied,
}

/// Approval evidence is kept separate from policy admission so an approval card cannot be
/// mistaken for a consumed one-shot grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityApprovalState {
    NotRequired,
    Pending,
    Approved,
    Denied,
    Expired,
    Cancelled,
    Unknown,
}

/// Effect evidence is deliberately conservative: `Unknown` keeps the request fenced until an
/// explicit reconciliation action appends a newer fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityEffectState {
    NotStarted,
    Started,
    Succeeded,
    Failed,
    Unknown,
}

/// Stop is an observation, not a cancellation command.  `Unconfirmed` must never be rendered as
/// a successful cancellation or released reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStopState {
    NotRequested,
    Requested,
    Confirmed,
    Unconfirmed,
    Unknown,
}

/// Probe modes have intentionally different meanings. Readiness is an admission gate; it does
/// not claim that every historical run or external provider is healthy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthProbeKind {
    Startup,
    Readiness,
    Liveness,
    Drain,
    Maintenance,
}

impl Default for HealthProbeKind {
    fn default() -> Self {
        Self::Liveness
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentHealthState {
    Healthy,
    Degraded,
    Unavailable,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentHealth {
    pub name: String,
    pub version: String,
    pub state: ComponentHealthState,
    #[serde(default)]
    pub last_success_cursor: Option<u64>,
    #[serde(default)]
    pub limitation: Option<String>,
}

impl ComponentHealth {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        state: ComponentHealthState,
        last_success_cursor: Option<u64>,
        limitation: Option<String>,
    ) -> Result<Self, String> {
        let component = Self {
            name: name.into(),
            version: version.into(),
            state,
            last_success_cursor,
            limitation,
        };
        component.validate()?;
        Ok(component)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_nonempty(&self.name, "health_component_name", 128)?;
        validate_nonempty(&self.version, "health_component_version", 64)?;
        if let Some(cursor) = self.last_success_cursor {
            if cursor == 0 {
                return Err("health_component_cursor_invalid".to_owned());
            }
        }
        if let Some(limitation) = &self.limitation {
            validate_nonempty(
                limitation,
                "health_component_limitation",
                MAX_ATTRIBUTE_VALUE_BYTES,
            )?;
        }
        Ok(())
    }
}

/// One bounded capability admission/effect observation reconstructed from committed facts.
///
/// This record is intentionally a projection.  It contains action and request digests, stable
/// IDs and low-cardinality states, but never raw tool arguments, shell commands, paths, headers,
/// secrets or handler output.  In particular, `effect_known=false` requires `fenced=true` so an
/// unknown result cannot be retried as if no effect had happened.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityAttemptRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    #[serde(default)]
    pub run_id: Option<RunId>,
    #[serde(default)]
    pub turn_id: Option<TurnId>,
    #[serde(default)]
    pub invocation_id: Option<InvocationId>,
    #[serde(default)]
    pub execution_id: Option<ExecutionId>,
    pub request_id: RequestId,
    pub attempt: u32,
    pub capability_id: String,
    pub operation: String,
    pub action_digest: String,
    pub admission: CapabilityAdmissionState,
    pub approval: CapabilityApprovalState,
    pub effect: CapabilityEffectState,
    pub stop: CapabilityStopState,
    pub effect_known: bool,
    pub stop_confirmed: Option<bool>,
    pub fenced: bool,
    pub zero_effect: bool,
    pub status: TraceStatus,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub record_digest: String,
}

impl CapabilityAttemptRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trace_id: TraceId,
        span_id: SpanId,
        run_id: Option<RunId>,
        turn_id: Option<TurnId>,
        invocation_id: Option<InvocationId>,
        execution_id: Option<ExecutionId>,
        request_id: RequestId,
        attempt: u32,
        capability_id: impl Into<String>,
        operation: impl Into<String>,
        action_digest: impl Into<String>,
        admission: CapabilityAdmissionState,
        approval: CapabilityApprovalState,
        effect: CapabilityEffectState,
        stop: CapabilityStopState,
        effect_known: bool,
        stop_confirmed: Option<bool>,
        fenced: bool,
        zero_effect: bool,
        status: TraceStatus,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        error_code: Option<String>,
        attributes: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut record = Self {
            schema: CAPABILITY_ATTEMPT_SCHEMA.to_owned(),
            version: CAPABILITY_ATTEMPT_SCHEMA_VERSION,
            trace_id,
            span_id,
            run_id,
            turn_id,
            invocation_id,
            execution_id,
            request_id,
            attempt,
            capability_id: capability_id.into(),
            operation: operation.into(),
            action_digest: action_digest.into(),
            admission,
            approval,
            effect,
            stop,
            effect_known,
            stop_confirmed,
            fenced,
            zero_effect,
            status,
            source_cursor,
            source_event_ids,
            error_code,
            attributes,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            CAPABILITY_ATTEMPT_SCHEMA,
            CAPABILITY_ATTEMPT_SCHEMA_VERSION,
        )?;
        TraceId::parse(self.trace_id.as_str())?;
        SpanId::parse(self.span_id.as_str())?;
        if self.attempt == 0 {
            return Err("capability_attempt_invalid".to_owned());
        }
        validate_nonempty(&self.capability_id, "capability_id", 128)?;
        validate_nonempty(&self.operation, "capability_operation", 128)?;
        validate_digest(&self.action_digest, "capability_action_digest")?;
        if self.effect_known != !matches!(self.effect, CapabilityEffectState::Unknown) {
            return Err("capability_effect_evidence_conflict".to_owned());
        }
        if matches!(self.effect, CapabilityEffectState::Unknown) && !self.fenced {
            return Err("capability_unknown_not_fenced".to_owned());
        }
        if self.zero_effect
            != (self.effect_known && matches!(self.effect, CapabilityEffectState::NotStarted))
        {
            return Err("capability_zero_effect_evidence_conflict".to_owned());
        }
        match self.stop {
            CapabilityStopState::Confirmed if self.stop_confirmed != Some(true) => {
                return Err("capability_stop_confirmation_conflict".to_owned())
            }
            CapabilityStopState::Unconfirmed if self.stop_confirmed != Some(false) => {
                return Err("capability_stop_confirmation_conflict".to_owned())
            }
            CapabilityStopState::NotRequested if self.stop_confirmed.is_some() => {
                return Err("capability_stop_confirmation_conflict".to_owned())
            }
            _ => {}
        }
        if self.status == TraceStatus::Ok
            && (self.admission != CapabilityAdmissionState::Allowed
                || self.effect != CapabilityEffectState::Succeeded
                || !self.effect_known
                || self.zero_effect
                || matches!(
                    self.stop,
                    CapabilityStopState::Unconfirmed | CapabilityStopState::Unknown
                )
                || self.error_code.is_some())
        {
            return Err("capability_attempt_ok_without_complete_evidence".to_owned());
        }
        if self.admission == CapabilityAdmissionState::Denied
            && (!self.zero_effect || self.effect != CapabilityEffectState::NotStarted)
        {
            return Err("capability_denied_with_effect_evidence".to_owned());
        }
        if let Some(error_code) = &self.error_code {
            validate_nonempty(error_code, "capability_error_code", 128)?;
        }
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        validate_attributes(&self.attributes)?;
        validate_digest(&self.record_digest, "capability_attempt_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("capability_attempt_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "record_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

fn validate_prompt_version(value: &str) -> Result<(), String> {
    if value.len() > 128 {
        return Err("model_prompt_version_too_long".to_owned());
    }
    if let Some(hex) = value.strip_prefix("sha256:") {
        if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(());
        }
    }
    if let Some(hex) = value.strip_prefix("fnv1a64:") {
        if hex.len() == 16 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(());
        }
    }
    Err("model_prompt_version_invalid".to_owned())
}

/// One bounded provider/model attempt observation derived from a committed `run.model_turn`
/// event.  It is a projection record, not a provider receipt or a billing proof.
///
/// The record deliberately carries only hashes and low-cardinality classifications for prompt,
/// route, cache and error data.  In particular, prompt text, authentication headers and raw
/// provider responses are not representable in this contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttemptRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub run_id: RunId,
    #[serde(default)]
    pub turn_id: Option<TurnId>,
    #[serde(default)]
    pub step_id: Option<StepId>,
    #[serde(default)]
    pub model_attempt_id: Option<ModelAttemptId>,
    pub model_call_id: RequestId,
    pub model_request_id: RequestId,
    pub attempt: u32,
    pub provider_id: String,
    pub model_id: String,
    pub route_digest: String,
    #[serde(default)]
    pub prompt_version: Option<String>,
    pub purpose: ModelPurpose,
    #[serde(default)]
    pub streaming: Option<bool>,
    pub attempted: bool,
    pub status: TraceStatus,
    #[serde(default)]
    pub stop_reason: Option<ModelFinish>,
    #[serde(default)]
    pub usage: Option<ModelUsage>,
    pub usage_complete: bool,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub retry_class: Option<ModelRetryClass>,
    #[serde(default)]
    pub cache_usage: Option<ModelCacheUsage>,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub record_digest: String,
}

impl ModelAttemptRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trace_id: TraceId,
        span_id: SpanId,
        run_id: RunId,
        turn_id: Option<TurnId>,
        model_call_id: RequestId,
        model_request_id: RequestId,
        attempt: u32,
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        route_digest: impl Into<String>,
        prompt_version: Option<String>,
        purpose: ModelPurpose,
        streaming: Option<bool>,
        attempted: bool,
        status: TraceStatus,
        stop_reason: Option<ModelFinish>,
        usage: Option<ModelUsage>,
        usage_complete: bool,
        latency_ms: Option<u64>,
        retry_class: Option<ModelRetryClass>,
        cache_usage: Option<ModelCacheUsage>,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        error_code: Option<String>,
        attributes: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut record = Self {
            schema: MODEL_ATTEMPT_SCHEMA.to_owned(),
            version: MODEL_ATTEMPT_SCHEMA_VERSION,
            trace_id,
            span_id,
            run_id,
            turn_id,
            step_id: None,
            model_attempt_id: None,
            model_call_id,
            model_request_id,
            attempt,
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            route_digest: route_digest.into(),
            prompt_version,
            purpose,
            streaming,
            attempted,
            status,
            stop_reason,
            usage,
            usage_complete,
            latency_ms,
            retry_class,
            cache_usage,
            source_cursor,
            source_event_ids,
            error_code,
            attributes,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            MODEL_ATTEMPT_SCHEMA,
            MODEL_ATTEMPT_SCHEMA_VERSION,
        )?;
        TraceId::parse(self.trace_id.as_str())?;
        SpanId::parse(self.span_id.as_str())?;
        if self.attempt == 0 {
            return Err("model_attempt_invalid".to_owned());
        }
        validate_nonempty(
            &self.provider_id,
            "model_provider_id",
            MAX_MODEL_PROVIDER_BYTES,
        )?;
        validate_nonempty(&self.model_id, "model_id", MAX_MODEL_ID_BYTES)?;
        validate_digest(&self.route_digest, "model_route_digest")?;
        if let Some(prompt_version) = &self.prompt_version {
            validate_prompt_version(prompt_version)?;
        }
        if self.usage_complete && self.usage.is_none() {
            return Err("model_usage_completion_without_usage".to_owned());
        }
        if let Some(usage) = &self.usage {
            if usage.input_tokens > MAX_MODEL_USAGE_TOKENS
                || usage.output_tokens > MAX_MODEL_USAGE_TOKENS
                || usage
                    .input_tokens
                    .checked_add(usage.output_tokens)
                    .is_none()
            {
                return Err("model_usage_out_of_bounds".to_owned());
            }
        }
        if let Some(latency_ms) = self.latency_ms {
            if latency_ms > 86_400_000 {
                return Err("model_latency_out_of_bounds".to_owned());
            }
        }
        if let Some(error_code) = &self.error_code {
            validate_nonempty(error_code, "model_error_code", MAX_MODEL_ERROR_CODE_BYTES)?;
        }
        if self.status == TraceStatus::Ok
            && (!self.attempted
                || !self.usage_complete
                || self.error_code.is_some()
                || self
                    .retry_class
                    .is_some_and(|class| class != ModelRetryClass::Never)
                || !matches!(
                    self.stop_reason,
                    Some(ModelFinish::EndTurn | ModelFinish::ToolUse)
                ))
        {
            return Err("model_attempt_ok_without_complete_evidence".to_owned());
        }
        if matches!(
            self.stop_reason,
            Some(ModelFinish::Length | ModelFinish::Incomplete)
        ) && self.status == TraceStatus::Ok
        {
            return Err("model_attempt_truncated_as_ok".to_owned());
        }
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.source_event_ids.len() != 1 {
            return Err("model_attempt_source_event_required".to_owned());
        }
        validate_attributes(&self.attributes)?;
        validate_digest(&self.record_digest, "model_attempt_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("model_attempt_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Attach server-owned Step/ModelAttempt identity after constructing a legacy-compatible
    /// record. Recompute the record digest so the optional fields are covered by the projection
    /// contract.
    pub fn with_identity(
        mut self,
        step_id: Option<StepId>,
        model_attempt_id: Option<ModelAttemptId>,
    ) -> Result<Self, String> {
        self.step_id = step_id;
        self.model_attempt_id = model_attempt_id;
        self.record_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "record_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

impl HealthSnapshot {
    pub fn new(
        component: impl Into<String>,
        status: SignalStatus,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        observed_at_ms: u64,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: HEALTH_SNAPSHOT_SCHEMA.to_owned(),
            version: HEALTH_SNAPSHOT_SCHEMA_VERSION,
            probe: HealthProbeKind::Liveness,
            component: component.into(),
            status,
            source_cursor,
            source_event_ids,
            observed_at_ms,
            capabilities: BTreeMap::new(),
            components: BTreeMap::new(),
            limitations: Vec::new(),
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            HEALTH_SNAPSHOT_SCHEMA,
            HEALTH_SNAPSHOT_SCHEMA_VERSION,
        )?;
        validate_nonempty(&self.component, "health_component", 128)?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.observed_at_ms == 0 {
            return Err("health_observed_at_required".to_owned());
        }
        if self.capabilities.len() > MAX_HEALTH_CAPABILITIES {
            return Err("health_capability_limit".to_owned());
        }
        for key in self.capabilities.keys() {
            validate_nonempty(key, "health_capability", MAX_ATTRIBUTE_KEY_BYTES)?;
        }
        if self.components.len() > MAX_HEALTH_CAPABILITIES {
            return Err("health_component_limit".to_owned());
        }
        for (key, component) in &self.components {
            validate_nonempty(key, "health_component_key", MAX_ATTRIBUTE_KEY_BYTES)?;
            component.validate()?;
            if key != &component.name {
                return Err("health_component_key_mismatch".to_owned());
            }
        }
        if self.limitations.len() > MAX_HEALTH_LIMITATIONS {
            return Err("health_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            validate_nonempty(limitation, "health_limitation", MAX_ATTRIBUTE_VALUE_BYTES)?;
        }
        validate_digest(&self.snapshot_digest, "health_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("health_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "snapshot_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}

/// A bounded operator-facing evidence projection assembled from committed facts.
///
/// This contract intentionally carries measurements and limitations, never an effect outcome.
/// In particular, `effect_success_claim` is permanently false: a metric or health observation
/// cannot turn an invocation with an unknown effect into a success. Correlation and causation
/// references are metadata-only and are checked by the same secret sentinel as other telemetry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorEvidenceSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: SignalStatus,
    pub health_status: SignalStatus,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub metric_snapshot_digest: String,
    pub health_snapshot_digest: String,
    #[serde(default)]
    pub trace_correlation_digests: Vec<String>,
    #[serde(default)]
    pub append_latency_ms: Option<u64>,
    #[serde(default)]
    pub flush_latency_ms: Option<u64>,
    #[serde(default)]
    pub projector_latency_ms: Option<u64>,
    #[serde(default)]
    pub recovery_latency_ms: Option<u64>,
    #[serde(default)]
    pub queue_depth: Option<u64>,
    pub last_durable_cursor: u64,
    pub unknown_count: u64,
    pub orphan_count: u64,
    pub stop_unconfirmed_count: u64,
    pub artifact_bytes: u64,
    #[serde(default)]
    pub correlation_refs: Vec<String>,
    #[serde(default)]
    pub causation_refs: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
    /// Always false. This field is explicit so consumers cannot infer effect success from health.
    pub effect_success_claim: bool,
    pub evidence_digest: String,
}

impl OperatorEvidenceSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status: SignalStatus,
        health_status: SignalStatus,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        metric_snapshot_digest: impl Into<String>,
        health_snapshot_digest: impl Into<String>,
        trace_correlation_digests: Vec<String>,
        append_latency_ms: Option<u64>,
        flush_latency_ms: Option<u64>,
        projector_latency_ms: Option<u64>,
        recovery_latency_ms: Option<u64>,
        queue_depth: Option<u64>,
        last_durable_cursor: u64,
        unknown_count: u64,
        orphan_count: u64,
        stop_unconfirmed_count: u64,
        artifact_bytes: u64,
        correlation_refs: Vec<String>,
        causation_refs: Vec<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: OPERATOR_EVIDENCE_SCHEMA.to_owned(),
            version: OPERATOR_EVIDENCE_SCHEMA_VERSION,
            status,
            health_status,
            source_cursor,
            source_event_ids,
            metric_snapshot_digest: metric_snapshot_digest.into(),
            health_snapshot_digest: health_snapshot_digest.into(),
            trace_correlation_digests,
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
            correlation_refs,
            causation_refs,
            limitations,
            effect_success_claim: false,
            evidence_digest: String::new(),
        };
        snapshot.evidence_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn validate_ref_list(values: &[String], field: &str) -> Result<(), String> {
        if values.len() > MAX_OPERATOR_EVIDENCE_REFS {
            return Err(format!("{field}_limit"));
        }
        let mut unique = BTreeSet::new();
        for value in values {
            validate_nonempty(value, field, MAX_ATTRIBUTE_VALUE_BYTES)?;
            crate::validate_secret_free_text(value, field)
                .map_err(|_| "telemetry_secret_scan_blocks_publish".to_owned())?;
            if !unique.insert(value) {
                return Err(format!("{field}_duplicate"));
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_header(
            &self.schema,
            self.version,
            OPERATOR_EVIDENCE_SCHEMA,
            OPERATOR_EVIDENCE_SCHEMA_VERSION,
        )?;
        validate_cursor(self.source_cursor, &self.source_event_ids)?;
        if self.last_durable_cursor == 0 || self.last_durable_cursor > self.source_cursor {
            return Err("operator_last_durable_cursor_invalid".to_owned());
        }
        validate_digest(
            &self.metric_snapshot_digest,
            "operator_metric_snapshot_digest",
        )?;
        validate_digest(
            &self.health_snapshot_digest,
            "operator_health_snapshot_digest",
        )?;
        if self.trace_correlation_digests.len() > MAX_OPERATOR_EVIDENCE_REFS {
            return Err("operator_trace_correlation_digest_limit".to_owned());
        }
        for digest in &self.trace_correlation_digests {
            validate_digest(digest, "operator_trace_correlation_digest")?;
        }
        if self.health_status != SignalStatus::Ok && self.status == SignalStatus::Ok {
            return Err(if self.health_status == SignalStatus::Unknown {
                "health_unknown_is_not_healthy".to_owned()
            } else {
                "operator_health_not_ok".to_owned()
            });
        }
        if self.unknown_count > 0 || self.orphan_count > 0 || self.stop_unconfirmed_count > 0 {
            if self.status == SignalStatus::Ok {
                return Err("metrics_cannot_claim_effect_success".to_owned());
            }
        }
        if self.effect_success_claim {
            return Err("metrics_cannot_claim_effect_success".to_owned());
        }
        Self::validate_ref_list(&self.correlation_refs, "operator_correlation_ref")?;
        Self::validate_ref_list(&self.causation_refs, "operator_causation_ref")?;
        if self.limitations.len() > MAX_HEALTH_LIMITATIONS {
            return Err("operator_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            validate_nonempty(limitation, "operator_limitation", MAX_ATTRIBUTE_VALUE_BYTES)?;
            crate::validate_secret_free_text(limitation, "operator_limitation")
                .map_err(|_| "telemetry_secret_scan_blocks_publish".to_owned())?;
        }
        if !self.limitations.is_empty() && self.status == SignalStatus::Ok {
            return Err("operator_limitations_require_degraded".to_owned());
        }
        validate_digest(&self.evidence_digest, "operator_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("operator_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_bytes(self)
    }

    pub fn digest(&self) -> String {
        value_without_digest(self, "evidence_digest")
            .map(|value| json_digest(&value))
            .unwrap_or_else(|_| "sha256:".to_owned())
    }
}
