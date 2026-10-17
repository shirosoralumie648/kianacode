//! Bounded trace export and W3C context adaptation.
//!
//! Export is intentionally optional and best-effort. A foreign `traceparent` is parsed into a
//! link only; server-owned trace/span IDs, actor, scope and authority remain unchanged. Foreign
//! `tracestate` and `baggage` members are classified as inert annotations: the raw value is never
//! retained, only a bounded digest, and authority-shaped keys are refused instead of being
//! promoted. Closing or sampling out this exporter never changes EventLog or Receipt facts.
//!
//! This module holds no control-plane, event-store, broker, receipt or filesystem handle. A
//! refusal, a full buffer or a closed exporter can therefore only change this exporter's own
//! bounded statistics. The OTLP mapping below is a local, lossy projection without any transport
//! or backend acknowledgement.

use kiana_domain::{
    encode_bounded_value, json_digest, RedactionProfile, RedactionSignal, SpanId, SpanLink,
    SpanLinkKind, TraceExportSpan, TraceId, TraceParent, TraceStatus, TraceSummary,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

const MAX_EXPORT_RECORDS: usize = 4_096;
/// Bound for one inbound foreign `tracestate`/`baggage` header.
pub const MAX_FOREIGN_CONTEXT_BYTES: usize = 4 * 1024;
/// Bound for the number of inert members retained from one inbound foreign header.
pub const MAX_FOREIGN_ANNOTATIONS: usize = 16;
/// Bound for one foreign member key before it is refused.
pub const MAX_FOREIGN_KEY_BYTES: usize = 64;
/// Bound for one foreign member value before it is hashed and dropped.
pub const MAX_FOREIGN_VALUE_BYTES: usize = 256;
/// Bound for the stored last-rejection code.
const MAX_LAST_REJECTION_BYTES: usize = 128;

/// Schema label of the local OTLP mapping. It is not an OTLP endpoint or a delivery guarantee.
pub const OTLP_MAPPING_SCHEMA: &str = "kiana.otlp-mapping.v1";
/// Static limitations that hold for every mapping produced by this module.
pub const OTLP_MAPPING_LIMITATIONS: &[&str] = &[
    "local_mapping_only",
    "no_transport",
    "no_backend_ack",
    "no_timestamps",
    "no_authority_fields",
];

/// Foreign keys that would read as identity, scope or authority if they were promoted. They are
/// refused at the boundary instead of being silently absorbed as telemetry.
const FOREIGN_AUTHORITY_KEYS: &[&str] = &[
    "actor",
    "approval",
    "authority",
    "capability",
    "cell",
    "command",
    "department",
    "grant",
    "identity",
    "org",
    "organization",
    "policy",
    "project",
    "role",
    "scope",
    "session",
    "tenant",
    "user",
    "workspace",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceExportConfig {
    pub enabled: bool,
    pub sampled: bool,
    pub capacity: usize,
}

impl Default for TraceExportConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sampled: false,
            capacity: 1_024,
        }
    }
}

/// Why a summary was not exported. These are local classifications, not foreign input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceSamplingReason {
    /// The local exporter is disabled by configuration.
    ExporterDisabled,
    /// The local exporter is enabled but the server-side sampling switch is off.
    LocalSamplingOff,
    /// The caller explicitly declined to export this summary.
    CallerDeclined,
}

impl TraceSamplingReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ExporterDisabled => "exporter_disabled",
            Self::LocalSamplingOff => "local_sampling_off",
            Self::CallerDeclined => "caller_declined",
        }
    }
}

/// Server-owned decision about whether one summary may be exported.
///
/// A foreign `traceparent` sampled flag is advisory metadata: it is exposed through
/// [`ForeignTraceContext::parent_sampled`] and never enters this decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceSamplingDecision {
    Sampled,
    NotSampled(TraceSamplingReason),
}

impl TraceSamplingDecision {
    pub const fn sampled(self) -> bool {
        matches!(self, Self::Sampled)
    }

