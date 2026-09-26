use kiana_quality::{
    DeterministicEvaluator, RuntimeCorrectnessEvaluator, RuntimeCorrectnessInput,
    RUNTIME_CORRECTNESS_INPUT_SCHEMA,
};
use serde_json::{json, Value};

const CORRELATION: &str = "00000000-0000-0000-0000-000000000001";
const REQUEST: &str = "00000000-0000-0000-0000-000000000002";
const RUN: &str = "00000000-0000-0000-0000-000000000003";
const INVOCATION: &str = "00000000-0000-0000-0000-000000000004";
const EXECUTION: &str = "00000000-0000-0000-0000-000000000005";
const CAPABILITY_REQUEST: &str = "00000000-0000-0000-0000-000000000006";
const APPROVAL: &str = "00000000-0000-0000-0000-000000000007";

fn event(
    cursor: u64,
    sequence: u64,
    kind: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    data: Value,
) -> Value {
    let event_id = format!("00000000-0000-0000-0000-{cursor:012}");
    json!({
        "source_cursor": cursor,
        "event_id": event_id,
        "kind": kind,
        "value": {
            "aggregate_id": aggregate_id,
            "aggregate_type": aggregate_type,
            "correlation_id": CORRELATION,
            "data": data,
            "event_id": event_id,
            "kind": kind,
            "request_id": REQUEST,
            "sequence": sequence,
            "source_cursor": cursor,
        }
    })
}

fn input(events: Vec<Value>, terminal_event_indexes: Vec<usize>) -> Value {
    let source_cursor_start = events
        .first()
        .and_then(|event| event.get("source_cursor"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let source_cursor_end = events
        .last()
        .and_then(|event| event.get("source_cursor"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "schema": RUNTIME_CORRECTNESS_INPUT_SCHEMA,
        "trace": {
            "normalization_version": "eq18.canonical-events.v1",
            "array_policy": "ordered",
            "source_cursor_start": source_cursor_start,
            "source_cursor_end": source_cursor_end,
            "correlation_id": CORRELATION,
            "events": events,
            "terminal_event_indexes": terminal_event_indexes,
        }
    })
}

fn codes(findings: &[kiana_quality::Finding]) -> Vec<&str> {
    findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

#[test]
fn valid_runtime_lifecycle_has_no_findings() {
    let trace = input(
        vec![
            event(1, 1, "run.started", "run", RUN, json!({"run_id": RUN})),
            event(
                2,
                2,
                "run.tool_call",
                "run",
                RUN,
                json!({
                    "run_id": RUN,
                    "invocation_id": INVOCATION,
                    "execution_id": EXECUTION,
                    "capability_request_id": CAPABILITY_REQUEST,
                    "call_id": "call-1",
                }),
            ),
            event(
                3,
                3,
                "approval.requested",
                "approval",
                APPROVAL,
                json!({
                    "approval_id": APPROVAL,
                    "run_id": RUN,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
            event(
                4,
                4,
                "approval.approved",
                "approval",
                APPROVAL,
                json!({
                    "approval_id": APPROVAL,
                    "run_id": RUN,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
            event(
                5,
                5,
                "approval.consumed",
                "approval",
                APPROVAL,
                json!({
                    "approval_id": APPROVAL,
                    "run_id": RUN,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
            event(
                6,
                6,
                "invocation.dispatching",
                "execution_permit",
                EXECUTION,
                json!({
                    "run_id": RUN,
                    "invocation_id": INVOCATION,
                    "execution_id": EXECUTION,
                    "capability_request_id": CAPABILITY_REQUEST,
                    "call_id": "call-1",
                    "approval_id": APPROVAL,
                }),
            ),
            event(
                7,
                7,
                "capability.completed",
                "run",
                CAPABILITY_REQUEST,
                json!({
                    "run_id": RUN,
                    "invocation_id": INVOCATION,
                    "execution_id": EXECUTION,
                    "capability_request_id": CAPABILITY_REQUEST,
                    "call_id": "call-1",
                    "approval_id": APPROVAL,
                }),
            ),
            event(8, 8, "run.completed", "run", RUN, json!({"run_id": RUN})),
        ],
        vec![6, 7],
    );

    let evaluator = RuntimeCorrectnessEvaluator;
    let findings = evaluator.evaluate(&trace).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");

    let decoded: RuntimeCorrectnessInput = serde_json::from_value(trace).unwrap();
    assert_eq!(decoded.validate(), Ok(()));
}

#[test]
fn runtime_evaluator_catches_unmatched_and_duplicate_terminal() {
    let trace = input(
        vec![
            event(
                1,
                1,
                "capability.completed",
                "run",
                CAPABILITY_REQUEST,
                json!({
                    "run_id": RUN,
                    "invocation_id": INVOCATION,
                    "execution_id": EXECUTION,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
            event(
                2,
                2,
                "capability.failed",
                "run",
                CAPABILITY_REQUEST,
                json!({
                    "run_id": RUN,
                    "invocation_id": INVOCATION,
                    "execution_id": EXECUTION,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
        ],
        vec![0, 1],
    );

    let findings = RuntimeCorrectnessEvaluator.evaluate(&trace).unwrap();
    let codes = codes(&findings);
    assert!(codes.contains(&"runtime.invocation_unmatched"));
    assert!(codes.contains(&"runtime.invocation_terminal_duplicate"));
    assert!(codes.contains(&"runtime.terminal_duplicate"));
}

#[test]
fn unknown_outcome_blocks_a_later_retry_and_cancel_requires_a_fence() {
    let trace = input(
        vec![
            event(
                1,
                1,
                "run.result_unknown",
                "run",
                RUN,
                json!({"run_id": RUN}),
            ),
            event(
                2,
                2,
                "invocation.dispatching",
                "run",
                RUN,
                json!({
                    "run_id": RUN,
                    "invocation_id": INVOCATION,
                    "execution_id": EXECUTION,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
            event(3, 3, "run.cancelled", "run", RUN, json!({"run_id": RUN})),
        ],
        vec![0, 2],
    );

    let findings = RuntimeCorrectnessEvaluator.evaluate(&trace).unwrap();
    let codes = codes(&findings);
    assert!(codes.contains(&"runtime.unknown_retry_forbidden"));
    assert!(codes.contains(&"runtime.cancel_unfenced"));
}

#[test]
fn malformed_runtime_input_is_rejected_before_evaluation() {
    let input = json!({
        "schema": RUNTIME_CORRECTNESS_INPUT_SCHEMA,
        "trace": {
            "normalization_version": "eq17.durable-events.v1",
            "array_policy": "ordered",
            "source_cursor_start": 1,
            "source_cursor_end": 1,
            "correlation_id": CORRELATION,
            "events": [],
            "terminal_event_indexes": [],
        }
    });
    let error = RuntimeCorrectnessEvaluator.evaluate(&input).unwrap_err();
    assert_eq!(
        error,
        kiana_quality::EvaluatorError::InputInvalid(
            "runtime_correctness_input_normalization_invalid"
        )
    );
}
