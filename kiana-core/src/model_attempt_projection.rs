//! Event-derived provider/model/stream/usage observations.
//!
//! `run.model_turn` is the only input accepted here.  The reducer is intentionally read-only and
//! fail-closed: a malformed, truncated, timed-out or retrying attempt can be inspected, but it
//! can never become an `ok` record.  Prompt text, provider headers, endpoints and raw responses
//! are never copied into the projection.

use kiana_domain::{
    json_digest, redact_text, EventId, ModelAttemptRecord, ModelCacheUsage, ModelFinish,
    ModelPurpose, ModelRetryClass, ModelUsage, RequestId, RunId, RuntimeEvent, SpanId, TraceId,
    TraceStatus, TurnId,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::HashSet;

use super::{ControlPlane, CoreError};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ModelAttemptProjectionError {
    #[error("model_attempt_event_run_id_invalid")]
    EventRunIdInvalid,
    #[error("model_attempt_event_run_id_conflict")]
    EventRunIdConflict,
    #[error("model_attempt_event_field_invalid:{0}")]
    EventFieldInvalid(&'static str),
    #[error("model_attempt_cursor_overflow")]
    CursorOverflow,
    #[error("model_attempt_record_invalid:{0}")]
    RecordInvalid(String),
}

fn event_run_id(event: &RuntimeEvent) -> Result<Option<RunId>, ModelAttemptProjectionError> {
    let aggregate = match (
        event.aggregate_type.as_deref(),
        event.aggregate_id.as_deref(),
    ) {
        (Some("run"), Some(value)) => {
            Some(RunId::parse_str(value).ok_or(ModelAttemptProjectionError::EventRunIdInvalid)?)
        }
        (Some("run"), None) => return Err(ModelAttemptProjectionError::EventRunIdInvalid),
        _ => None,
    };
    let payload = match event.data.get("run_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => {
            Some(RunId::parse_str(value).ok_or(ModelAttemptProjectionError::EventRunIdInvalid)?)
        }
        Some(_) => return Err(ModelAttemptProjectionError::EventRunIdInvalid),
    };
    if let (Some(payload), Some(aggregate)) = (payload, aggregate) {
        if payload != aggregate {
            return Err(ModelAttemptProjectionError::EventRunIdConflict);
        }
    }
    Ok(payload.or(aggregate))
}

fn optional_field<T: DeserializeOwned>(
    event: &RuntimeEvent,
    name: &'static str,
) -> Result<Option<T>, ModelAttemptProjectionError> {
    match event.data.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| ModelAttemptProjectionError::EventFieldInvalid(name)),
    }
}

fn id_field(event: &RuntimeEvent, name: &'static str, fallback: RequestId) -> (RequestId, bool) {
    match event.data.get(name) {
        None | Some(Value::Null) => (fallback, false),
        Some(value) => serde_json::from_value::<RequestId>(value.clone())
            .map(|id| (id, false))
            .unwrap_or((fallback, true)),
    }
}

fn turn_for(
    event: &RuntimeEvent,
    current: Option<TurnId>,
) -> Result<Option<TurnId>, ModelAttemptProjectionError> {
    if let Some(turn_id) = optional_field(event, "turn_id")? {
        return Ok(Some(turn_id));
    }
    Ok(current)
}

fn safe_label(value: Option<&Value>, fallback: &str, max_bytes: usize) -> (String, bool) {
    let Some(value) = value.and_then(Value::as_str) else {
        return (fallback.to_owned(), true);
    };
    let value = redact_text(value.trim());
    if value.is_empty()
        || value.len() > max_bytes
        || value.contains('\0')
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
        })
    {
        return (fallback.to_owned(), true);
    }
    (value, false)
}