    pub const fn reason(self) -> Option<TraceSamplingReason> {
        match self {
            Self::Sampled => None,
            Self::NotSampled(reason) => Some(reason),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TraceExportError {
    #[error("trace_export_capacity_invalid")]
    CapacityInvalid,
    #[error("trace_export_closed")]
    Closed,
    #[error("trace_export_capacity_exceeded")]
    CapacityExceeded,
    #[error("trace_export_parent_invalid:{0}")]
    ParentInvalid(String),
    #[error("trace_export_summary_invalid:{0}")]
    SummaryInvalid(String),
    #[error("trace_export_record_invalid:{0}")]
    RecordInvalid(String),
    #[error("trace_export_foreign_context_invalid:{0}")]
    ForeignContextInvalid(String),
    #[error("trace_export_foreign_authority_key:{0}")]
    ForeignAuthorityKey(String),
}

impl TraceExportError {
    /// Stable, bounded code for diagnostics and counters. Message payloads are not included, so a
    /// code never echoes refused content.
    pub fn code(&self) -> &'static str {
        match self {
            Self::CapacityInvalid => "trace_export_capacity_invalid",
            Self::Closed => "trace_export_closed",
            Self::CapacityExceeded => "trace_export_capacity_exceeded",
            Self::ParentInvalid(_) => "trace_export_parent_invalid",
            Self::SummaryInvalid(_) => "trace_export_summary_invalid",
            Self::RecordInvalid(_) => "trace_export_record_invalid",
            Self::ForeignContextInvalid(_) => "trace_export_foreign_context_invalid",
            Self::ForeignAuthorityKey(_) => "trace_export_foreign_authority_key",
        }
    }
}

/// One inert foreign annotation. The raw value is deliberately not retained: only a bounded digest
/// remains, so an operator can match an inbound header without copying foreign payloads into Kiana
/// telemetry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignAnnotation {
    pub key: String,
    pub value_digest: String,
}

/// Bounded, inert foreign W3C context parsed at an external boundary.
///
/// The type carries no actor, project, session, authority, approval, capability or policy field.
/// It can only contribute `ForeignParent` links and inert annotation digests, and it never exposes
/// a raw foreign value to a caller.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ForeignTraceContext {
    pub parent: Option<TraceParent>,
    pub trace_state: Vec<ForeignAnnotation>,
    pub baggage: Vec<ForeignAnnotation>,
}

impl ForeignTraceContext {
    /// Parse the bounded inbound W3C headers. An invalid `traceparent`, an oversized header, a
    /// malformed member, a secret sentinel or an authority-shaped key refuses the whole context;
    /// nothing is silently rewritten or promoted.
    pub fn from_ingress(
        traceparent: Option<&str>,
        tracestate: Option<&str>,
        baggage: Option<&str>,
    ) -> Result<Self, TraceExportError> {
        let parent = traceparent
            .map(TraceParent::parse)
            .transpose()
            .map_err(TraceExportError::ParentInvalid)?;
        Ok(Self {
            parent,
            trace_state: tracestate
                .map(parse_foreign_annotations)
                .transpose()?
                .unwrap_or_default(),
            baggage: baggage
                .map(parse_foreign_annotations)
                .transpose()?
                .unwrap_or_default(),
        })
    }

    /// Advisory parent sampling bit from the foreign `traceparent`. It never overrides the local
    /// [`LocalTraceExporter::sampling_decision`].
    pub fn parent_sampled(&self) -> Option<bool> {
        self.parent
            .as_ref()
            .map(|parent| parent.trace_flags & 0x01 != 0)
    }

    /// Link-only view of the foreign parent. No field of this context is authority.
    pub fn span_links(&self) -> Vec<SpanLink> {
        self.parent
            .iter()
            .map(|parent| SpanLink::new(parent.parent_ref(), SpanLinkKind::ForeignParent))
            .collect()
    }
}

fn foreign_context_invalid(code: &str) -> TraceExportError {
    TraceExportError::ForeignContextInvalid(code.to_owned())
}

fn foreign_key_byte_allowed(byte: u8) -> bool {
    byte.is_ascii_graphic() && byte != b',' && byte != b'='
}

