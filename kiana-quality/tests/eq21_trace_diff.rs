use kiana_domain::RequestId;
use kiana_quality::{
    ArrayPolicy, TraceDiff, TraceDiffClass, VolatileEvent, VolatileEventTrace,
    CANONICAL_NORMALIZATION_VERSION, VOLATILE_NORMALIZATION_VERSION,
};
use serde_json::json;

fn trace(state: &str) -> VolatileEventTrace {
    VolatileEventTrace {
        normalization_version: VOLATILE_NORMALIZATION_VERSION.to_owned(),
        source_normalization_version: CANONICAL_NORMALIZATION_VERSION.to_owned(),
        array_policy: ArrayPolicy::Ordered,
        source_cursor_start: 10,
        source_cursor_end: 11,
        correlation_id: RequestId::new(),
        events: vec![
            VolatileEvent {
                source_cursor: 10,
                kind: "run.started".to_owned(),
                value: json!({
                    "kind": "run.started",
                    "source_cursor": 10,
                    "data": {"state": state}
                }),
            },
            VolatileEvent {
                source_cursor: 11,
                kind: "run.completed".to_owned(),
                value: json!({
                    "kind": "run.completed",
                    "source_cursor": 11,
                    "data": {"state": "completed"}
                }),
            },
        ],
        terminal_event_indexes: vec![1],
        replacement_count: 0,
        replacements: Vec::new(),
    }
}

#[test]
fn first_nested_field_divergence_is_stable_and_bounded() {
    let expected = trace("queued");
    let actual = trace("running");
    let diff = TraceDiff::compare(&expected, &actual);
    assert_eq!(diff.classification, TraceDiffClass::FieldMismatch);
    let divergence = diff.first_divergence.expect("first divergence");
    assert_eq!(divergence.event_index, Some(0));
    assert_eq!(divergence.source_cursor, Some(10));
    assert_eq!(divergence.field_path, "events[0].data.state");
    assert_eq!(divergence.expected_summary, "\"queued\"");
    assert_eq!(divergence.actual_summary, "\"running\"");
}

#[test]
fn cursor_kind_length_and_version_failures_are_classified() {
    let expected = trace("queued");
    let mut cursor = trace("queued");
    cursor.events[0].source_cursor = 99;
    assert_eq!(
        TraceDiff::compare(&expected, &cursor).classification,
        TraceDiffClass::CursorMismatch
    );

    let mut kind = trace("queued");
    kind.events[0].kind = "run.prompt".to_owned();
    assert_eq!(
        TraceDiff::compare(&expected, &kind).classification,
        TraceDiffClass::EventKindMismatch
    );

    let mut extra = trace("queued");
    extra.events.push(VolatileEvent {
        source_cursor: 12,
        kind: "run.delta".to_owned(),
        value: json!({"kind": "run.delta", "source_cursor": 12}),
    });
    assert_eq!(
        TraceDiff::compare(&expected, &extra).classification,
        TraceDiffClass::UnexpectedEvent
    );

    let mut incompatible = trace("queued");
    incompatible.normalization_version = "eq20.other.v1".to_owned();
    assert_eq!(
        TraceDiff::compare(&expected, &incompatible).classification,
        TraceDiffClass::NotComparable
    );
}

#[test]
fn identical_traces_ignore_provenance_request_id_but_not_terminal_metadata() {
    let expected = trace("queued");
    let actual = trace("queued");
    assert!(TraceDiff::compare(&expected, &actual).is_identical());

    let mut terminal = trace("queued");
    terminal.terminal_event_indexes.clear();
    assert_eq!(
        TraceDiff::compare(&expected, &terminal).classification,
        TraceDiffClass::TerminalMismatch
    );
}

#[test]
fn divergence_summary_is_redacted() {
    let mut expected = trace("queued");
    let mut actual = trace("queued");
    expected.events[0].value["data"]["text"] = json!("Authorization: Bearer expected-secret");
    actual.events[0].value["data"]["text"] = json!("Authorization: Bearer actual-secret");
    let diff = TraceDiff::compare(&expected, &actual);
    let divergence = diff.first_divergence.expect("first divergence");
    assert!(!divergence.expected_summary.contains("expected-secret"));
    assert!(!divergence.actual_summary.contains("actual-secret"));
}
