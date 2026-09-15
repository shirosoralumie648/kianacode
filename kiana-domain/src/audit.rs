//! Server-derived audit taxonomy and committed-event reducer.
//!
//! Audit rows are projections of immutable EventLog facts.  This module intentionally has no
//! writer, broker, provider, UI or exporter dependency: callers provide the already committed
//! events and an explicit journal cursor, and the reducer either returns a complete append-only
//! batch or fails closed.  In particular, an event payload can never submit an `audit.*` row or
//! replace an existing row.

use crate::{
    encode_bounded_value, redact_text_with_profile, AuditActionKind, AuditDecision, AuditRecord,
    DataClass, EventCursor, RedactionProfile, RedactionSignal, RequestId, RuntimeEvent,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// The actor attached to every row produced by this reducer.  Payload actor fields are
/// diagnostic input only and are never copied into the audit record.
pub const SERVER_AUDIT_ACTOR: &str = "server:control-plane";
/// Bounded source-event batches keep a projection replay deterministic and memory bounded.
pub const MAX_AUDIT_REDUCE_EVENTS: usize = 4_096;

/// A stable action/decision pair for one registered RuntimeEvent kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditEventSpec {
    pub action_kind: AuditActionKind,
    pub decision: AuditDecision,
    pub target_kind: &'static str,
}

impl AuditEventSpec {
    pub const fn new(
        action_kind: AuditActionKind,
        decision: AuditDecision,
        target_kind: &'static str,
    ) -> Self {
        Self {
            action_kind,
            decision,
            target_kind,
        }
    }
}

fn spec(
    action_kind: AuditActionKind,
    decision: AuditDecision,
    target_kind: &'static str,
) -> Option<AuditEventSpec> {
    Some(AuditEventSpec::new(action_kind, decision, target_kind))
}

fn declared_decision(data: &Value) -> Option<&str> {
    data.get("decision")
        .and_then(Value::as_str)
        .or_else(|| data.get("outcome").and_then(Value::as_str))
}

fn decision_matches(declared: &str, expected: AuditDecision) -> bool {
    let normalized = declared.trim().to_ascii_lowercase();
    match expected {
        AuditDecision::Accepted => matches!(
            normalized.as_str(),
            "accepted" | "accept" | "allowed" | "allow" | "authorized" | "completed" | "success"
        ),
        AuditDecision::Denied => matches!(
            normalized.as_str(),
            "denied" | "deny" | "blocked" | "rejected" | "revoked"
        ),
        AuditDecision::Staged => matches!(
            normalized.as_str(),
            "staged" | "requested" | "awaiting_approval" | "ask"
        ),
        AuditDecision::Approved => matches!(normalized.as_str(), "approved" | "approve"),
        AuditDecision::Consumed => matches!(
            normalized.as_str(),
            "consumed" | "completed" | "reconciled" | "success"
        ),
        AuditDecision::Failed => matches!(
            normalized.as_str(),
            "failed" | "failure" | "cancelled" | "canceled"
        ),
        AuditDecision::Unknown => matches!(normalized.as_str(), "unknown" | "result_unknown"),
        AuditDecision::Queried => matches!(
            normalized.as_str(),
            "queried" | "query" | "completed" | "success"
        ),
        AuditDecision::Exported => {
            matches!(normalized.as_str(), "exported" | "completed" | "success")
        }
    }
}

fn capability_decision(data: &Value) -> Result<AuditDecision, String> {
    let gate = data.get("gate");
    let nested = gate
        .and_then(|value| value.get("decision"))
        .and_then(Value::as_str);
    let direct = data.get("decision").and_then(Value::as_str);
    let value = nested
        .or(direct)
        .ok_or_else(|| "audit_capability_decision_required".to_owned())?;
    let normalized = value.trim().to_ascii_lowercase();
    let decision = match normalized.as_str() {
        "allowed" | "allow" | "accepted" | "authorized" => AuditDecision::Accepted,
        "denied" | "deny" | "blocked" | "rejected" => AuditDecision::Denied,
        "awaiting_approval" | "ask" | "staged" | "requested" => AuditDecision::Staged,
        _ => return Err("audit_capability_decision_unknown".to_owned()),
    };
    if let (Some(nested), Some(direct)) = (nested, direct) {
        if !decision_matches(direct, decision) || !decision_matches(nested, decision) {
            return Err("audit_decision_conflict".to_owned());
        }
    }
    Ok(decision)
}