fn foreign_key_allowed(key: &str) -> bool {
    if key.is_empty() || key.len() > MAX_FOREIGN_KEY_BYTES {
        return false;
    }
    if key.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return false;
    }
    key.bytes().all(foreign_key_byte_allowed)
}

fn foreign_authority_key(key: &str) -> Option<&'static str> {
    for segment in key.split(|character: char| !character.is_ascii_alphanumeric()) {
        for authority in FOREIGN_AUTHORITY_KEYS {
            if *authority == segment {
                return Some(*authority);
            }
        }
    }
    None
}

fn foreign_value_too_long(value: &str) -> bool {
    value.len() > MAX_FOREIGN_VALUE_BYTES || value.bytes().any(|byte| byte.is_ascii_control())
}

fn parse_foreign_annotations(value: &str) -> Result<Vec<ForeignAnnotation>, TraceExportError> {
    if value.len() > MAX_FOREIGN_CONTEXT_BYTES || value.trim() != value {
        return Err(foreign_context_invalid("trace_export_foreign_context_size"));
    }
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let members = value.split(',').collect::<Vec<_>>();
    if members.len() > MAX_FOREIGN_ANNOTATIONS {
        return Err(foreign_context_invalid(
            "trace_export_foreign_annotation_limit",
        ));
    }
    let profile = RedactionProfile::for_signal(RedactionSignal::Trace);
    let mut annotations = Vec::with_capacity(members.len());
    for member in members {
        let (key, raw_value) = match member.trim().split_once('=') {
            Some(parts) => parts,
            None => {
                return Err(foreign_context_invalid(
                    "trace_export_foreign_member_invalid",
                ));
            }
        };
        let key = key.trim();
        let raw_value = raw_value.trim();
        if !foreign_key_allowed(key) {
            return Err(foreign_context_invalid("trace_export_foreign_key_invalid"));
        }
        if let Some(authority) = foreign_authority_key(key) {
            return Err(TraceExportError::ForeignAuthorityKey(authority.to_owned()));
        }
        if foreign_value_too_long(raw_value) {
            return Err(foreign_context_invalid(
                "trace_export_foreign_value_invalid",
            ));
        }
        let raw = json!({"key": key, "value": raw_value});
        let encoded = encode_bounded_value(&profile, &raw)
            .map_err(|_| foreign_context_invalid("trace_export_foreign_secret_sentinel"))?;
        // The scanner refuses rewritten values instead of publishing a transformed annotation that
        // no longer matches the inbound header.
        if encoded.value != raw {
            return Err(foreign_context_invalid(
                "trace_export_foreign_annotation_changed",
            ));
        }
        annotations.push(ForeignAnnotation {
            key: key.to_owned(),
            value_digest: json_digest(&raw),
        });
    }
    Ok(annotations)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceExportReceipt {
    pub sequence: u64,
    pub export_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceExportDisposition {
    Exported(TraceExportReceipt),
    SampledOut,
}

/// Bounded local export statistics.
///
/// These counters describe this exporter's own buffer only. They are not EventLog facts, not
/// delivery evidence and not a claim that any external backend accepted a mapping.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TraceExportStats {
    pub exported_total: u64,
    pub sampled_out_total: u64,
    pub rejected_total: u64,
    pub flush_sequence: u64,
    pub depth: usize,
    pub capacity: usize,
    pub closed: bool,
    pub last_rejection_reason: Option<String>,
}

#[derive(Default)]
struct ExportState {
    records: Vec<TraceExportSpan>,
    sequence: u64,
    flush_sequence: u64,
    closed: bool,
    exported_total: u64,
    sampled_out_total: u64,
    rejected_total: u64,
    last_rejection_reason: Option<String>,
}

/// A local bounded exporter. `jsonl()` returns redacted, validated lines for an outer adapter to
/// persist; this type itself does not open files or contact an OTLP backend.
#[derive(Clone)]
pub struct LocalTraceExporter {
    config: TraceExportConfig,
    state: Arc<Mutex<ExportState>>,
}

impl std::fmt::Debug for LocalTraceExporter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalTraceExporter")
            .field("enabled", &self.config.enabled)
            .field("sampled", &self.config.sampled)
            .field("capacity", &self.config.capacity)
            .field("records", &self.records().len())
            .finish()
    }
}

