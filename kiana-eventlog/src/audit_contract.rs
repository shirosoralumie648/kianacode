//! EventStore boundary validation for server-derived audit facts.
//!
//! `AuditRecord` rows are projections of committed facts.  A caller can therefore not append an
//! arbitrary `audit.*` RuntimeEvent and make it look like a server decision.  The one accepted
//! audit kind carries a strict, digest-bound `AuditRecordEvent` envelope and repeats the metadata
//! that the EventStore must see before it mutates either memory or durable JSONL state.

use kiana_domain::{AuditRecordEvent, RuntimeEvent, AUDIT_EVENT_KIND};

/// Validate the EventStore-only audit envelope before any CAS, idempotency replay or write.
pub(crate) fn validate_runtime_event(event: &RuntimeEvent) -> Result<(), String> {
    if !event.kind.starts_with("audit.") {
        return Ok(());
    }
    if event.kind != AUDIT_EVENT_KIND {
        return Err("eventlog_audit_kind_untrusted".to_owned());
    }

    event
        .validate_redaction_metadata()
        .map_err(|error| format!("eventlog_audit_redaction:{error}"))?;
    let envelope: AuditRecordEvent = serde_json::from_value(event.data.clone())
        .map_err(|_| "eventlog_audit_payload_invalid".to_owned())?;
    envelope
        .validate()
        .map_err(|error| format!("eventlog_audit_envelope:{error}"))?;

    if event.redaction_profile.as_deref() != Some(envelope.redaction_profile.as_str()) {
        return Err("eventlog_audit_redaction_binding_mismatch".to_owned());
    }
    if event.payload_recoverable != Some(false) {
        return Err("eventlog_audit_payload_recoverable".to_owned());
    }
    if event.data_epoch != Some(envelope.record.data_epoch) {
        return Err("eventlog_audit_data_epoch_mismatch".to_owned());
    }
    if !event.artifact_refs.is_empty() {
        return Err("eventlog_audit_artifact_refs_forbidden".to_owned());
    }

    let expected_key = format!("audit-record:{}", envelope.record.audit_id);
    if event.idempotency_key.as_deref() != Some(expected_key.as_str()) {
        return Err("eventlog_audit_idempotency_binding_mismatch".to_owned());
    }
    Ok(())
}