/// Classify one registered event kind.  Unknown non-audit events remain opaque and are ignored;
/// unknown/self-submitted `audit.*` events are rejected because they cannot establish provenance.
pub fn classify_audit_event(kind: &str, data: &Value) -> Result<Option<AuditEventSpec>, String> {
    if kind.trim().is_empty() || kind.len() > 256 {
        return Err("audit_event_kind_invalid".to_owned());
    }
    if kind.starts_with("audit.") {
        if kind == "audit.correction" {
            return Err("audit_correction_requires_append".to_owned());
        }
        return Err("audit_event_kind_untrusted".to_owned());
    }

    let result = match kind {
        "request.accepted" | "command.accepted" | "command.authorized" => {
            spec(AuditActionKind::Command, AuditDecision::Accepted, "command")
        }
        "command.rejected" | "run.rejected" => {
            spec(AuditActionKind::Command, AuditDecision::Denied, "command")
        }
        "command.completed" | "run.completed" => {
            spec(AuditActionKind::Command, AuditDecision::Accepted, "command")
        }
        "command.failed" | "run.failed" | "run.cancelled" => {
            spec(AuditActionKind::Command, AuditDecision::Failed, "command")
        }
        "run.result_unknown" => spec(AuditActionKind::Command, AuditDecision::Unknown, "command"),
        "run.authorized" | "authorization.accepted" => spec(
            AuditActionKind::Authorization,
            AuditDecision::Accepted,
            "authorization",
        ),
        "authorization.denied" => spec(
            AuditActionKind::Authorization,
            AuditDecision::Denied,
            "authorization",
        ),
        "approval.requested" => spec(AuditActionKind::Approval, AuditDecision::Staged, "approval"),
        "approval.approved" => spec(
            AuditActionKind::Approval,
            AuditDecision::Approved,
            "approval",
        ),
        "approval.denied" => spec(AuditActionKind::Approval, AuditDecision::Denied, "approval"),
        "approval.consumed" => spec(
            AuditActionKind::Approval,
            AuditDecision::Consumed,
            "approval",
        ),
        "approval.failed" | "approval.continuation_unavailable" => {
            spec(AuditActionKind::Approval, AuditDecision::Failed, "approval")
        }
        "run.capability_requested" => spec(
            AuditActionKind::Capability,
            AuditDecision::Staged,
            "capability",
        ),
        "run.capability_blocked" | "capability.blocked" => spec(
            AuditActionKind::Capability,
            AuditDecision::Denied,
            "capability",
        ),
        "capability.decision" => Some(AuditEventSpec::new(
            AuditActionKind::Capability,
            capability_decision(data)?,
            "capability",
        )),
        "capability.completed" => spec(
            AuditActionKind::Capability,
            AuditDecision::Consumed,
            "capability",
        ),
        "capability.failed" | "capability.cancelled" => spec(
            AuditActionKind::Capability,
            AuditDecision::Failed,
            "capability",
        ),
        "capability.result_unknown" => spec(
            AuditActionKind::Capability,
            AuditDecision::Unknown,
            "capability",
        ),
        "credential.accepted" | "credential.created" => spec(
            AuditActionKind::Credential,
            AuditDecision::Accepted,
            "credential",
        ),
        "credential.rotated" => spec(
            AuditActionKind::Credential,
            AuditDecision::Consumed,
            "credential",
        ),
        "credential.revoked" => spec(
            AuditActionKind::Credential,
            AuditDecision::Denied,
            "credential",
        ),
        "credential.consumed" => spec(
            AuditActionKind::Credential,
            AuditDecision::Consumed,
            "credential",
        ),
        "credential.failed" => spec(
            AuditActionKind::Credential,
            AuditDecision::Failed,
            "credential",
        ),
        "credential.result_unknown" => spec(
            AuditActionKind::Credential,
            AuditDecision::Unknown,
            "credential",
        ),
        "recovery.requested" => spec(AuditActionKind::Recovery, AuditDecision::Staged, "recovery"),
        "recovery.started" => spec(
            AuditActionKind::Recovery,
            AuditDecision::Accepted,
            "recovery",
        ),
        "recovery.reconciled" | "recovery.completed" => spec(
            AuditActionKind::Recovery,
            AuditDecision::Consumed,
            "recovery",
        ),
        "recovery.failed" => spec(AuditActionKind::Recovery, AuditDecision::Failed, "recovery"),
        "recovery.result_unknown" => spec(
            AuditActionKind::Recovery,
            AuditDecision::Unknown,
            "recovery",
        ),
        "query.completed" | "query.succeeded" => {
            spec(AuditActionKind::Query, AuditDecision::Queried, "query")
        }
        "query.denied" | "query.failed" => {
            spec(AuditActionKind::Query, AuditDecision::Denied, "query")
        }
        "query.result_unknown" => spec(AuditActionKind::Query, AuditDecision::Unknown, "query"),
        "export.completed" | "export.succeeded" => {
            spec(AuditActionKind::Export, AuditDecision::Exported, "export")
        }
        "export.failed" => spec(AuditActionKind::Export, AuditDecision::Failed, "export"),
        "export.result_unknown" => spec(AuditActionKind::Export, AuditDecision::Unknown, "export"),
        _ => None,
    };

    if let Some(spec) = result {
        if let Some(declared) = declared_decision(data) {
            if kind != "capability.decision" && !decision_matches(declared, spec.decision) {
                return Err("audit_decision_conflict".to_owned());
            }
        }
    }
    Ok(result)
}