impl LocalTraceExporter {
    pub fn new(config: TraceExportConfig) -> Result<Self, TraceExportError> {
        if config.capacity == 0 || config.capacity > MAX_EXPORT_RECORDS {
            return Err(TraceExportError::CapacityInvalid);
        }
        Ok(Self {
            config,
            state: Arc::new(Mutex::new(ExportState::default())),
        })
    }

    pub fn noop() -> Self {
        Self::new(TraceExportConfig::default()).expect("default trace export config is valid")
    }

    pub fn config(&self) -> TraceExportConfig {
        self.config
    }

    /// Server-owned sampling decision for one summary. A foreign parent's sampled bit is advisory
    /// and is deliberately not an input here.
    pub fn sampling_decision(&self, caller_sampled: bool) -> TraceSamplingDecision {
        if !self.config.enabled {
            return TraceSamplingDecision::NotSampled(TraceSamplingReason::ExporterDisabled);
        }
        if !self.config.sampled {
            return TraceSamplingDecision::NotSampled(TraceSamplingReason::LocalSamplingOff);
        }
        if !caller_sampled {
            return TraceSamplingDecision::NotSampled(TraceSamplingReason::CallerDeclined);
        }
        TraceSamplingDecision::Sampled
    }

    fn derive_span_id(summary: &TraceSummary) -> Result<SpanId, TraceExportError> {
        if let Some(root) = &summary.root_span_ref {
            return SpanId::parse(root).map_err(TraceExportError::SummaryInvalid);
        }
        let digest = json_digest(&serde_json::json!({
            "trace_id": summary.trace_id,
            "source_cursor": summary.source_cursor,
            "kind": "kiana.trace-export",
        }));
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or_else(|| TraceExportError::SummaryInvalid("span_id".to_owned()))?;
        SpanId::parse(&hex[..16]).map_err(TraceExportError::SummaryInvalid)
    }

