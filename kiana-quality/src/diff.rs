//! EQ-21 deterministic first-divergence trace diff.

use crate::{ArrayPolicy, VolatileEventTrace};
use kiana_domain::{canonical_journal_bytes, redact_text};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const TRACE_DIFF_SCHEMA: &str = "kiana.quality-trace-diff.v1";
const MAX_SUMMARY_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDiffClass {
    Identical,
    NotComparable,
    CursorMismatch,
    EventKindMismatch,
    MissingEvent,
    UnexpectedEvent,
    FieldMismatch,
    TerminalMismatch,
    TraceMetadataMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceDivergence {
    pub event_index: Option<usize>,
    pub source_cursor: Option<u64>,
    pub field_path: String,
    pub classification: TraceDiffClass,
    pub expected_summary: String,
    pub actual_summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceDiff {
    pub schema: String,
    pub expected_normalization_version: String,
    pub actual_normalization_version: String,
    pub expected_array_policy: ArrayPolicy,
    pub actual_array_policy: ArrayPolicy,
    pub compared_event_count: usize,
    pub classification: TraceDiffClass,
    #[serde(default)]
    pub first_divergence: Option<TraceDivergence>,
}

impl TraceDiff {
    pub fn compare(expected: &VolatileEventTrace, actual: &VolatileEventTrace) -> Self {
        let mut diff = Self {
            schema: TRACE_DIFF_SCHEMA.to_owned(),
            expected_normalization_version: expected.normalization_version.clone(),
            actual_normalization_version: actual.normalization_version.clone(),
            expected_array_policy: expected.array_policy,
            actual_array_policy: actual.array_policy,
            compared_event_count: expected.events.len().min(actual.events.len()),
            classification: TraceDiffClass::Identical,
            first_divergence: None,
        };

        if expected.normalization_version != actual.normalization_version {
            return diff.with_divergence(TraceDivergence {
                event_index: None,
                source_cursor: None,
                field_path: "normalization_version".to_owned(),
                classification: TraceDiffClass::NotComparable,
                expected_summary: summarize(&Value::String(expected.normalization_version.clone())),
                actual_summary: summarize(&Value::String(actual.normalization_version.clone())),
            });
        }
        if expected.array_policy != actual.array_policy {
            return diff.with_divergence(TraceDivergence {
                event_index: None,
                source_cursor: None,
                field_path: "array_policy".to_owned(),
                classification: TraceDiffClass::NotComparable,
                expected_summary: format!("{:?}", expected.array_policy),
                actual_summary: format!("{:?}", actual.array_policy),
            });
        }
        if expected.source_normalization_version != actual.source_normalization_version {
            return diff.with_divergence(TraceDivergence {
                event_index: None,
                source_cursor: None,
                field_path: "source_normalization_version".to_owned(),
                classification: TraceDiffClass::NotComparable,
                expected_summary: summarize(&Value::String(
                    expected.source_normalization_version.clone(),
                )),
                actual_summary: summarize(&Value::String(
                    actual.source_normalization_version.clone(),
                )),
            });
        }

        for index in 0..diff.compared_event_count {
            let expected_event = &expected.events[index];
            let actual_event = &actual.events[index];
            if expected_event.source_cursor != actual_event.source_cursor {
                return diff.with_divergence(TraceDivergence {
                    event_index: Some(index),
                    source_cursor: Some(actual_event.source_cursor),
                    field_path: format!("events[{index}].source_cursor"),
                    classification: TraceDiffClass::CursorMismatch,
                    expected_summary: expected_event.source_cursor.to_string(),
                    actual_summary: actual_event.source_cursor.to_string(),
                });
            }
            if expected_event.kind != actual_event.kind {
                return diff.with_divergence(TraceDivergence {
                    event_index: Some(index),
                    source_cursor: Some(actual_event.source_cursor),
                    field_path: format!("events[{index}].kind"),
                    classification: TraceDiffClass::EventKindMismatch,
                    expected_summary: summarize(&Value::String(expected_event.kind.clone())),
                    actual_summary: summarize(&Value::String(actual_event.kind.clone())),
                });
            }
            if let Some((path, expected_value, actual_value)) =
                first_json_divergence(&expected_event.value, &actual_event.value, "")
            {
                return diff.with_divergence(TraceDivergence {
                    event_index: Some(index),
                    source_cursor: Some(actual_event.source_cursor),
                    field_path: format!("events[{index}].{path}"),
                    classification: TraceDiffClass::FieldMismatch,
                    expected_summary: summarize(&expected_value),
                    actual_summary: summarize(&actual_value),
                });
            }
        }

        if expected.events.len() > actual.events.len() {
            let index = actual.events.len();
            let event = &expected.events[index];
            return diff.with_divergence(TraceDivergence {
                event_index: Some(index),
                source_cursor: Some(event.source_cursor),
                field_path: format!("events[{index}]"),
                classification: TraceDiffClass::MissingEvent,
                expected_summary: summarize(&event.value),
                actual_summary: "<missing>".to_owned(),
            });
        }
        if actual.events.len() > expected.events.len() {
            let index = expected.events.len();
            let event = &actual.events[index];
            return diff.with_divergence(TraceDivergence {
                event_index: Some(index),
                source_cursor: Some(event.source_cursor),
                field_path: format!("events[{index}]"),
                classification: TraceDiffClass::UnexpectedEvent,
                expected_summary: "<missing>".to_owned(),
                actual_summary: summarize(&event.value),
            });
        }

        if expected.terminal_event_indexes != actual.terminal_event_indexes {
            return diff.with_divergence(TraceDivergence {
                event_index: None,
                source_cursor: None,
                field_path: "terminal_event_indexes".to_owned(),
                classification: TraceDiffClass::TerminalMismatch,
                expected_summary: summarize(
                    &serde_json::to_value(&expected.terminal_event_indexes).unwrap(),
                ),
                actual_summary: summarize(
                    &serde_json::to_value(&actual.terminal_event_indexes).unwrap(),
                ),
            });
        }
        if expected.replacements != actual.replacements {
            return diff.with_divergence(TraceDivergence {
                event_index: None,
                source_cursor: None,
                field_path: "replacements".to_owned(),
                classification: TraceDiffClass::TraceMetadataMismatch,
                expected_summary: summarize(&serde_json::to_value(&expected.replacements).unwrap()),
                actual_summary: summarize(&serde_json::to_value(&actual.replacements).unwrap()),
            });
        }
        for (field, expected_value, actual_value) in [
            (
                "source_cursor_start",
                Value::from(expected.source_cursor_start),
                Value::from(actual.source_cursor_start),
            ),
            (
                "source_cursor_end",
                Value::from(expected.source_cursor_end),
                Value::from(actual.source_cursor_end),
            ),
            (
                "replacement_count",
                Value::from(expected.replacement_count),
                Value::from(actual.replacement_count),
            ),
        ] {
            if expected_value != actual_value {
                return diff.with_divergence(TraceDivergence {
                    event_index: None,
                    source_cursor: None,
                    field_path: field.to_owned(),
                    classification: TraceDiffClass::TraceMetadataMismatch,
                    expected_summary: summarize(&expected_value),
                    actual_summary: summarize(&actual_value),
                });
            }
        }
        diff
    }

    pub fn is_identical(&self) -> bool {
        self.classification == TraceDiffClass::Identical
    }

    fn with_divergence(mut self, divergence: TraceDivergence) -> Self {
        self.classification = divergence.classification;
        self.first_divergence = Some(divergence);
        self
    }
}

fn first_json_divergence(
    expected: &Value,
    actual: &Value,
    path: &str,
) -> Option<(String, Value, Value)> {
    match (expected, actual) {
        (Value::Object(expected_fields), Value::Object(actual_fields)) => {
            let keys = expected_fields
                .keys()
                .chain(actual_fields.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for key in keys {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (expected_fields.get(&key), actual_fields.get(&key)) {
                    (Some(expected), Some(actual)) => {
                        if let Some(divergence) =
                            first_json_divergence(expected, actual, &child_path)
                        {
                            return Some(divergence);
                        }
                    }
                    (Some(expected), None) => {
                        return Some((
                            child_path,
                            expected.clone(),
                            Value::String("<missing>".to_owned()),
                        ))
                    }
                    (None, Some(actual)) => {
                        return Some((
                            child_path,
                            Value::String("<missing>".to_owned()),
                            actual.clone(),
                        ))
                    }
                    (None, None) => unreachable!("union key has no value"),
                }
            }
            None
        }
        (Value::Array(expected_values), Value::Array(actual_values)) => {
            let shared = expected_values.len().min(actual_values.len());
            for index in 0..shared {
                let child_path = format!("{path}[{index}]");
                if let Some(divergence) = first_json_divergence(
                    &expected_values[index],
                    &actual_values[index],
                    &child_path,
                ) {
                    return Some(divergence);
                }
            }
            if expected_values.len() != actual_values.len() {
                let index = shared;
                return Some((
                    format!("{path}[{index}]"),
                    expected_values
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| Value::String("<missing>".to_owned())),
                    actual_values
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| Value::String("<missing>".to_owned())),
                ));
            }
            None
        }
        _ if expected == actual => None,
        _ => Some((path.to_owned(), expected.clone(), actual.clone())),
    }
}

fn summarize(value: &Value) -> String {
    let redacted = kiana_domain::redact_value(value);
    let bytes = canonical_journal_bytes(&redacted).unwrap_or_else(|_| b"<encode-error>".to_vec());
    let text = redact_text(&String::from_utf8_lossy(&bytes));
    if text.len() <= MAX_SUMMARY_BYTES {
        text
    } else {
        let mut bounded = text
            .char_indices()
            .take_while(|(index, _)| *index < MAX_SUMMARY_BYTES)
            .map(|(_, character)| character)
            .collect::<String>();
        bounded.push_str("…");
        bounded
    }
}
