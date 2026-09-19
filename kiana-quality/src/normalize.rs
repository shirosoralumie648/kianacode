//! EQ-17 durable event selection and structural validation.

use kiana_domain::{
    event_kind_spec, validate_runtime_event, EventId, RequestId, RuntimeEvent, MAX_EVAL_EVENTS,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Version of the durable-event selection contract. Canonical JSON/redaction versions are added
/// by later normalizer steps and must not silently change this selection contract.
pub const TRACE_NORMALIZATION_VERSION: &str = "eq17.durable-events.v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableEvent {
    /// EventLog source cursor at which the immutable event was observed.
    pub source_cursor: u64,
    /// The committed RuntimeEvent. Stream deltas and UI/log observations are not representable.
    pub event: RuntimeEvent,
}

impl DurableEvent {
    pub fn new(source_cursor: u64, event: RuntimeEvent) -> Self {
        Self {
            source_cursor,
            event,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableEventSelection {
    pub normalization_version: String,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub correlation_id: RequestId,
    pub events: Vec<DurableEvent>,
    pub terminal_event_indexes: Vec<usize>,
}

impl DurableEventSelection {
    pub fn validate(&self) -> Result<(), NormalizationError> {
        TraceNormalizer::default().validate_selection(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NormalizationError {
    #[error("normalizer_empty_event_source")]
    EmptyEventSource,
    #[error("normalizer_event_limit_exceeded")]
    EventLimitExceeded,
    #[error("normalizer_source_cursor_invalid:{cursor}")]
    InvalidSourceCursor { cursor: u64 },
    #[error("normalizer_source_cursor_not_monotonic:{previous}->{next}")]
    SourceCursorNotMonotonic { previous: u64, next: u64 },
    #[error("normalizer_event_id_invalid")]
    InvalidEventId,
    #[error("normalizer_event_id_duplicate")]
    DuplicateEventId,
    #[error("normalizer_event_kind_invalid")]
    InvalidEventKind,
    #[error("normalizer_event_sequence_invalid")]
    InvalidEventSequence,
    #[error("normalizer_event_sequence_not_monotonic")]
    EventSequenceNotMonotonic,
    #[error("normalizer_stream_metadata_invalid")]
    InvalidStreamMetadata,
    #[error("normalizer_stream_version_invalid")]
    InvalidStreamVersion,
    #[error("normalizer_stream_version_not_monotonic")]
    StreamVersionNotMonotonic,
    #[error("normalizer_event_validation:{0}")]
    EventValidation(String),
    #[error("normalizer_correlation_missing")]
    CorrelationMissing,
    #[error("normalizer_correlation_mismatch")]
    CorrelationMismatch,
    #[error("normalizer_causal_reference_missing")]
    CausalReferenceMissing,
    #[error("normalizer_terminal_conflict:{stream}:{previous}:{next}")]
    TerminalConflict {
        stream: String,
        previous: String,
        next: String,
    },
    #[error("normalizer_selection_version_invalid")]
    SelectionVersionInvalid,
    #[error("normalizer_selection_cursor_invalid")]
    SelectionCursorInvalid,
    #[error("normalizer_selection_correlation_invalid")]
    SelectionCorrelationInvalid,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum StreamKey {
    Aggregate(String, String),
    Request(RequestId),
}

impl StreamKey {
    fn from_event(event: &RuntimeEvent) -> Result<Self, NormalizationError> {
        match (
            event.aggregate_type.as_deref(),
            event.aggregate_id.as_deref(),
        ) {
            (Some(aggregate_type), Some(aggregate_id))
                if !aggregate_type.trim().is_empty() && !aggregate_id.trim().is_empty() =>
            {
                Ok(Self::Aggregate(
                    aggregate_type.to_owned(),
                    aggregate_id.to_owned(),
                ))
            }
            (None, None) => Ok(Self::Request(event.request_id)),
            _ => Err(NormalizationError::InvalidStreamMetadata),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Aggregate(aggregate_type, aggregate_id) => {
                format!("{aggregate_type}:{aggregate_id}")
            }
            Self::Request(request_id) => format!("request:{request_id}"),
        }
    }
}

/// Stateless normalizer facade for the pure EQ-17 selection operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct TraceNormalizer;

impl TraceNormalizer {
    pub fn new() -> Self {
        Self
    }

    /// Select a bounded, source-ordered slice of committed events and validate its durable
    /// sequence, stream, terminal and correlation contracts. Cursor gaps are allowed because a
    /// run-scoped slice may be selected from a global EventLog containing other aggregates.
    pub fn select_durable_events(
        &self,
        events: &[DurableEvent],
    ) -> Result<DurableEventSelection, NormalizationError> {
        if events.is_empty() {
            return Err(NormalizationError::EmptyEventSource);
        }
        if events.len() > MAX_EVAL_EVENTS {
            return Err(NormalizationError::EventLimitExceeded);
        }

        let mut event_ids = BTreeSet::new();
        let mut last_request_sequences = BTreeMap::new();
        let mut last_stream_versions = BTreeMap::new();
        let mut terminal_by_stream = BTreeMap::new();
        let mut correlation_id = None;
        let mut terminal_event_indexes = Vec::new();
        let mut previous_cursor = None;

        for (index, durable) in events.iter().enumerate() {
            if durable.source_cursor == 0 {
                return Err(NormalizationError::InvalidSourceCursor {
                    cursor: durable.source_cursor,
                });
            }
            if let Some(previous) = previous_cursor {
                if durable.source_cursor <= previous {
                    return Err(NormalizationError::SourceCursorNotMonotonic {
                        previous,
                        next: durable.source_cursor,
                    });
                }
            }
            previous_cursor = Some(durable.source_cursor);

            let event = &durable.event;
            if event.event_id.as_uuid().is_nil() {
                return Err(NormalizationError::InvalidEventId);
            }
            if !event_ids.insert(event.event_id) {
                return Err(NormalizationError::DuplicateEventId);
            }
            if event.kind.trim().is_empty() {
                return Err(NormalizationError::InvalidEventKind);
            }
            if event.sequence == 0 {
                return Err(NormalizationError::InvalidEventSequence);
            }
            event
                .validate_identity_links()
                .map_err(NormalizationError::EventValidation)?;
            event
                .validate_redaction_metadata()
                .map_err(NormalizationError::EventValidation)?;
            validate_runtime_event(event).map_err(NormalizationError::EventValidation)?;

            let event_correlation = event
                .correlation_id
                .ok_or(NormalizationError::CorrelationMissing)?;
            if let Some(expected) = correlation_id {
                if expected != event_correlation {
                    return Err(NormalizationError::CorrelationMismatch);
                }
            } else {
                correlation_id = Some(event_correlation);
            }

            if let Some(previous) = last_request_sequences.insert(event.request_id, event.sequence)
            {
                if event.sequence <= previous {
                    return Err(NormalizationError::EventSequenceNotMonotonic);
                }
            }

            let stream = StreamKey::from_event(event)?;
            let stream_version = event.stream_version.unwrap_or(event.sequence);
            if stream_version == 0 {
                return Err(NormalizationError::InvalidStreamVersion);
            }
            if let Some(previous) = last_stream_versions.insert(stream.clone(), stream_version) {
                if stream_version <= previous {
                    return Err(NormalizationError::StreamVersionNotMonotonic);
                }
            }

            if event_kind_spec(&event.kind).is_some_and(|spec| spec.terminal) {
                if let Some((previous_index, previous_kind)) =
                    terminal_by_stream.insert(stream.clone(), (index, event.kind.clone()))
                {
                    return Err(NormalizationError::TerminalConflict {
                        stream: stream.label(),
                        previous: format!("{previous_kind}@{previous_index}"),
                        next: format!("{}@{index}", event.kind),
                    });
                }
                terminal_event_indexes.push(index);
            }
        }

        for durable in events {
            if durable
                .event
                .causation_event_id
                .is_some_and(|event_id| !event_ids.contains(&event_id))
                || durable
                    .event
                    .parent_event_id
                    .is_some_and(|event_id| !event_ids.contains(&event_id))
            {
                return Err(NormalizationError::CausalReferenceMissing);
            }
        }

        let correlation_id = correlation_id.ok_or(NormalizationError::CorrelationMissing)?;
        let source_cursor_start = events
            .first()
            .map(|event| event.source_cursor)
            .ok_or(NormalizationError::EmptyEventSource)?;
        let source_cursor_end = events
            .last()
            .map(|event| event.source_cursor)
            .ok_or(NormalizationError::EmptyEventSource)?;
        let selection = DurableEventSelection {
            normalization_version: TRACE_NORMALIZATION_VERSION.to_owned(),
            source_cursor_start,
            source_cursor_end,
            correlation_id,
            events: events.to_owned(),
            terminal_event_indexes,
        };
        self.validate_selection(&selection)?;
        Ok(selection)
    }

    /// EQ-17's public normalization entry point. Later steps extend the result with canonical
    /// JSON, redaction and explicitly declared volatile replacement; they must retain this gate.
    pub fn normalize(
        &self,
        events: &[DurableEvent],
    ) -> Result<DurableEventSelection, NormalizationError> {
        self.select_durable_events(events)
    }

    pub fn validate_selection(
        &self,
        selection: &DurableEventSelection,
    ) -> Result<(), NormalizationError> {
        if selection.normalization_version != TRACE_NORMALIZATION_VERSION {
            return Err(NormalizationError::SelectionVersionInvalid);
        }
        if selection.events.is_empty()
            || selection.events.len() > MAX_EVAL_EVENTS
            || selection.source_cursor_start == 0
            || selection.source_cursor_end < selection.source_cursor_start
            || selection.source_cursor_start != selection.events[0].source_cursor
            || selection.source_cursor_end
                != selection.events[selection.events.len() - 1].source_cursor
        {
            return Err(NormalizationError::SelectionCursorInvalid);
        }
        if selection.correlation_id.as_uuid().is_nil() {
            return Err(NormalizationError::SelectionCorrelationInvalid);
        }
        if selection
            .terminal_event_indexes
            .iter()
            .any(|index| *index >= selection.events.len())
        {
            return Err(NormalizationError::SelectionCursorInvalid);
        }
        Ok(())
    }
}
