//! Versioned model/provider facts and the bounded redaction boundary.
//!
//! A [`ModelEvent`] is an audit payload, not a model request and not an execution permit.  It
//! contains server-owned identities and hashes of request material; the request, headers, query
//! values and provider bodies never belong in this type.  The event can therefore be persisted
//! by the EventLog without making the transcript or a provider credential a second source of
//! truth.

use crate::{
    canonical_journal_bytes, encode_bounded_text, json_digest, CorrelationContext, EventCursor,
    EventId, ExecutionId, InvocationId, ModelAttemptId, ModelFinish, ModelRetryClass, ModelUsage,
    RedactionProfile, RequestId, RunId, RuntimeEvent, SchemaVersion, SessionId, SpanId, StepId,
    StreamingRedactor, TurnId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MODEL_EVENT_SCHEMA: &str = "kiana.model-event.v1";
pub const MODEL_EVENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MODEL_TRACE_SCHEMA: &str = "kiana.provider-trace.v1";
pub const MODEL_TRACE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MODEL_FACT_COMMITMENT_SCHEMA: &str = "kiana.model-fact-commitment.v1";
pub const MODEL_FACT_COMMITMENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MODEL_INVOCATION_LINK_SCHEMA: &str = "kiana.model-invocation-link.v1";
pub const MODEL_INVOCATION_LINK_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_EFFECT: &str =
    "model_fact_persistence_required_before_tool_dispatch";
pub const MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_COMPLETION: &str =
    "model_fact_persistence_required_before_mark_completed";
pub const MAX_MODEL_PROVIDER_ID_BYTES: usize = 128;
pub const MAX_MODEL_ID_BYTES: usize = 256;
pub const MAX_MODEL_CONFIGURATION_BYTES: usize = 256;
pub const MAX_MODEL_TOOL_NAME_BYTES: usize = 256;
pub const MAX_MODEL_CALL_ID_BYTES: usize = 128;
pub const MAX_PROVIDER_REFERENCE_BYTES: usize = 512;
pub const MAX_PROVIDER_TRACE_FIELDS: usize = 6;
pub const MAX_PROVIDER_TRACE_BYTES: usize = 64 * 1024;
pub const MAX_PROVIDER_DELTA_SEGMENTS: usize = 64;
pub const MAX_MODEL_RETRY_REASON_BYTES: usize = 512;

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn required_text(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn optional_text(value: Option<&str>, field: &str, max: usize) -> Result<(), String> {
    if let Some(value) = value {
        required_text(value, field, max)?;
    }
    Ok(())
}

/// The six model fact kinds.  The older `run.model_turn` envelope remains readable through the
/// event registry; new producers may use these narrower kinds without changing the execution
/// spine.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelEventKind {
    Prepared,
    Denied,
    AttemptStarted,
    RetryScheduled,
    Finished,
    UsageCorrection,
}

impl ModelEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "model.prepared",
            Self::Denied => "model.denied",
            Self::AttemptStarted => "model.attempt_started",
            Self::RetryScheduled => "model.retry_scheduled",
            Self::Finished => "model.finished",
            Self::UsageCorrection => "model.usage_correction",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Denied | Self::Finished)
    }
}

/// Provider IDs are opaque observations.  Missing wire IDs stay explicitly unknown instead of
/// being replaced with a model call ID or another locally generated value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderReferenceStatus {
    Known,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub status: ProviderReferenceStatus,
}

pub type ProviderRequestReference = ProviderReference;
pub type ProviderResponseReference = ProviderReference;
pub type ProviderRequestRef = ProviderReference;
pub type ProviderResponseRef = ProviderReference;

impl ProviderReference {
    pub fn known(id: impl Into<String>) -> Result<Self, String> {
        let id = id.into();
        required_text(&id, "provider_reference_id", MAX_PROVIDER_REFERENCE_BYTES)?;
        Ok(Self {
            id: Some(id),
            status: ProviderReferenceStatus::Known,
        })
    }

    pub const fn unknown() -> Self {
        Self {
            id: None,
            status: ProviderReferenceStatus::Unknown,
        }
    }

