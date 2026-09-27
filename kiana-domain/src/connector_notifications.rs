//! INT-27 connector notification payload gate over the existing notification materializer.
//!
//! Connector health, invocation, reconciliation and approval messages are projections of
//! committed EventLog facts. This validator requires source cursor/evidence/limitation/dedupe
//! metadata and redacted summaries; it does not create a notification, deliver it or mutate a
//! HumanTask.

use crate::{
    redact_text, validate_notification_event, NotificationEventClass, NotificationEventSource,
};
use serde_json::Value;

pub const CONNECTOR_NOTIFICATION_KINDS: &[&str] = &[
    "connector.health",
    "connector.invocation",
    "connector.reconciliation",
    "connector.approval",
];

pub fn validate_connector_notification_payload(
    kind: &str,
    payload: &Value,
) -> Result<NotificationEventClass, String> {
    if !CONNECTOR_NOTIFICATION_KINDS.contains(&kind) {
        return Err("connector_notification_kind_unregistered".to_owned());
    }
    let class = validate_notification_event(kind, NotificationEventSource::EventLog, payload)?;
    let object = payload
        .as_object()
        .ok_or_else(|| "connector_notification_payload_object_required".to_owned())?;
    for field in ["connector_id", "binding_id", "dedup_key", "limitation"] {
        if object
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty() || redact_text(value) != value)
        {
            return Err(format!("connector_notification_{field}_required"));
        }
    }
    if object
        .get("source_cursor")
        .and_then(Value::as_u64)
        .is_none_or(|cursor| cursor == 0)
    {
        return Err("connector_notification_source_cursor_required".to_owned());
    }
    let evidence = object
        .get("evidence_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| "connector_notification_evidence_required".to_owned())?;
    if !valid_digest(evidence) {
        return Err("connector_notification_evidence_invalid".to_owned());
    }
    let summary = object
        .get("summary")
        .and_then(Value::as_str)
        .ok_or_else(|| "connector_notification_summary_required".to_owned())?;
    if summary.trim().is_empty() || redact_text(summary) != summary {
        return Err("connector_notification_summary_invalid".to_owned());
    }
    Ok(class)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