fn is_sha256(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_prompt_hash(value: &str) -> bool {
    if is_sha256(value) {
        return true;
    }
    value
        .strip_prefix("fnv1a64:")
        .is_some_and(|hex| hex.len() == 16 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn route_digest(data: &Value, provider_id: &str, model_id: &str) -> String {
    for value in [
        data.get("route_digest"),
        data.pointer("/prepared/route_digest"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    {
        if is_sha256(value) {
            return value.to_owned();
        }
    }
    let route = data.pointer("/prepared/route");
    let route_identity = json!({
        "provider_id": route
            .and_then(|value| value.get("provider_id"))
            .and_then(Value::as_str)
            .unwrap_or(provider_id),
        "protocol": route
            .and_then(|value| value.get("protocol"))
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        "model_id": route
            .and_then(|value| value.get("model_id"))
            .and_then(Value::as_str)
            .unwrap_or(model_id),
        "profile": route
            .and_then(|value| value.get("profile"))
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        "configuration_revision": route
            .and_then(|value| value.get("configuration_revision"))
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        "streaming": route
            .and_then(|value| value.get("streaming"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    });
    json_digest(&route_identity)
}

fn prompt_version(data: &Value) -> Option<String> {
    for value in [
        data.get("prompt_version"),
        data.pointer("/prepared/prompt_version"),
        data.pointer("/prepared/request_hash"),
        data.get("prompt_hash"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .filter(|value| !value.trim().is_empty())
    {
        if is_prompt_hash(value) {
            return Some(value.to_owned());
        }
        // A compatibility event may carry a textual version.  Hash it before projection so a
        // legacy producer cannot smuggle prompt text into telemetry.
        return Some(json_digest(&json!({
            "prompt_version": redact_text(value),
        })));
    }
    let hashes = data
        .get("prompt_sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|source| source.get("prompt_hash"))
        .filter_map(Value::as_str)
        .filter(|hash| is_prompt_hash(hash))
        .collect::<Vec<_>>();
    (!hashes.is_empty()).then(|| json_digest(&json!({ "prompt_hashes": hashes })))
}

fn parse_purpose(data: &Value) -> (ModelPurpose, bool) {
    for value in [data.get("purpose"), data.pointer("/prepared/purpose")]
        .into_iter()
        .flatten()
    {
        return serde_json::from_value(value.clone())
            .map(|purpose| (purpose, false))
            .unwrap_or((ModelPurpose::Task, true));
    }
    (ModelPurpose::Task, true)
}

fn parse_finish(data: &Value) -> (Option<ModelFinish>, bool) {
    let Some(value) = data
        .get("finish")
        .filter(|value| !value.is_null())
        .or_else(|| data.get("stop_reason").filter(|value| !value.is_null()))
    else {
        return (None, false);
    };
    if let Ok(finish) = serde_json::from_value::<ModelFinish>(value.clone()) {
        return (Some(finish), false);
    }
    let Some(reason) = value.as_str() else {
        return (None, true);
    };
    match reason {
        "stop" | "end_turn" | "completed" => (Some(ModelFinish::EndTurn), false),
        "tool_use" | "tool_calls" | "requires_action" => (Some(ModelFinish::ToolUse), false),
        "length" | "max_tokens" | "MAX_TOKENS" => (Some(ModelFinish::Length), false),
        "refusal" | "content_filter" | "SAFETY" => (Some(ModelFinish::Refusal), false),
        "pause_turn" => (Some(ModelFinish::Pause), false),
        "incomplete" => (Some(ModelFinish::Incomplete), false),
        _ => (None, true),
    }
}

fn parse_usage(data: &Value) -> (Option<ModelUsage>, bool) {
    let Some(value) = data.get("usage").filter(|value| !value.is_null()) else {
        return (None, false);
    };
    let Ok(usage) = serde_json::from_value::<ModelUsage>(value.clone()) else {
        return (None, true);
    };
    if usage.input_tokens > kiana_domain::MAX_MODEL_USAGE_TOKENS
        || usage.output_tokens > kiana_domain::MAX_MODEL_USAGE_TOKENS
        || usage
            .input_tokens
            .checked_add(usage.output_tokens)
            .is_none()
    {
        return (None, true);
    }
    (Some(usage), false)
}

fn parse_usage_complete(data: &Value, usage: Option<&ModelUsage>) -> (bool, bool) {
    match data.get("usage_complete") {
        None | Some(Value::Null) => (usage.is_some(), false),
        Some(Value::Bool(value)) => (*value && usage.is_some(), false),
        Some(_) => (false, true),
    }
}

fn parse_streaming(data: &Value) -> (Option<bool>, bool) {
    if let Some(value) = data.get("streaming") {
        return value
            .as_bool()
            .map(|value| (Some(value), false))
            .unwrap_or((None, true));
    }
    let streaming = data
        .pointer("/prepared/route/streaming")
        .and_then(Value::as_bool);
    (streaming, streaming.is_none())
}

fn parse_latency(data: &Value) -> (Option<u64>, bool) {
    match data.get("elapsed_ms") {
        None | Some(Value::Null) => (None, false),
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= 86_400_000)
            .map(|value| (Some(value), false))
            .unwrap_or((None, true)),
    }
}

fn parse_retry_class(data: &Value, error_code: Option<&str>) -> (Option<ModelRetryClass>, bool) {
    for value in [data.get("retry_class"), data.pointer("/error/retry_class")]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_null())
    {
        let parsed = serde_json::from_value::<ModelRetryClass>(value.clone()).ok();
        return (parsed, parsed.is_none());
    }
    let derived = error_code.and_then(|code| {
        if code.contains("provider_http_429") || code.contains("provider_http_503") {
            Some(ModelRetryClass::Rejected)
        } else {
            None
        }
    });
    (derived, false)
}

fn parse_cache_usage(data: &Value) -> (Option<ModelCacheUsage>, bool) {
    let Some(value) = data
        .get("cache_usage")
        .or_else(|| data.get("cache_hit"))
        .filter(|value| !value.is_null())
    else {
        return (None, false);
    };
    if let Some(hit) = value.as_bool() {
        return (
            Some(if hit {
                ModelCacheUsage::Hit
            } else {
                ModelCacheUsage::Miss
            }),
            false,
        );
    }
    match value.as_str().map(str::to_ascii_lowercase).as_deref() {
        Some("hit") => (Some(ModelCacheUsage::Hit), false),
        Some("miss") => (Some(ModelCacheUsage::Miss), false),
        Some("not_requested" | "not-requested") => (Some(ModelCacheUsage::NotRequested), false),
        Some("unknown") => (Some(ModelCacheUsage::Unknown), false),
        _ => (Some(ModelCacheUsage::Unknown), true),
    }
}

fn error_code(data: &Value) -> (Option<String>, bool) {
    let Some(error) = data.get("error").filter(|value| !value.is_null()) else {
        let Some(value) = data.get("error_code").and_then(Value::as_str) else {
            return (None, false);
        };
        return safe_error_code(value);
    };
    let value = error
        .get("code")
        .and_then(Value::as_str)
        .or_else(|| data.get("error_code").and_then(Value::as_str))
        .or_else(|| error.as_str());
    match value {
        Some(value) => safe_error_code(value),
        None => (Some("model_error_unclassified".to_owned()), true),
    }
}

fn safe_error_code(value: &str) -> (Option<String>, bool) {
    let candidate = value
        .trim()
        .split(|character: char| matches!(character, ':' | ' ' | '\n' | '\t' | '/'))
        .next()
        .unwrap_or_default();
    if candidate.is_empty()
        || candidate.len() > kiana_domain::MAX_MODEL_ERROR_CODE_BYTES
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return (Some("model_error_unclassified".to_owned()), true);
    }
    (Some(candidate.to_owned()), false)
}

fn error_present(data: &Value) -> bool {
    data.get("error").is_some_and(|value| {
        !value.is_null() && (!value.is_string() || !value.as_str().unwrap_or_default().is_empty())
    }) || data
        .get("error_code")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
}

fn status_for(
    attempted: bool,
    finish: Option<ModelFinish>,
    finish_malformed: bool,
    usage_complete: bool,
    usage_malformed: bool,
    error_code: Option<&str>,
    retry_class: Option<ModelRetryClass>,
) -> TraceStatus {
    if !attempted {
        return TraceStatus::Unknown;
    }
    if retry_class.is_some_and(|class| class != ModelRetryClass::Never) {
        return TraceStatus::Unknown;
    }
    if let Some(code) = error_code {
        if [
            "timeout",
            "incomplete",
            "truncated",
            "stream",
            "read",
            "connection",
        ]
        .iter()
        .any(|fragment| code.contains(fragment))
        {
            return TraceStatus::Unknown;
        }
        return TraceStatus::Error;
    }
    if finish_malformed || usage_malformed || finish.is_none() {
        return TraceStatus::Unknown;
    }
    match finish {
        Some(ModelFinish::EndTurn | ModelFinish::ToolUse) => {
            if usage_complete {
                TraceStatus::Ok
            } else {
                TraceStatus::Unknown
            }
        }
        Some(ModelFinish::Refusal) => TraceStatus::Error,
        Some(ModelFinish::Pause) => TraceStatus::Degraded,
        Some(ModelFinish::Length | ModelFinish::Incomplete) => TraceStatus::Unknown,
        None => TraceStatus::Unknown,
    }
}

fn stable_trace_id(run_id: RunId) -> Result<TraceId, ModelAttemptProjectionError> {
    let digest = json_digest(&json!({"run_id": run_id, "kind": "kiana.trace"}));
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or(ModelAttemptProjectionError::EventFieldInvalid("trace_id"))?;
    TraceId::parse(&hex[..32]).map_err(ModelAttemptProjectionError::RecordInvalid)
}

fn stable_span_id(
    run_id: RunId,
    model_request_id: RequestId,
    attempt: u32,
) -> Result<SpanId, ModelAttemptProjectionError> {
    let digest = json_digest(&json!({
        "run_id": run_id,
        "model_request_id": model_request_id,
        "attempt": attempt,
        "kind": "kiana.model-attempt",
    }));
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or(ModelAttemptProjectionError::EventFieldInvalid("span_id"))?;
    SpanId::parse(&hex[..16]).map_err(ModelAttemptProjectionError::RecordInvalid)
}

/// Fold committed `run.model_turn` facts into bounded provider/model attempt records.
pub fn project_model_attempts(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<ModelAttemptRecord>, ModelAttemptProjectionError> {
    let trace_id = stable_trace_id(run_id)?;
    let mut records = Vec::new();
    let mut seen_event_ids = HashSet::<EventId>::new();
    let mut seen_attempts = HashSet::<(RequestId, u32)>::new();
    let mut current_turn = None;

    for (index, event) in events.iter().enumerate() {
        if !seen_event_ids.insert(event.event_id) {
            continue;
        }
        if event_run_id(event)? != Some(run_id) {
            continue;
        }
        let event_turn = turn_for(event, current_turn)?;
        if matches!(
            event.kind.as_str(),
            "run.authorized" | "run.predecessor" | "run.prompt"
        ) {
            current_turn =
                event_turn.or_else(|| Some(TurnId::from_uuid(event.request_id.as_uuid())));
        } else if let Some(turn_id) = event_turn {
            current_turn = Some(turn_id);
        }
        if event.kind != "run.model_turn" {
            continue;
        }
        let source_cursor = u64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(ModelAttemptProjectionError::CursorOverflow)?;
        let (model_call_id, call_malformed) = id_field(event, "model_call_id", event.request_id);
        let (model_request_id, request_malformed) =
            id_field(event, "model_request_id", model_call_id);
        let mut malformed = call_malformed || request_malformed;
        let attempt = match event.data.get("attempt").and_then(Value::as_u64) {
            Some(value) if value > 0 && value <= u64::from(u32::MAX) => value as u32,
            None => {
                malformed = true;
                1
            }
            Some(_) => {
                malformed = true;
                1
            }
        };
        if !seen_attempts.insert((model_request_id, attempt)) {
            continue;
        }
        let (purpose, purpose_malformed) = parse_purpose(&event.data);
        malformed |= purpose_malformed;
        let (provider_id, provider_malformed) = safe_label(
            event
                .data
                .get("provider_id")
                .or_else(|| event.data.pointer("/prepared/route/provider_id")),
            "unknown",
            kiana_domain::MAX_MODEL_PROVIDER_BYTES,
        );
        let (model_id, model_malformed) = safe_label(
            event
                .data
                .get("model_id")
                .or_else(|| event.data.pointer("/prepared/route/model_id")),
            "unknown",
            kiana_domain::MAX_MODEL_ID_BYTES,
        );
        malformed |= provider_malformed || model_malformed;
        let route_digest = route_digest(&event.data, &provider_id, &model_id);
        let prompt_version = prompt_version(&event.data);
        let (streaming, streaming_malformed) = parse_streaming(&event.data);
        malformed |= streaming_malformed;
        let attempted = event
            .data
            .get("attempted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if event.data.get("attempted").is_none() {
            malformed = true;
        }
        let (finish, finish_malformed) = parse_finish(&event.data);
        malformed |= finish_malformed;
        let (usage, usage_malformed) = parse_usage(&event.data);
        malformed |= usage_malformed;
        let (usage_complete, completion_malformed) =
            parse_usage_complete(&event.data, usage.as_ref());
        malformed |= completion_malformed;
        let (latency_ms, latency_malformed) = parse_latency(&event.data);
        malformed |= latency_malformed;
        let (error_code, error_malformed) = error_code(&event.data);
        malformed |= error_malformed;
        let error_present = error_present(&event.data);
        let (retry_class, retry_malformed) = parse_retry_class(&event.data, error_code.as_deref());
        malformed |= retry_malformed;
        let (cache_usage, cache_malformed) = parse_cache_usage(&event.data);
        malformed |= cache_malformed;
        let status = status_for(
            attempted,
            finish,
            finish_malformed,
            usage_complete,
            usage_malformed,
            error_code.as_deref().filter(|_| error_present),
            retry_class,
        );
        let mut attributes = std::collections::BTreeMap::new();
        attributes.insert(
            "usage_state".to_owned(),
            if usage_complete {
                "complete".to_owned()
            } else {
                "missing_or_incomplete".to_owned()
            },
        );
        if let Some(streaming) = streaming {
            attributes.insert("streaming".to_owned(), streaming.to_string());
        }
        if let Some(retry_class) = retry_class {
            attributes.insert(
                "retry_class".to_owned(),
                format!("{retry_class:?}").to_ascii_lowercase(),
            );
        }
        if let Some(cache_usage) = cache_usage {
            attributes.insert(
                "cache_usage".to_owned(),
                format!("{cache_usage:?}").to_ascii_lowercase(),
            );
        }
        if malformed {
            attributes.insert(
                "metadata_quality".to_owned(),
                "malformed_or_untrusted".to_owned(),
            );
        }
        let span_id = stable_span_id(run_id, model_request_id, attempt)?;
        let record = ModelAttemptRecord::new(
            trace_id.clone(),
            span_id,
            run_id,
            event_turn.or(current_turn),
            model_call_id,
            model_request_id,
            attempt,
            provider_id,
            model_id,
            route_digest,
            prompt_version,
            purpose,
            streaming,
            attempted,
            if malformed && status == TraceStatus::Ok {
                TraceStatus::Unknown
            } else {
                status
            },
            finish,
            usage,
            usage_complete,
            latency_ms,
            retry_class,
            cache_usage,
            source_cursor,
            vec![event.event_id],
            error_code,
            attributes,
        )
        .map_err(ModelAttemptProjectionError::RecordInvalid)?;
        records.push(record);
    }
    Ok(records)
}

/// Compatibility alias for callers that use provider-attempt terminology.
pub fn project_provider_attempts(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<ModelAttemptRecord>, ModelAttemptProjectionError> {
    project_model_attempts(run_id, events)
}

impl ControlPlane {
    /// Rebuild provider/model attempt observations from persisted EventLog facts only.
    pub async fn model_attempts(
        &self,
        run_id: RunId,
    ) -> Result<Vec<ModelAttemptRecord>, CoreError> {
        let events = self.events_for_persisted_run(run_id).await?;
        if events.is_empty() {
            return Err(kiana_ports::PortError::Failed("run_not_found".to_owned()).into());
        }
        project_model_attempts(run_id, &events)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }

    /// Alias for provider-focused read paths; this remains a projection, not a sink.
    pub async fn provider_attempts(
        &self,
        run_id: RunId,
    ) -> Result<Vec<ModelAttemptRecord>, CoreError> {
        self.model_attempts(run_id).await
    }
}