    pub fn validate(&self, field: &str) -> Result<(), String> {
        match (&self.id, self.status) {
            (Some(id), ProviderReferenceStatus::Known) => {
                required_text(id, field, MAX_PROVIDER_REFERENCE_BYTES)
            }
            (None, ProviderReferenceStatus::Unknown) => Ok(()),
            (Some(_), ProviderReferenceStatus::Unknown)
            | (None, ProviderReferenceStatus::Known) => Err(format!("{field}_status_mismatch")),
        }
    }

    pub fn is_unknown(&self) -> bool {
        self.status == ProviderReferenceStatus::Unknown
    }
}

/// A durable reference to a redacted provider value.  It carries no source bytes; `digest` is
/// computed over the redacted value, so callers can compare observations without recovering the
/// original header, query string, body or delta.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactedTraceReference {
    pub digest: String,
    pub bytes: usize,
    pub profile_digest: String,
    pub redacted: bool,
}

impl RedactedTraceReference {
    pub fn from_text(profile: &RedactionProfile, value: &str) -> Result<Self, String> {
        let encoded = encode_bounded_text(profile, value)?;
        Self::from_redacted_text(&profile.profile_digest, &encoded.text)
    }

    pub fn from_redacted_text(profile_digest: &str, value: &str) -> Result<Self, String> {
        valid_digest(profile_digest, "trace_profile_digest")?;
        if value.as_bytes().contains(&0) || value.len() > MAX_PROVIDER_TRACE_BYTES {
            return Err("provider_trace_value_invalid".to_owned());
        }
        Ok(Self {
            digest: json_digest(&json!(value)),
            bytes: value.len(),
            profile_digest: profile_digest.to_owned(),
            redacted: true,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        valid_digest(&self.digest, "trace_digest")?;
        valid_digest(&self.profile_digest, "trace_profile_digest")?;
        if self.bytes > MAX_PROVIDER_TRACE_BYTES || !self.redacted {
            return Err("provider_trace_reference_unredacted_or_too_large".to_owned());
        }
        Ok(())
    }
}

/// Header/query/error/body/delta/replay are intentionally references only.  There is no field
/// for an authorization header, raw request, prompt, response body or replay bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTraceMetadata {
    pub schema: String,
    pub version: SchemaVersion,
    pub profile_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<RedactedTraceReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<RedactedTraceReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RedactedTraceReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<RedactedTraceReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<RedactedTraceReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<RedactedTraceReference>,
    pub trace_digest: String,
}

impl ProviderTraceMetadata {
    pub fn empty(profile: &RedactionProfile) -> Result<Self, String> {
        profile.validate()?;
        let mut metadata = Self {
            schema: MODEL_TRACE_SCHEMA.to_owned(),
            version: MODEL_TRACE_SCHEMA_VERSION,
            profile_digest: profile.profile_digest.clone(),
            header: None,
            query: None,
            error: None,
            body: None,
            delta: None,
            replay: None,
            trace_digest: String::new(),
        };
        metadata.trace_digest = metadata.digest();
        metadata.validate()?;
        Ok(metadata)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_texts(
        profile: &RedactionProfile,
        header: Option<&str>,
        query: Option<&str>,
        error: Option<&str>,
        body: Option<&str>,
        delta: Option<&str>,
        replay: Option<&str>,
    ) -> Result<Self, String> {
        profile.validate()?;
        let mut metadata = Self {
            schema: MODEL_TRACE_SCHEMA.to_owned(),
            version: MODEL_TRACE_SCHEMA_VERSION,
            profile_digest: profile.profile_digest.clone(),
            header: header
                .map(|value| RedactedTraceReference::from_text(profile, value))
                .transpose()?,
            query: query
                .map(|value| RedactedTraceReference::from_text(profile, value))
                .transpose()?,
            error: error
                .map(|value| RedactedTraceReference::from_text(profile, value))
                .transpose()?,
            body: body
                .map(|value| RedactedTraceReference::from_text(profile, value))
                .transpose()?,
            delta: delta
                .map(|value| RedactedTraceReference::from_text(profile, value))
                .transpose()?,
            replay: replay
                .map(|value| RedactedTraceReference::from_text(profile, value))
                .transpose()?,
            trace_digest: String::new(),
        };
        metadata.trace_digest = metadata.digest();
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_TRACE_SCHEMA
            || !self.version.is_compatible_with(&MODEL_TRACE_SCHEMA_VERSION)
        {
            return Err("provider_trace_schema_invalid".to_owned());
        }
        valid_digest(&self.profile_digest, "trace_profile_digest")?;
        for value in [
            self.header.as_ref(),
            self.query.as_ref(),
            self.error.as_ref(),
            self.body.as_ref(),
            self.delta.as_ref(),
            self.replay.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            value.validate()?;
            if value.profile_digest != self.profile_digest {
                return Err("provider_trace_profile_mismatch".to_owned());
            }
        }
        valid_digest(&self.trace_digest, "provider_trace_digest")?;
        if self.trace_digest != self.digest() {
            return Err("provider_trace_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "profile_digest": self.profile_digest,
            "header": self.header,
            "query": self.query,
            "error": self.error,
            "body": self.body,
            "delta": self.delta,
            "replay": self.replay,
        }))
    }
}

/// A bounded delta ledger.  It counts every observed delta but retains at most
/// `max_segments` redacted segment references.  The running aggregate digest still changes for
/// dropped segments, allowing a receipt to prove that aggregation occurred without writing one
/// durable row per token.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDeltaLedger {
    pub schema: String,
    pub version: SchemaVersion,
    pub profile: RedactionProfile,
    pub max_segments: usize,
    pub observed_delta_count: u64,
    pub retained_segments: Vec<RedactedTraceReference>,
    pub dropped_segment_count: u64,
    pub aggregate_digest: String,
    #[serde(skip)]
    redactor: StreamingRedactor,
}