fn string_field<'a>(data: &'a Value, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| data.get(*name).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_epoch(data: &Value, names: &[&str], field: &str) -> Result<u64, String> {
    for name in names {
        let Some(value) = data.get(*name) else {
            continue;
        };
        let parsed = value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()));
        return parsed
            .filter(|epoch| *epoch > 0)
            .ok_or_else(|| format!("audit_{field}_invalid"));
    }
    Err(format!("audit_{field}_required"))
}

fn validate_sha256(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("audit_{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("audit_{field}_invalid"));
    }
    Ok(())
}

fn optional_digest(data: &Value, field: &str) -> Result<Option<String>, String> {
    let Some(value) = data.get(field) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| format!("audit_{field}_invalid"))?
        .trim();
    validate_sha256(value, field)?;
    Ok(Some(value.to_owned()))
}

fn optional_request_id(data: &Value, field: &str) -> Result<Option<RequestId>, String> {
    let Some(value) = data.get(field) else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .ok_or_else(|| format!("audit_{field}_invalid"))?;
    let uuid = Uuid::parse_str(text.trim()).map_err(|_| format!("audit_{field}_invalid"))?;
    Ok(Some(RequestId::from_uuid(uuid)))
}

fn target_binding(
    event: &RuntimeEvent,
    event_spec: AuditEventSpec,
) -> Result<(String, String), String> {
    let payload_target = string_field(
        &event.data,
        &[
            "target_ref",
            "run_id",
            "invocation_id",
            "execution_id",
            "capability_request_id",
            "approval_id",
            "command_id",
            "credential_id",
            "recovery_id",
            "query_id",
            "export_id",
        ],
    );
    let metadata = match (&event.aggregate_type, &event.aggregate_id) {
        (Some(kind), Some(id)) => {
            if kind.trim().is_empty() || id.trim().is_empty() {
                return Err("audit_target_binding_required".to_owned());
            }
            Some((kind.trim().to_owned(), id.trim().to_owned()))
        }
        (None, None) => None,
        _ => return Err("audit_target_binding_incomplete".to_owned()),
    };
    if let (Some((kind, id)), Some(payload)) = (&metadata, payload_target) {
        if kind == "run"
            && event
                .data
                .get("run_id")
                .and_then(Value::as_str)
                .is_some_and(|run_id| run_id.trim() != id)
        {
            return Err("audit_target_binding_conflict".to_owned());
        }
        if id != payload && event.data.get("target_ref").is_some() {
            return Err("audit_target_binding_conflict".to_owned());
        }
        if kind == event_spec.target_kind && id != payload {
            // A typed aggregate (for example `approval`) must not be paired with a different
            // explicit target. Generic run aggregates may carry a nested capability ID.
            return Err("audit_target_binding_conflict".to_owned());
        }
    }
    let (kind, reference) = metadata.unwrap_or_else(|| {
        (
            event_spec.target_kind.to_owned(),
            payload_target.unwrap_or_default().to_owned(),
        )
    });
    if reference.trim().is_empty() {
        return Err("audit_target_binding_required".to_owned());
    }
    if kind.len() > 128 || reference.len() > 512 {
        return Err("audit_target_binding_too_long".to_owned());
    }
    Ok((kind, reference))
}

