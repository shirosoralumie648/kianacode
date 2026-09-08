//! Shared, side-effect-free append validation for event-store adapters.
//!
//! The in-memory and JSONL stores differ only in synchronization and
//! persistence.  Keeping the stream and idempotency rules here prevents the
//! adapters from drifting while leaving their storage-specific work local.

use kiana_domain::RuntimeEvent;
use kiana_ports::PortError;

#[derive(Debug, PartialEq)]
pub(crate) enum AppendPlan {
    Append(RuntimeEvent),
    Replay(RuntimeEvent),
}

/// Validate a non-idempotent append before the adapter mutates storage.
pub(crate) fn plan_append(
    events: &[RuntimeEvent],
    event: RuntimeEvent,
    expected_version: Option<u64>,
) -> Result<RuntimeEvent, PortError> {
    reject_expected_version(events, &event, expected_version)?;
    reject_conflicts(events, &event)?;
    Ok(event)
}

pub(crate) fn plan_idempotent_append(
    events: &[RuntimeEvent],
    event: RuntimeEvent,
    expected_version: Option<u64>,
) -> Result<AppendPlan, PortError> {
    // Resolve retries before CAS so a committed request can replay after the
    // stream has advanced.
    let key = validate_idempotency_key(&event)?;
    if let Some(existing) = events
        .iter()
        .find(|existing| existing.idempotency_key.as_deref() == Some(key))
    {
        ensure_idempotent_match(existing, &event)?;
        return Ok(AppendPlan::Replay(existing.clone()));
    }

    plan_append(events, event, expected_version).map(AppendPlan::Append)
}

/// Validate an idempotency key before an adapter takes its storage lock.
///
/// Adapters historically rejected malformed idempotent requests before
/// waiting on their lock; keeping this small boundary helper public within the
/// crate preserves that error ordering after the extraction.
pub(crate) fn validate_idempotency_key(event: &RuntimeEvent) -> Result<&str, PortError> {
    event
        .idempotency_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| PortError::Failed("event_idempotency_key_required".to_owned()))
}

pub(crate) fn reject_conflicts(
    events: &[RuntimeEvent],
    event: &RuntimeEvent,
) -> Result<(), PortError> {
    if events
        .iter()
        .any(|existing| existing.event_id == event.event_id)
    {
        return Err(PortError::Conflict("event_id_duplicate".to_owned()));
    }
    let version = stream_version(event);
    if events
        .iter()
        .any(|existing| same_stream(existing, event) && stream_version(existing) >= version)
    {
        return Err(PortError::Conflict(
            "event_sequence_not_monotonic".to_owned(),
        ));
    }
    Ok(())
}

fn reject_expected_version(
    events: &[RuntimeEvent],
    event: &RuntimeEvent,
    expected_version: Option<u64>,
) -> Result<(), PortError> {
    let Some(expected_version) = expected_version else {
        return Ok(());
    };
    let current_version = events
        .iter()
        .filter(|existing| same_stream(existing, event))
        .map(stream_version)
        .max()
        .unwrap_or(0);
    if current_version != expected_version {
        return Err(PortError::Conflict(
            "event_stream_version_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn same_stream(left: &RuntimeEvent, right: &RuntimeEvent) -> bool {
    let left_has_metadata = left.aggregate_type.is_some() || left.aggregate_id.is_some();
    let right_has_metadata = right.aggregate_type.is_some() || right.aggregate_id.is_some();
    if left_has_metadata || right_has_metadata {
        return matches!(
            (
                left.aggregate_type.as_deref(),
                left.aggregate_id.as_deref(),
                right.aggregate_type.as_deref(),
                right.aggregate_id.as_deref(),
            ),
            (Some(left_type), Some(left_id), Some(right_type), Some(right_id))
                if left_type == right_type && left_id == right_id
        );
    }
    left.request_id == right.request_id
}

fn stream_version(event: &RuntimeEvent) -> u64 {
    event.stream_version.unwrap_or(event.sequence)
}

fn ensure_idempotent_match(
    existing: &RuntimeEvent,
    candidate: &RuntimeEvent,
) -> Result<(), PortError> {
    let same_payload = existing.request_id == candidate.request_id
        && existing.sequence == candidate.sequence
        && existing.kind == candidate.kind
        && existing.data == candidate.data
        && existing.aggregate_type == candidate.aggregate_type
        && existing.aggregate_id == candidate.aggregate_id
        && existing.stream_version == candidate.stream_version;
    if !same_payload {
        return Err(PortError::Conflict(
            "event_idempotency_key_payload_mismatch".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::RequestId;
    use serde_json::Value;

    fn event(request_id: RequestId, sequence: u64, kind: &str) -> RuntimeEvent {
        RuntimeEvent::new(request_id, sequence, kind, Value::Null).unwrap()
    }

    #[test]
    fn plans_append_and_replay_without_mutating_storage() {
        let request_id = RequestId::new();
        let candidate = event(request_id, 1, "accepted").with_idempotency_key("request:1");
        let plan = plan_idempotent_append(&[], candidate.clone(), Some(0)).unwrap();
        assert_eq!(plan, AppendPlan::Append(candidate.clone()));

        let replay = plan_idempotent_append(
            std::slice::from_ref(&candidate),
            candidate.clone(),
            Some(99),
        )
        .unwrap();
        assert_eq!(replay, AppendPlan::Replay(candidate));
    }

    #[test]
    fn preserves_cas_and_payload_conflicts() {
        let request_id = RequestId::new();
        let first = event(request_id, 1, "accepted").with_idempotency_key("request:1");
        let stale = event(request_id, 2, "completed").with_idempotency_key("request:2");
        assert_eq!(
            plan_idempotent_append(std::slice::from_ref(&first), stale, Some(0)).unwrap_err(),
            PortError::Conflict("event_stream_version_mismatch".to_owned())
        );

        let changed = event(request_id, 1, "different").with_idempotency_key("request:1");
        assert_eq!(
            plan_idempotent_append(&[first], changed, None).unwrap_err(),
            PortError::Conflict("event_idempotency_key_payload_mismatch".to_owned())
        );
    }

    #[test]
    fn requires_keys_only_for_idempotent_appends() {
        let request_id = RequestId::new();
        let candidate = event(request_id, 1, "accepted");
        assert!(plan_append(&[], candidate.clone(), None).is_ok());
        assert_eq!(
            plan_idempotent_append(&[], candidate, None).unwrap_err(),
            PortError::Failed("event_idempotency_key_required".to_owned())
        );
    }
}
