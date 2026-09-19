//! EQ-18 canonical JSON, field whitelisting and trace redaction.

use crate::{DurableEvent, DurableEventSelection, NormalizationError, TraceNormalizer};
use kiana_domain::{
    canonical_journal_bytes, event_kind_spec, redact_with_profile, DataClass, EventId,
    RedactionProfile, RedactionSignal, RequestId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const CANONICAL_NORMALIZATION_VERSION: &str = "eq18.canonical-events.v1";

const CANONICAL_EVENT_FIELDS: &[&str] = &[
    "aggregate_id",
    "aggregate_type",
    "artifact_refs",
    "causation_event_id",
    "correlation_id",
    "data",
    "data_epoch",
    "event_id",
    "idempotency_key",
    "kind",
    "parent_event_id",
    "payload_recoverable",
    "redaction_profile",
    "request_id",
    "sequence",
    "source_cursor",
    "stream_version",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrayPolicy {
    /// Preserve declared event array order.
    Ordered,
    /// Sort each array by its redacted canonical element bytes for multiset comparison.
    Multiset,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalEvent {
    pub source_cursor: u64,
    pub event_id: EventId,
    pub kind: String,
    pub value: Value,
}

impl CanonicalEvent {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalizationError> {
        canonical_journal_bytes(&self.value).map_err(CanonicalizationError::CanonicalEncodingFailed)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalEventTrace {
    pub normalization_version: String,
    pub array_policy: ArrayPolicy,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub correlation_id: RequestId,
    pub events: Vec<CanonicalEvent>,
    pub terminal_event_indexes: Vec<usize>,
}

impl CanonicalEventTrace {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalizationError> {
        canonical_journal_bytes(self).map_err(CanonicalizationError::CanonicalEncodingFailed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CanonicalizationError {
    #[error("canonical_selection:{0}")]
    Selection(#[from] NormalizationError),
    #[error("canonical_payload_object_required:{kind}")]
    PayloadObjectRequired { kind: String },
    #[error("canonical_payload_unknown_field:{kind}:{field}")]
    PayloadUnknownField { kind: String, field: String },
    #[error("canonical_redaction:{0}")]
    Redaction(String),
    #[error("canonical_encoding:{0}")]
    CanonicalEncodingFailed(String),
    #[error("canonical_decode_failed")]
    CanonicalDecodeFailed,
}

fn redaction_profile() -> RedactionProfile {
    RedactionProfile::new(
        RedactionSignal::Trace,
        DataClass::Internal,
        kiana_domain::MAX_REDACTION_VALUE_BYTES,
        kiana_domain::MAX_REDACTION_DEPTH,
    )
    .expect("built-in trace redaction profile is valid")
}

fn whitelist_payload(kind: &str, value: &Value) -> Result<Value, CanonicalizationError> {
    let Some(object) = value.as_object() else {
        return Err(CanonicalizationError::PayloadObjectRequired {
            kind: kind.to_owned(),
        });
    };
    if let Some(spec) = event_kind_spec(kind) {
        for field in object.keys() {
            if !spec.allowed_fields.iter().any(|allowed| allowed == field) {
                return Err(CanonicalizationError::PayloadUnknownField {
                    kind: kind.to_owned(),
                    field: field.to_owned(),
                });
            }
        }
    }
    Ok(Value::Object(object.clone()))
}

fn canonicalize_arrays(value: Value, policy: ArrayPolicy) -> Result<Value, CanonicalizationError> {
    match value {
        Value::Array(values) => {
            let mut normalized = values
                .into_iter()
                .map(|value| canonicalize_arrays(value, policy))
                .collect::<Result<Vec<_>, _>>()?;
            if policy == ArrayPolicy::Multiset {
                let mut with_bytes = normalized
                    .into_iter()
                    .map(|value| {
                        let bytes = canonical_journal_bytes(&value)
                            .map_err(CanonicalizationError::CanonicalEncodingFailed)?;
                        Ok((bytes, value))
                    })
                    .collect::<Result<Vec<_>, CanonicalizationError>>()?;
                with_bytes.sort_by(|left, right| left.0.cmp(&right.0));
                normalized = with_bytes.into_iter().map(|(_, value)| value).collect();
            }
            Ok(Value::Array(normalized))
        }
        Value::Object(fields) => {
            let mut ordered = BTreeMap::new();
            for (key, value) in fields {
                ordered.insert(key, canonicalize_arrays(value, policy)?);
            }
            Ok(Value::Object(ordered.into_iter().collect::<Map<_, _>>()))
        }
        other => Ok(other),
    }
}

fn canonical_event_value(
    durable: &DurableEvent,
    policy: ArrayPolicy,
) -> Result<Value, CanonicalizationError> {
    let event = &durable.event;
    let payload = whitelist_payload(&event.kind, &event.data)?;
    let mut fields = BTreeMap::new();
    fields.insert(
        "aggregate_id",
        serde_json::to_value(&event.aggregate_id).unwrap(),
    );
    fields.insert(
        "aggregate_type",
        serde_json::to_value(&event.aggregate_type).unwrap(),
    );
    fields.insert(
        "artifact_refs",
        serde_json::to_value(&event.artifact_refs).unwrap(),
    );
    fields.insert(
        "causation_event_id",
        serde_json::to_value(&event.causation_event_id).unwrap(),
    );
    fields.insert(
        "correlation_id",
        serde_json::to_value(&event.correlation_id).unwrap(),
    );
    fields.insert("data", payload);
    fields.insert(
        "data_epoch",
        serde_json::to_value(event.data_epoch).unwrap(),
    );
    fields.insert("event_id", serde_json::to_value(event.event_id).unwrap());
    fields.insert(
        "idempotency_key",
        serde_json::to_value(&event.idempotency_key).unwrap(),
    );
    fields.insert("kind", Value::String(event.kind.clone()));
    fields.insert(
        "parent_event_id",
        serde_json::to_value(&event.parent_event_id).unwrap(),
    );
    fields.insert(
        "payload_recoverable",
        serde_json::to_value(event.payload_recoverable).unwrap(),
    );
    fields.insert(
        "redaction_profile",
        serde_json::to_value(&event.redaction_profile).unwrap(),
    );
    fields.insert(
        "request_id",
        serde_json::to_value(event.request_id).unwrap(),
    );
    fields.insert("sequence", Value::from(event.sequence));
    fields.insert("source_cursor", Value::from(durable.source_cursor));
    fields.insert(
        "stream_version",
        serde_json::to_value(event.stream_version).unwrap(),
    );
    debug_assert_eq!(fields.len(), CANONICAL_EVENT_FIELDS.len());
    debug_assert!(fields
        .keys()
        .all(|field| CANONICAL_EVENT_FIELDS.contains(field)));

    let whitelisted = Value::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    );
    let redacted = redact_with_profile(&redaction_profile(), &whitelisted)
        .map_err(CanonicalizationError::Redaction)?;
    let canonical = canonicalize_arrays(redacted, policy)?;
    let bytes = canonical_journal_bytes(&canonical)
        .map_err(CanonicalizationError::CanonicalEncodingFailed)?;
    serde_json::from_slice(&bytes).map_err(|_| CanonicalizationError::CanonicalDecodeFailed)
}

/// Canonicalize a JSON value with sorted object keys and an explicit array policy. This function
/// is intentionally independent from event selection so EQ-18 can be used for later evidence
/// objects without inventing a second JSON canonicalizer.
pub fn canonical_json(value: Value, policy: ArrayPolicy) -> Result<Value, CanonicalizationError> {
    let redacted = redact_with_profile(&redaction_profile(), &value)
        .map_err(CanonicalizationError::Redaction)?;
    canonicalize_arrays(redacted, policy)
}

impl TraceNormalizer {
    pub fn canonicalize(
        &self,
        events: &[DurableEvent],
        policy: ArrayPolicy,
    ) -> Result<CanonicalEventTrace, CanonicalizationError> {
        let selection = self.select_durable_events(events)?;
        self.canonicalize_selection(&selection, policy)
    }

    pub fn normalize_canonical(
        &self,
        events: &[DurableEvent],
        policy: ArrayPolicy,
    ) -> Result<CanonicalEventTrace, CanonicalizationError> {
        self.canonicalize(events, policy)
    }

    pub fn canonicalize_selection(
        &self,
        selection: &DurableEventSelection,
        policy: ArrayPolicy,
    ) -> Result<CanonicalEventTrace, CanonicalizationError> {
        selection.validate()?;
        let events = selection
            .events
            .iter()
            .map(|event| {
                Ok(CanonicalEvent {
                    source_cursor: event.source_cursor,
                    event_id: event.event.event_id,
                    kind: event.event.kind.clone(),
                    value: canonical_event_value(event, policy)?,
                })
            })
            .collect::<Result<Vec<_>, CanonicalizationError>>()?;
        Ok(CanonicalEventTrace {
            normalization_version: CANONICAL_NORMALIZATION_VERSION.to_owned(),
            array_policy: policy,
            source_cursor_start: selection.source_cursor_start,
            source_cursor_end: selection.source_cursor_end,
            correlation_id: selection.correlation_id,
            events,
            terminal_event_indexes: selection.terminal_event_indexes.clone(),
        })
    }
}
