//! Bounded trace export and W3C context adaptation.
//!
//! Export is intentionally optional and best-effort. A foreign `traceparent` is parsed into a
//! link only; server-owned trace/span IDs, actor, scope and authority remain unchanged. Closing
//! or sampling out this exporter never changes EventLog or Receipt facts.

use kiana_domain::{
    json_digest, SpanId, SpanLink, SpanLinkKind, TraceExportSpan, TraceId, TraceParent,
    TraceStatus, TraceSummary,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

const MAX_EXPORT_RECORDS: usize = 4_096;

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

#[derive(Default)]
struct ExportState {
    records: Vec<TraceExportSpan>,
    sequence: u64,
    flush_sequence: u64,
    closed: bool,
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

    pub fn export_summary(
        &self,
        summary: &TraceSummary,
        traceparent: Option<&str>,
        sampled: bool,
    ) -> Result<TraceExportDisposition, TraceExportError> {
        summary
            .validate()
            .map_err(TraceExportError::SummaryInvalid)?;
        let parent_span_id = traceparent
            .map(TraceParent::parse)
            .transpose()
            .map_err(TraceExportError::ParentInvalid)?
            .map(|parent| parent.parent_span_id);
        if !self.config.enabled || !self.config.sampled || !sampled {
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
            Self::allowlisted_attributes(summary),
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
        Ok(TraceExportDisposition::Exported(receipt))
    }

    pub fn records(&self) -> Vec<TraceExportSpan> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .records
            .clone()
    }

    pub fn jsonl(&self) -> Result<String, TraceExportError> {
        let records = self.records();
        let mut output = String::new();
        for record in records {
            record.validate().map_err(TraceExportError::RecordInvalid)?;
            let line = serde_json::to_string(&record)
                .map_err(|_| TraceExportError::RecordInvalid("encode".to_owned()))?;
            output.push_str(&line);
            output.push('\n');
        }
        Ok(output)
    }

    pub fn flush(&self) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.flush_sequence = state.flush_sequence.saturating_add(1);
        state.flush_sequence
    }

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