fn validate_payload_actor(data: &Value) -> Result<(), String> {
    for field in ["actor_ref", "audit_actor_ref", "actor"] {
        let Some(value) = data.get(field) else {
            continue;
        };
        let actor = value
            .as_str()
            .ok_or_else(|| "audit_actor_untrusted".to_owned())?;
        if !actor.starts_with("server:") && !actor.starts_with("principal:") {
            return Err("audit_actor_untrusted".to_owned());
        }
    }
    if data.get("audit_record").is_some() || data.get("record_digest").is_some() {
        return Err("audit_record_payload_untrusted".to_owned());
    }
    Ok(())
}

fn optional_reason(data: &Value, profile: &RedactionProfile) -> Result<Option<String>, String> {
    let Some(value) = ["reason_code", "reason", "error_code", "error"]
        .iter()
        .find_map(|field| data.get(*field))
    else {
        return Ok(None);
    };
    let reason = value
        .as_str()
        .ok_or_else(|| "audit_reason_invalid".to_owned())?;
    redact_text_with_profile(profile, reason).map(Some)
}

fn data_class(data: &Value) -> Result<DataClass, String> {
    let Some(value) = data.get("data_class") else {
        return Ok(DataClass::Internal);
    };
    serde_json::from_value(value.clone()).map_err(|_| "audit_data_class_invalid".to_owned())
}

fn retention_class(data: &Value) -> Result<String, String> {
    let value = string_field(data, &["retention_class"]).unwrap_or("audit");
    if value.len() > 64 {
        return Err("audit_retention_class_too_long".to_owned());
    }
    Ok(value.to_owned())
}

fn logical_key(event: &RuntimeEvent, target_ref: &str, event_spec: AuditEventSpec) -> String {
    if let Some(key) = string_field(&event.data, &["audit_id"]) {
        return format!("audit_id:{key}");
    }
    if let Some(key) = event
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        return format!("idempotency:{key}");
    }
    format!(
        "event:{}:{}:{target_ref}",
        event_spec.action_kind as u8, event.event_id
    )
}

fn record_for_event(
    event: &RuntimeEvent,
    event_spec: AuditEventSpec,
    source_cursor: EventCursor,
    profile: &RedactionProfile,
) -> Result<AuditRecord, String> {
    if event.sequence == 0 {
        return Err("audit_source_event_invalid".to_owned());
    }
    validate_payload_actor(&event.data)?;
    let (target_kind, target_ref) = target_binding(event, event_spec)?;
    let authority_epoch = parse_epoch(
        &event.data,
        &["authority_epoch", "authority_revision"],
        "authority_epoch",
    )?;
    let data_epoch = parse_epoch(&event.data, &["data_epoch", "data_revision"], "data_epoch")?;
    let data_class = data_class(&event.data)?;
    let retention_class = retention_class(&event.data)?;
    let encoded = encode_bounded_value(profile, &event.data)?;
    let action_digest = crate::json_digest(&json!({
        "event_kind": event.kind,
        "data": encoded.value,
    }));
    let mut record = AuditRecord::new(
        format!("audit:{}", event.event_id),
        event_spec.action_kind,
        event_spec.decision,
        SERVER_AUDIT_ACTOR,
        target_kind,
        target_ref,
        source_cursor,
        vec![event.event_id],
        authority_epoch,
        data_epoch,
        data_class,
        retention_class,
    )?;
    record.request_id = Some(event.request_id);
    record.command_id = optional_request_id(&event.data, "command_id")?;
    record.correlation_ref =
        string_field(&event.data, &["correlation_ref", "correlation_id"]).map(str::to_owned);
    record.causation_ref =
        string_field(&event.data, &["causation_ref", "causation_id"]).map(str::to_owned);
    record.action_digest = Some(action_digest);
    record.input_digest = optional_digest(&event.data, "input_digest")?;
    record.reason_code = optional_reason(&event.data, profile)?;
    record.attributes = BTreeMap::from([
        ("event_kind".to_owned(), event.kind.clone()),
        (
            "decision".to_owned(),
            serde_json::to_value(event_spec.decision)
                .map_err(|_| "audit_encode_failed".to_owned())?
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
        ),
        (
            "redaction_profile".to_owned(),
            profile.profile_digest.clone(),
        ),
    ]);
    record.record_digest = record.digest();
    record.validate()?;
    Ok(record)
}

