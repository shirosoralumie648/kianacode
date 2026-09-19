use kiana_domain::{RequestId, RuntimeEvent};
use kiana_quality::{DurableEvent, NormalizationError, TraceNormalizer};
use serde_json::json;

fn event(
    request_id: RequestId,
    sequence: u64,
    kind: &str,
    run_id: &str,
    correlation_id: RequestId,
) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, kind, json!({"run_id": run_id}))
        .expect("valid runtime event")
        .with_stream_metadata("run", run_id, sequence)
        .with_identity_links(None, Some(correlation_id), None, None)
}

fn durable(source_cursor: u64, event: RuntimeEvent) -> DurableEvent {
    DurableEvent::new(source_cursor, event)
}

#[test]
fn durable_selection_preserves_source_order_and_accepts_global_cursor_gaps() {
    let request_id = RequestId::new();
    let correlation_id = RequestId::new();
    let selected = TraceNormalizer::new()
        .select_durable_events(&[
            durable(
                7,
                event(request_id, 1, "run.started", "run-1", correlation_id),
            ),
            durable(
                11,
                event(request_id, 2, "run.completed", "run-1", correlation_id),
            ),
        ])
        .expect("valid durable selection");

    assert_eq!(selected.source_cursor_start, 7);
    assert_eq!(selected.source_cursor_end, 11);
    assert_eq!(selected.terminal_event_indexes, vec![1]);
    assert_eq!(selected.correlation_id, correlation_id);
}

#[test]
fn invalid_event_cursor_or_multiple_terminal_is_rejected() {
    let request_id = RequestId::new();
    let correlation_id = RequestId::new();
    let invalid_cursor = TraceNormalizer::new().select_durable_events(&[durable(
        0,
        event(request_id, 1, "run.started", "run-2", correlation_id),
    )]);
    assert_eq!(
        invalid_cursor.unwrap_err(),
        NormalizationError::InvalidSourceCursor { cursor: 0 }
    );

    let multiple_terminal = TraceNormalizer::new().select_durable_events(&[
        durable(
            1,
            event(request_id, 1, "run.completed", "run-2", correlation_id),
        ),
        durable(
            2,
            event(request_id, 2, "run.failed", "run-2", correlation_id),
        ),
    ]);
    assert!(matches!(
        multiple_terminal,
        Err(NormalizationError::TerminalConflict { .. })
    ));
}

#[test]
fn sequence_and_correlation_drift_fail_closed() {
    let request_id = RequestId::new();
    let correlation_id = RequestId::new();
    let drifted_correlation = TraceNormalizer::new().select_durable_events(&[
        durable(
            1,
            event(request_id, 1, "run.started", "run-3", correlation_id),
        ),
        durable(
            2,
            event(request_id, 2, "run.prompt", "run-3", RequestId::new()),
        ),
    ]);
    assert_eq!(
        drifted_correlation.unwrap_err(),
        NormalizationError::CorrelationMismatch
    );

    let non_monotonic = TraceNormalizer::new().select_durable_events(&[
        durable(
            1,
            event(request_id, 2, "run.started", "run-4", correlation_id),
        ),
        durable(
            2,
            event(request_id, 1, "run.prompt", "run-4", correlation_id),
        ),
    ]);
    assert_eq!(
        non_monotonic.unwrap_err(),
        NormalizationError::EventSequenceNotMonotonic
    );
}

#[test]
fn causation_reference_must_be_inside_selected_durable_slice() {
    let request_id = RequestId::new();
    let correlation_id = RequestId::new();
    let first = event(request_id, 1, "run.started", "run-5", correlation_id);
    let mut second = event(request_id, 2, "run.prompt", "run-5", correlation_id);
    second.causation_event_id = Some(kiana_domain::EventId::new());
    let result =
        TraceNormalizer::new().select_durable_events(&[durable(1, first), durable(2, second)]);
    assert_eq!(
        result.unwrap_err(),
        NormalizationError::CausalReferenceMissing
    );
}