impl ProviderDeltaLedger {
    pub fn new(profile: RedactionProfile, max_segments: usize) -> Result<Self, String> {
        profile.validate()?;
        if max_segments == 0 || max_segments > MAX_PROVIDER_DELTA_SEGMENTS {
            return Err("provider_delta_segment_limit_invalid".to_owned());
        }
        let mut ledger = Self {
            schema: MODEL_TRACE_SCHEMA.to_owned(),
            version: MODEL_TRACE_SCHEMA_VERSION,
            profile,
            max_segments,
            observed_delta_count: 0,
            retained_segments: Vec::new(),
            dropped_segment_count: 0,
            aggregate_digest: String::new(),
            redactor: StreamingRedactor::new(),
        };
        ledger.aggregate_digest = ledger.digest();
        ledger.validate()?;
        Ok(ledger)
    }

    /// Record one provider delta.  Only the redacted output digest is retained.
    pub fn append_text(&mut self, value: &str) -> Result<(), String> {
        self.observed_delta_count = self
            .observed_delta_count
            .checked_add(1)
            .ok_or_else(|| "provider_delta_count_overflow".to_owned())?;
        let safe = self.redactor.push(value);
        self.record_safe_segment(&safe)
    }

    /// Flush a marker or secret that was split across the final provider chunks.
    pub fn finish(&mut self) -> Result<(), String> {
        let safe = self.redactor.finish();
        self.record_safe_segment(&safe)
    }

    fn record_safe_segment(&mut self, value: &str) -> Result<(), String> {
        if value.is_empty() {
            self.aggregate_digest = self.digest();
            return Ok(());
        }
        let reference = RedactedTraceReference::from_text(&self.profile, value)?;
        if self.retained_segments.len() < self.max_segments {
            self.retained_segments.push(reference.clone());
        } else {
            self.dropped_segment_count = self
                .dropped_segment_count
                .checked_add(1)
                .ok_or_else(|| "provider_delta_dropped_count_overflow".to_owned())?;
        }
        self.aggregate_digest = self.digest_with_segment(&reference);
        Ok(())
    }

    fn digest_with_segment(&self, segment: &RedactedTraceReference) -> String {
        json_digest(&json!({
            "prior": self.aggregate_digest,
            "observed_delta_count": self.observed_delta_count,
            "dropped_segment_count": self.dropped_segment_count,
            "segment": segment.digest,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_TRACE_SCHEMA
            || !self.version.is_compatible_with(&MODEL_TRACE_SCHEMA_VERSION)
            || self.max_segments == 0
            || self.max_segments > MAX_PROVIDER_DELTA_SEGMENTS
            || self.retained_segments.len() > self.max_segments
            || self.dropped_segment_count > self.observed_delta_count
        {
            return Err("provider_delta_ledger_invalid".to_owned());
        }
        self.profile.validate()?;
        for segment in &self.retained_segments {
            segment.validate()?;
            if segment.profile_digest != self.profile.profile_digest {
                return Err("provider_delta_profile_mismatch".to_owned());
            }
        }
        valid_digest(&self.aggregate_digest, "provider_delta_aggregate_digest")?;
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "profile_digest": self.profile.profile_digest,
            "max_segments": self.max_segments,
            "observed_delta_count": self.observed_delta_count,
            "retained_segments": self.retained_segments,
            "dropped_segment_count": self.dropped_segment_count,
        }))
    }