    fn allowlisted_attributes(summary: &TraceSummary) -> BTreeMap<String, String> {
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
        summary
            .attributes
            .iter()
            .filter(|(key, _)| ALLOWED.iter().any(|allowed| allowed == key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    fn checked_attributes(
        summary: &TraceSummary,
    ) -> Result<BTreeMap<String, String>, TraceExportError> {
        let attributes = Self::allowlisted_attributes(summary);
        let raw = serde_json::to_value(&attributes).map_err(|_| {
            TraceExportError::RecordInvalid("trace_export_encode_failed".to_owned())
        })?;
        let profile =
            kiana_domain::RedactionProfile::for_signal(kiana_domain::RedactionSignal::Export);
        let encoded = kiana_domain::encode_bounded_value(&profile, &raw)
            .map_err(TraceExportError::RecordInvalid)?;
        // The source summary is sealed. Refuse unsafe metadata instead of publishing
        // transformed values that no longer match its source-bound meaning.
        if encoded.value != raw {
            return Err(TraceExportError::RecordInvalid(
                "trace_export_attributes_changed".to_owned(),
            ));
        }
        Ok(attributes)
    }

    fn record_failure(&self, error: &TraceExportError) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.rejected_total = state.rejected_total.saturating_add(1);
        let mut reason = error.code().to_owned();
        reason.truncate(MAX_LAST_REJECTION_BYTES);
        state.last_rejection_reason = Some(reason);
    }

    fn finish_export(
        &self,
        result: Result<TraceExportDisposition, TraceExportError>,
    ) -> Result<TraceExportDisposition, TraceExportError> {
        if let Err(error) = &result {
            self.record_failure(error);
        }
        result
    }

    fn export_inner(
        &self,
        summary: &TraceSummary,
        parent_span_id: Option<SpanId>,
        caller_sampled: bool,
    ) -> Result<TraceExportDisposition, TraceExportError> {
        summary
            .validate()
            .map_err(TraceExportError::SummaryInvalid)?;
        if !self.sampling_decision(caller_sampled).sampled() {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.sampled_out_total = state.sampled_out_total.saturating_add(1);
            return Ok(TraceExportDisposition::SampledOut);
        }
        let trace_id =
            TraceId::parse(&summary.trace_id).map_err(TraceExportError::SummaryInvalid)?;
        let span_id = Self::derive_span_id(summary)?;
        let record = TraceExportSpan::new(
            trace_id,
            span_id,
            parent_span_id,
            "kiana.trace",
            summary.status,
            true,
            summary.source_cursor,
            summary.source_event_ids.clone(),
            summary.duration_ms,
            Self::checked_attributes(summary)?,
        )
        .map_err(TraceExportError::RecordInvalid)?;
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return Err(TraceExportError::Closed);
        }
        if state.records.len() >= self.config.capacity {
            return Err(TraceExportError::CapacityExceeded);
        }
        state.sequence = state.sequence.saturating_add(1);
        let receipt = TraceExportReceipt {
            sequence: state.sequence,
            export_digest: record.export_digest.clone(),
        };
        state.records.push(record);
        state.exported_total = state.exported_total.saturating_add(1);
        Ok(TraceExportDisposition::Exported(receipt))
    }

    pub fn export_summary(
        &self,
        summary: &TraceSummary,
        traceparent: Option<&str>,
        sampled: bool,
    ) -> Result<TraceExportDisposition, TraceExportError> {
        let parent_span_id = match traceparent.map(TraceParent::parse).transpose() {
            Ok(parent) => parent.map(|parent| parent.parent_span_id),
            Err(message) => {
                let error = TraceExportError::ParentInvalid(message);
                self.record_failure(&error);
                return Err(error);
            }
        };
        self.finish_export(self.export_inner(summary, parent_span_id, sampled))
    }

    /// Export one summary together with an already parsed foreign context. Only the parent link is
    /// read from the context; foreign annotations never enter the exported record.
    pub fn export_summary_with_context(
        &self,
        summary: &TraceSummary,
        context: Option<&ForeignTraceContext>,
        sampled: bool,
    ) -> Result<TraceExportDisposition, TraceExportError> {
        let parent_span_id = context
            .and_then(|context| context.parent.as_ref())
            .map(|parent| parent.parent_span_id.clone());
        self.finish_export(self.export_inner(summary, parent_span_id, sampled))
    }

    pub fn records(&self) -> Vec<TraceExportSpan> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .records
            .clone()
    }

    pub fn stats(&self) -> TraceExportStats {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        TraceExportStats {
            exported_total: state.exported_total,
            sampled_out_total: state.sampled_out_total,
            rejected_total: state.rejected_total,
            flush_sequence: state.flush_sequence,
            depth: state.records.len(),
            capacity: self.config.capacity,
            closed: state.closed,
            last_rejection_reason: state.last_rejection_reason.clone(),
        }
    }

    fn render_jsonl(records: &[TraceExportSpan]) -> Result<String, TraceExportError> {
        let mut output = String::new();
        for record in records {
            record.validate().map_err(TraceExportError::RecordInvalid)?;
            let line = serde_json::to_string(record).map_err(|_| {
                TraceExportError::RecordInvalid("trace_export_encode_failed".to_owned())
            })?;
            output.push_str(&line);
            output.push('\n');
        }
        Ok(output)
    }

    pub fn jsonl(&self) -> Result<String, TraceExportError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Self::render_jsonl(&state.records)
    }

    /// Render the validated records and release their buffer slots. Rendering refuses an invalid
    /// record before anything is cleared, so a failed drain never loses evidence silently.
    pub fn drain_jsonl(&self) -> Result<String, TraceExportError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let output = Self::render_jsonl(&state.records)?;
        state.records.clear();
        Ok(output)
    }

    pub fn flush(&self) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.flush_sequence = state.flush_sequence.saturating_add(1);
        state.flush_sequence
    }

    /// Close the exporter. Closing keeps the validated records for a later drain; it never
    /// rewrites EventLog, Receipt or authorization state.
    pub fn shutdown(&self) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        state.flush_sequence
    }

    pub fn reopen(&self) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = false;
        state.flush_sequence = state.flush_sequence.saturating_add(1);
        state.flush_sequence
    }
}