/// Reduce committed runtime events into append-only audit rows.
///
/// `source_cursor` is the first committed cursor represented by `events`; each subsequent event
/// receives the checked contiguous cursor `source_cursor + index`.  No rows are returned for
/// unknown non-audit event kinds.  Duplicate source IDs, logical-key reuse and contradictory
/// decisions are rejected instead of overwriting an earlier record.
pub fn reduce_audit_records(
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
) -> Result<Vec<AuditRecord>, String> {
    if source_cursor == 0 {
        return Err("audit_source_cursor_required".to_owned());
    }
    if events.len() > MAX_AUDIT_REDUCE_EVENTS {
        return Err("audit_reduce_event_limit".to_owned());
    }
    let profile = RedactionProfile::for_signal(RedactionSignal::Audit);
    let mut source_ids = BTreeSet::new();
    let mut logical_decisions = BTreeMap::<String, AuditDecision>::new();
    let mut records = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if !source_ids.insert(event.event_id.to_string()) {
            return Err("audit_source_event_duplicate".to_owned());
        }
        let cursor = source_cursor
            .checked_add(index as u64)
            .ok_or_else(|| "audit_source_cursor_overflow".to_owned())?;
        let Some(event_spec) = classify_audit_event(&event.kind, &event.data)? else {
            continue;
        };
        let (target_kind, target_ref) = target_binding(event, event_spec)?;
        let key = logical_key(event, &target_ref, event_spec);
        if let Some(previous) = logical_decisions.insert(key, event_spec.decision) {
            if previous != event_spec.decision {
                return Err("audit_decision_conflict".to_owned());
            }
            return Err("audit_record_duplicate".to_owned());
        }
        let record = record_for_event(event, event_spec, cursor, &profile)?;
        // Keep the target binding check visible to the reducer without trusting payload values.
        if record.target_kind != target_kind || record.target_ref != target_ref {
            return Err("audit_target_binding_conflict".to_owned());
        }
        records.push(record);
    }
    Ok(records)
}

/// Compatibility spelling for callers that name the operation after the projection.
pub fn reduce_committed_audit_records(
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
) -> Result<Vec<AuditRecord>, String> {
    reduce_audit_records(events, source_cursor)
}

/// Append newly reduced rows to an existing projection without replacing any original fact.
/// Existing rows are validated first; a source ID or generated audit ID collision is an explicit
/// conflict rather than an update or last-write-wins replacement.
pub fn append_audit_records(
    existing: &[AuditRecord],
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
) -> Result<Vec<AuditRecord>, String> {
    let mut audit_ids = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    for record in existing {
        record.validate()?;
        if !audit_ids.insert(record.audit_id.clone()) {
            return Err("audit_record_duplicate".to_owned());
        }
        for event_id in &record.source_event_ids {
            if !source_ids.insert(event_id.to_string()) {
                return Err("audit_source_event_duplicate".to_owned());
            }
        }
    }
    let appended = reduce_audit_records(events, source_cursor)?;
    for record in &appended {
        if !audit_ids.insert(record.audit_id.clone()) {
            return Err("audit_record_conflict".to_owned());
        }
        for event_id in &record.source_event_ids {
            if !source_ids.insert(event_id.to_string()) {
                return Err("audit_source_event_conflict".to_owned());
            }
        }
    }
    let mut result = existing.to_vec();
    result.extend(appended);
    Ok(result)
}