    pub fn retained_count(&self) -> usize {
        self.retained_segments.len()
    }
}

impl Default for ProviderDeltaLedger {
    fn default() -> Self {
        Self::new(RedactionProfile::default(), 16).expect("default provider delta ledger")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEventSequences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_sequence: Option<EventCursor>,
}

impl ModelEventSequences {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .ui_sequence
            .into_iter()
            .chain(self.upstream_sequence)
            .chain(self.durable_sequence)
            .any(|value| value == 0)
        {
            return Err("model_event_sequence_must_start_at_one".to_owned());
        }
        Ok(())
    }
}

impl Default for ModelEventSequences {
    fn default() -> Self {
        Self {
            ui_sequence: None,
            upstream_sequence: None,
            durable_sequence: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRetryMetadata {
    pub retry_class: ModelRetryClass,
    pub retry_ordinal: u32,
    pub next_attempt: u32,
    pub delay_ms: u64,
    pub reason_digest: String,
}

impl ModelRetryMetadata {
    pub fn validate(&self) -> Result<(), String> {
        if self.retry_ordinal == 0 || self.next_attempt == 0 || self.delay_ms > 86_400_000 {
            return Err("model_retry_metadata_invalid".to_owned());
        }
        valid_digest(&self.reason_digest, "model_retry_reason_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelUsageCorrection {
    pub correction_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces_digest: Option<String>,
    pub usage: Option<ModelUsage>,
    pub reason_digest: String,
    pub observed_at_unix_ms: u64,
}

impl ModelUsageCorrection {
    pub fn new(
        replaces_digest: Option<String>,
        usage: Option<ModelUsage>,
        reason_digest: impl Into<String>,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut correction = Self {
            correction_digest: String::new(),
            replaces_digest,
            usage,
            reason_digest: reason_digest.into(),
            observed_at_unix_ms,
        };
        correction.correction_digest = correction.digest();
        correction.validate()?;
        Ok(correction)
    }

    pub fn validate(&self) -> Result<(), String> {
        valid_digest(&self.correction_digest, "model_usage_correction_digest")?;
        if let Some(digest) = &self.replaces_digest {
            valid_digest(digest, "model_usage_replaces_digest")?;
        }
        valid_digest(&self.reason_digest, "model_usage_correction_reason_digest")?;
        if self.observed_at_unix_ms == 0 {
            return Err("model_usage_correction_timestamp_required".to_owned());
        }
        if let Some(usage) = &self.usage {
            if usage.input_tokens > 1_000_000_000 || usage.output_tokens > 1_000_000_000 {
                return Err("model_usage_correction_tokens_invalid".to_owned());
            }
        }
        if self.correction_digest != self.digest() {
            return Err("model_usage_correction_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "replaces_digest": self.replaces_digest,
            "usage": self.usage,
            "reason_digest": self.reason_digest,
            "observed_at_unix_ms": self.observed_at_unix_ms,
        }))
    }
}

/// One versioned model fact.  The hashes bind the compiled request dimensions without persisting
/// prompt/schema/configuration bytes.  `correlation` carries server-derived session/run/turn and
/// optional invocation/execution scope; copied ID fields make the payload queryable and are
/// checked against it during validation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEvent {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: ModelEventKind,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub model_call_id: RequestId,
    pub model_attempt_id: ModelAttemptId,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<InvocationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<ExecutionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    pub provider_id: String,
    pub actual_model_id: String,
    pub configuration_revision: String,
    pub prompt_digest: String,
    pub schema_digest: String,
    pub route_digest: String,
    pub config_digest: String,
    pub tool_catalog_digest: String,
    pub provider_request: ProviderRequestReference,
    pub provider_response: ProviderResponseReference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<ModelRetryMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<ModelFinish>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ModelUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_correction: Option<ModelUsageCorrection>,
    pub trace: ProviderTraceMetadata,
    pub sequences: ModelEventSequences,
    pub correlation: CorrelationContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<EventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_cursor: Option<EventCursor>,
    pub event_digest: String,
}

impl ModelEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: ModelEventKind,
        correlation: CorrelationContext,
        step_id: StepId,
        model_call_id: RequestId,
        model_attempt_id: ModelAttemptId,
        attempt: u32,
        provider_id: impl Into<String>,
        actual_model_id: impl Into<String>,
        configuration_revision: impl Into<String>,
        prompt_digest: impl Into<String>,
        schema_digest: impl Into<String>,
        route_digest: impl Into<String>,
        config_digest: impl Into<String>,
        tool_catalog_digest: impl Into<String>,
        trace: ProviderTraceMetadata,
    ) -> Result<Self, String> {
        let run_id = correlation
            .run_id
            .ok_or_else(|| "model_event_run_required".to_owned())?;
        let turn_id = correlation
            .turn_id
            .ok_or_else(|| "model_event_turn_required".to_owned())?;
        let mut event = Self {
            schema: MODEL_EVENT_SCHEMA.to_owned(),
            version: MODEL_EVENT_SCHEMA_VERSION,
            kind,
            session_id: correlation.session_id.clone(),
            run_id,
            turn_id,
            step_id,
            model_call_id,
            model_attempt_id,
            attempt,
            invocation_id: correlation.invocation_id,
            execution_id: correlation.execution_id,
            tool_call_id: None,
            tool_name: None,
            provider_id: provider_id.into(),
            actual_model_id: actual_model_id.into(),
            configuration_revision: configuration_revision.into(),
            prompt_digest: prompt_digest.into(),
            schema_digest: schema_digest.into(),
            route_digest: route_digest.into(),
            config_digest: config_digest.into(),
            tool_catalog_digest: tool_catalog_digest.into(),
            provider_request: ProviderReference::unknown(),
            provider_response: ProviderReference::unknown(),
            retry: None,
            finish: None,
            usage: None,
            usage_correction: None,
            trace,
            sequences: ModelEventSequences::default(),
            correlation,
            source_event_id: None,
            source_cursor: None,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        event.validate()?;
        Ok(event)
    }

    pub fn with_provider_references(
        mut self,
        request: ProviderRequestReference,
        response: ProviderResponseReference,
    ) -> Result<Self, String> {
        self.provider_request = request;
        self.provider_response = response;
        self.reseal()
    }

    pub fn with_tool(
        mut self,
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
    ) -> Result<Self, String> {
        self.tool_call_id = Some(tool_call_id.into());
        self.tool_name = Some(tool_name.into());
        self.reseal()
    }

    pub fn with_retry(mut self, retry: ModelRetryMetadata) -> Result<Self, String> {
        self.retry = Some(retry);
        self.reseal()
    }

    pub fn with_finish(
        mut self,
        finish: ModelFinish,
        usage: Option<ModelUsage>,
    ) -> Result<Self, String> {
        self.finish = Some(finish);
        self.usage = usage;
        self.reseal()
    }

    pub fn with_usage_correction(
        mut self,
        correction: ModelUsageCorrection,
    ) -> Result<Self, String> {
        self.usage_correction = Some(correction);
        self.reseal()
    }

    pub fn with_sequences(mut self, sequences: ModelEventSequences) -> Result<Self, String> {
        self.sequences = sequences;
        self.reseal()
    }

    pub fn with_source(mut self, event_id: EventId, cursor: EventCursor) -> Result<Self, String> {
        if cursor == 0 {
            return Err("model_event_source_cursor_required".to_owned());
        }
        self.source_event_id = Some(event_id);
        self.source_cursor = Some(cursor);
        self.reseal()
    }

    fn reseal(mut self) -> Result<Self, String> {
        self.event_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn runtime_event_kind(&self) -> &'static str {
        self.kind.as_str()
    }

    pub fn payload(&self) -> Result<Value, String> {
        self.validate()?;
        serde_json::to_value(self).map_err(|_| "model_event_encode_failed".to_owned())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// Encode this already validated fact at the RuntimeEvent boundary.  The resulting event
    /// contains only the bounded payload and redaction metadata; callers still have to append it
    /// through EventStore before dispatching a tool or marking a run complete.
    pub fn into_runtime_event(&self, sequence: u64) -> Result<RuntimeEvent, String> {
        self.validate()?;
        if sequence == 0 {
            return Err("model_event_runtime_sequence_required".to_owned());
        }
        let mut event = RuntimeEvent::new(
            self.model_call_id,
            sequence,
            self.runtime_event_kind(),
            self.payload()?,
        )
        .map_err(|_| "model_event_runtime_encode_failed".to_owned())?
        .with_stream_metadata("model_call", self.model_call_id.to_string(), sequence)
        .with_redaction_metadata(
            self.trace.profile_digest.clone(),
            false,
            Some(self.correlation.data_epoch),
            Vec::new(),
        );
        event = event.with_identity_links(
            self.correlation.command_id,
            Some(self.correlation.correlation_id),
            self.source_event_id,
            None,
        );
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_EVENT_SCHEMA
            || !self.version.is_compatible_with(&MODEL_EVENT_SCHEMA_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.step_id.as_uuid().is_nil()
            || self.model_call_id.as_uuid().is_nil()
            || self.model_attempt_id.as_uuid().is_nil()
            || self.attempt == 0
        {
            return Err("model_event_identity_invalid".to_owned());
        }
        self.correlation.validate()?;
        if self.correlation.session_id != self.session_id
            || self.correlation.run_id != Some(self.run_id)
            || self.correlation.turn_id != Some(self.turn_id)
            || self.correlation.invocation_id != self.invocation_id
            || self.correlation.execution_id != self.execution_id
        {
            return Err("model_event_correlation_mismatch".to_owned());
        }
        required_text(
            &self.provider_id,
            "model_provider_id",
            MAX_MODEL_PROVIDER_ID_BYTES,
        )?;
        required_text(
            &self.actual_model_id,
            "model_actual_model_id",
            MAX_MODEL_ID_BYTES,
        )?;
        required_text(
            &self.configuration_revision,
            "model_configuration_revision",
            MAX_MODEL_CONFIGURATION_BYTES,
        )?;
        for (field, value) in [
            ("model_prompt_digest", &self.prompt_digest),
            ("model_schema_digest", &self.schema_digest),
            ("model_route_digest", &self.route_digest),
            ("model_config_digest", &self.config_digest),
            ("model_tool_catalog_digest", &self.tool_catalog_digest),
        ] {
            valid_digest(value, field)?;
        }
        self.provider_request.validate("provider_request_id")?;
        self.provider_response.validate("provider_response_id")?;
        self.trace.validate()?;
        self.sequences.validate()?;
        if self.execution_id.is_some() != self.invocation_id.is_some() {
            return Err("model_event_invocation_execution_pair_required".to_owned());
        }
        optional_text(
            self.tool_call_id.as_deref(),
            "model_tool_call_id",
            MAX_MODEL_CALL_ID_BYTES,
        )?;
        optional_text(
            self.tool_name.as_deref(),
            "model_tool_name",
            MAX_MODEL_TOOL_NAME_BYTES,
        )?;
        if self.tool_call_id.is_some() != self.tool_name.is_some() {
            return Err("model_event_tool_identity_pair_required".to_owned());
        }
        if self.tool_name.is_some() && self.invocation_id.is_none() {
            return Err("model_event_tool_requires_invocation".to_owned());
        }
        if let Some(retry) = &self.retry {
            retry.validate()?;
        }
        if let Some(usage) = &self.usage {
            if usage.input_tokens > 1_000_000_000 || usage.output_tokens > 1_000_000_000 {
                return Err("model_event_usage_invalid".to_owned());
            }
        }
        if let Some(correction) = &self.usage_correction {
            correction.validate()?;
        }
        match self.kind {
            ModelEventKind::Prepared | ModelEventKind::Denied | ModelEventKind::AttemptStarted => {
                if self.finish.is_some() || self.usage_correction.is_some() {
                    return Err("model_event_terminal_fields_unexpected".to_owned());
                }
            }
            ModelEventKind::RetryScheduled => {
                if self.retry.is_none() || self.finish.is_some() || self.usage_correction.is_some()
                {
                    return Err("model_retry_event_fields_invalid".to_owned());
                }
            }
            ModelEventKind::Finished => {
                if self.finish.is_none() || self.usage_correction.is_some() {
                    return Err("model_finished_event_fields_invalid".to_owned());
                }
            }
            ModelEventKind::UsageCorrection => {
                if self.usage_correction.is_none() {
                    return Err("model_usage_correction_required".to_owned());
                }
            }
        }
        if let Some(cursor) = self.source_cursor {
            if cursor == 0 || self.source_event_id.is_none() {
                return Err("model_event_source_binding_invalid".to_owned());
            }
        } else if self.source_event_id.is_some() {
            return Err("model_event_source_binding_invalid".to_owned());
        }
        valid_digest(&self.event_digest, "model_event_digest")?;
        if self.event_digest != self.digest() {
            return Err("model_event_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("event_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

/// Append status used by the execution boundary.  A pending or failed fact can be inspected but
/// cannot grant a tool dispatch or terminal completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFactPersistenceStatus {
    Pending,
    Committed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFactCommitment {
    pub schema: String,
    pub version: SchemaVersion,
    pub fact_digest: String,
    pub status: ModelFactPersistenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_sequence: Option<EventCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_digest: Option<String>,
    pub commitment_digest: String,
}

pub type ModelFactPersistenceGuard = ModelFactCommitment;

impl ModelFactCommitment {
    pub fn pending(event: &ModelEvent) -> Result<Self, String> {
        event.validate()?;
        Self::from_digest(event.event_digest.clone())
    }

    pub fn from_digest(fact_digest: impl Into<String>) -> Result<Self, String> {
        let mut commitment = Self {
            schema: MODEL_FACT_COMMITMENT_SCHEMA.to_owned(),
            version: MODEL_FACT_COMMITMENT_SCHEMA_VERSION,
            fact_digest: fact_digest.into(),
            status: ModelFactPersistenceStatus::Pending,
            durable_sequence: None,
            failure_digest: None,
            commitment_digest: String::new(),
        };
        commitment.commitment_digest = commitment.digest();
        commitment.validate()?;
        Ok(commitment)
    }

    pub fn mark_appended(&mut self, durable_sequence: EventCursor) -> Result<(), String> {
        self.validate()?;
        if self.status != ModelFactPersistenceStatus::Pending || durable_sequence == 0 {
            return Err("model_fact_append_transition_invalid".to_owned());
        }
        self.status = ModelFactPersistenceStatus::Committed;
        self.durable_sequence = Some(durable_sequence);
        self.commitment_digest = self.digest();
        self.validate()
    }

    pub fn mark_append_failed(&mut self, reason: impl Into<String>) -> Result<(), String> {
        self.validate()?;
        if self.status != ModelFactPersistenceStatus::Pending {
            return Err("model_fact_append_transition_invalid".to_owned());
        }
        let reason = reason.into();
        required_text(&reason, "model_fact_failure", 512)?;
        self.status = ModelFactPersistenceStatus::Failed;
        self.failure_digest = Some(json_digest(&json!(reason)));
        self.commitment_digest = self.digest();
        self.validate()
    }

    pub fn can_dispatch_tool(&self) -> bool {
        self.status == ModelFactPersistenceStatus::Committed
    }

    pub fn can_mark_completed(&self) -> bool {
        self.status == ModelFactPersistenceStatus::Committed
    }

    pub fn require_tool_dispatch(&self) -> Result<(), String> {
        if self.can_dispatch_tool() {
            Ok(())
        } else {
            Err(MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_EFFECT.to_owned())
        }
    }

    pub fn require_mark_completed(&self) -> Result<(), String> {
        if self.can_mark_completed() {
            Ok(())
        } else {
            Err(MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_COMPLETION.to_owned())
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_FACT_COMMITMENT_SCHEMA
            || !self
                .version
                .is_compatible_with(&MODEL_FACT_COMMITMENT_SCHEMA_VERSION)
        {
            return Err("model_fact_commitment_schema_invalid".to_owned());
        }
        valid_digest(&self.fact_digest, "model_fact_digest")?;
        match self.status {
            ModelFactPersistenceStatus::Pending => {
                if self.durable_sequence.is_some() || self.failure_digest.is_some() {
                    return Err("model_fact_pending_metadata_invalid".to_owned());
                }
            }
            ModelFactPersistenceStatus::Committed => {
                if self.durable_sequence.is_none() || self.failure_digest.is_some() {
                    return Err("model_fact_committed_metadata_invalid".to_owned());
                }
            }
            ModelFactPersistenceStatus::Failed => {
                if self.durable_sequence.is_some() || self.failure_digest.is_none() {
                    return Err("model_fact_failed_metadata_invalid".to_owned());
                }
            }
        }
        if self.durable_sequence == Some(0) {
            return Err("model_fact_durable_sequence_invalid".to_owned());
        }
        if let Some(digest) = &self.failure_digest {
            valid_digest(digest, "model_fact_failure_digest")?;
        }
        valid_digest(&self.commitment_digest, "model_fact_commitment_digest")?;
        if self.commitment_digest != self.digest() {
            return Err("model_fact_commitment_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "fact_digest": self.fact_digest,
            "status": self.status,
            "durable_sequence": self.durable_sequence,
            "failure_digest": self.failure_digest,
        }))
    }
}

/// Server-owned linkage from an admitted model attempt to the tool invocation and its execution
/// receipt.  Arguments and handler output are represented only by the existing receipt digest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttemptInvocationReceiptLink {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub model_call_id: RequestId,
    pub model_attempt_id: ModelAttemptId,
    pub invocation_id: InvocationId,
    pub execution_id: ExecutionId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub execution_receipt_digest: String,
    pub source_model_event_digest: String,
    pub link_digest: String,
}

pub type ModelAttemptToInvocationReceipt = ModelAttemptInvocationReceiptLink;

impl ModelAttemptInvocationReceiptLink {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        step_id: StepId,
        model_call_id: RequestId,
        model_attempt_id: ModelAttemptId,
        invocation_id: InvocationId,
        execution_id: ExecutionId,
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        execution_receipt_digest: impl Into<String>,
        source_model_event_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut link = Self {
            schema: MODEL_INVOCATION_LINK_SCHEMA.to_owned(),
            version: MODEL_INVOCATION_LINK_VERSION,
            run_id,
            turn_id,
            step_id,
            model_call_id,
            model_attempt_id,
            invocation_id,
            execution_id,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            execution_receipt_digest: execution_receipt_digest.into(),
            source_model_event_digest: source_model_event_digest.into(),
            link_digest: String::new(),
        };
        link.link_digest = link.digest();
        link.validate()?;
        Ok(link)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_INVOCATION_LINK_SCHEMA
            || !self
                .version
                .is_compatible_with(&MODEL_INVOCATION_LINK_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.step_id.as_uuid().is_nil()
            || self.model_call_id.as_uuid().is_nil()
            || self.model_attempt_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
        {
            return Err("model_invocation_link_identity_invalid".to_owned());
        }
        required_text(
            &self.tool_call_id,
            "model_tool_call_id",
            MAX_MODEL_CALL_ID_BYTES,
        )?;
        required_text(
            &self.tool_name,
            "model_tool_name",
            MAX_MODEL_TOOL_NAME_BYTES,
        )?;
        valid_digest(
            &self.execution_receipt_digest,
            "model_execution_receipt_digest",
        )?;
        valid_digest(&self.source_model_event_digest, "model_source_event_digest")?;
        valid_digest(&self.link_digest, "model_invocation_link_digest")?;
        if self.link_digest != self.digest() {
            return Err("model_invocation_link_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "model_call_id": self.model_call_id,
            "model_attempt_id": self.model_attempt_id,
            "invocation_id": self.invocation_id,
            "execution_id": self.execution_id,
            "tool_call_id": self.tool_call_id,
            "tool_name": self.tool_name,
            "execution_receipt_digest": self.execution_receipt_digest,
            "source_model_event_digest": self.source_model_event_digest,
        }))
    }
}

/// A stable helper for tests and adapters that need to bind a model attempt to a trace span.
pub fn model_attempt_span_id(
    run_id: RunId,
    model_call_id: RequestId,
    attempt: u32,
) -> Result<SpanId, String> {
    if attempt == 0 {
        return Err("model_attempt_span_attempt_required".to_owned());
    }
    let digest = json_digest(&json!({
        "run_id": run_id,
        "model_call_id": model_call_id,
        "attempt": attempt,
        "kind": "kiana.model-attempt",
    }));
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| "model_attempt_span_digest_invalid".to_owned())?;
    SpanId::parse(&hex[..16])
}