pub type NoopTraceExporter = LocalTraceExporter;

/// Parse a foreign W3C parent for linking. The returned relationship is never used as an
/// authenticated parent or authority source.
pub fn foreign_parent_link(traceparent: &str) -> Result<SpanLink, TraceExportError> {
    let parent = TraceParent::parse(traceparent).map_err(TraceExportError::ParentInvalid)?;
    Ok(SpanLink::new(
        parent.parent_ref(),
        SpanLinkKind::ForeignParent,
    ))
}

/// Convert a projection status without changing its semantics; retained for adapter callers that
/// need an explicit mapping boundary.
pub fn exportable_status(status: TraceStatus) -> TraceStatus {
    status
}

fn otlp_span(record: &TraceExportSpan) -> Value {
    let status_code = match record.status {
        TraceStatus::Ok => 1,
        TraceStatus::Error => 2,
        TraceStatus::Unknown | TraceStatus::Degraded => 0,
    };
    let mut attributes = record
        .attributes
        .iter()
        .map(|(key, value)| json!({"key": key, "value": {"stringValue": value}}))
        .collect::<Vec<_>>();
    if let Some(duration_ms) = record.duration_ms {
        attributes.push(json!({"key": "kiana.duration.ms", "value": {"intValue": duration_ms}}));
    }
    let digest_value = json!({"stringValue": &record.export_digest});
    attributes.push(json!({"key": "kiana.export.digest", "value": digest_value}));
    let cursor_value = json!({"intValue": record.source_cursor});
    attributes.push(json!({"key": "kiana.source.cursor", "value": cursor_value}));
    let mut span = json!({
        "traceId": record.trace_id.as_str(),
        "spanId": record.span_id.as_str(),
        "name": record.name.as_str(),
        "kind": 1,
        "flags": if record.sampled { 1 } else { 0 },
        "status": {"code": status_code},
        "attributes": attributes,
    });
    if let Some(parent_span_id) = &record.parent_span_id {
        span["parentSpanId"] = json!(parent_span_id.as_str());
    }
    span
}

/// Map validated export spans onto a bounded OTLP/JSON-shaped value.
///
/// The mapping is local and lossy: it carries no timestamp (the projection has none), no actor,
/// scope, approval, receipt or authority field, and it performs no network call. Field names follow
/// OTLP/JSON so an operator can inspect the shape offline. Nothing here claims that a collector
/// accepted the result; see [`OTLP_MAPPING_LIMITATIONS`].
pub fn otlp_export_json(records: &[TraceExportSpan]) -> Result<Value, TraceExportError> {
    if records.len() > MAX_EXPORT_RECORDS {
        return Err(TraceExportError::CapacityExceeded);
    }
    let mut spans = Vec::with_capacity(records.len());
    for record in records {
        record.validate().map_err(TraceExportError::RecordInvalid)?;
        spans.push(otlp_span(record));
    }
    Ok(json!({
        "mappingVersion": OTLP_MAPPING_SCHEMA,
        "mappingLimitations": OTLP_MAPPING_LIMITATIONS,
        "resourceSpans": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "kiana"}},
                    {"key": "kiana.exporter", "value": {"stringValue": "local-trace-exporter"}},
                ],
            },
            "scopeSpans": [{
                "scope": {"name": "kiana.trace.export", "version": "v1"},
                "spans": spans,
            }],
        }],
    }))
}

/// Digest of one local OTLP mapping. It binds the mapped bytes so two mappings can be compared
/// without claiming that any backend received either of them.
pub fn otlp_export_digest(records: &[TraceExportSpan]) -> Result<String, TraceExportError> {
    Ok(json_digest(&otlp_export_json(records)?))
}
