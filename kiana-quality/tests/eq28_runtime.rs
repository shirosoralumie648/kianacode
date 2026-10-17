//! EQ-28 runtime correctness fixtures.
//!
//! Rejection paths come first. Every lifecycle family named by the card - event order,
//! invocation correlation, terminal, retry, approval, cancel and Unknown - has a dedicated
//! rejection fixture before the success paths that keep the full evidence spine green.

use kiana_quality::{
    DeterministicEvaluator, EvaluatorError, EvaluatorRegistry, Finding,
    RuntimeCorrectnessEvaluator, RuntimeCorrectnessInput, RUNTIME_CORRECTNESS_EVALUATOR_ID,
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
const SECOND_APPROVAL: &str = "00000000-0000-0000-0000-000000000008";
const THIRD_APPROVAL: &str = "00000000-0000-0000-0000-000000000009";
const UNKNOWN_APPROVAL: &str = "00000000-0000-0000-0000-00000000000a";
const SECOND_CAPABILITY_REQUEST: &str = "00000000-0000-0000-0000-00000000000b";
const SECOND_INVOCATION: &str = "00000000-0000-0000-0000-00000000000c";
const SECOND_EXECUTION: &str = "00000000-0000-0000-0000-00000000000d";
const MODEL_CALL: &str = "00000000-0000-0000-0000-00000000000e";
const FOREIGN_CORRELATION: &str = "00000000-0000-0000-0000-0000000000ff";

fn event_with_correlation(
    cursor: u64,
    sequence: u64,
    kind: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    data: Value,
    correlation_id: &str,
) -> Value {
    let event_id = format!("00000000-0000-0000-0000-{cursor:012}");
    json!({
        "source_cursor": cursor,
        "event_id": event_id,
        "kind": kind,
        "value": {
            "aggregate_id": aggregate_id,
            "aggregate_type": aggregate_type,
            "correlation_id": correlation_id,
            "data": data,
            "event_id": event_id,
            "kind": kind,
            "request_id": REQUEST,
            "sequence": sequence,
            "source_cursor": cursor,
        }
    })
}

fn event(
    cursor: u64,
    sequence: u64,
    kind: &str,
    aggregate_type: &str,
    aggregate_id: &str,
    data: Value,
) -> Value {
    event_with_correlation(
        cursor,
        sequence,
        kind,
        aggregate_type,
        aggregate_id,
        data,
        CORRELATION,
    )
}

fn raw_event(cursor: u64, kind: &str, value: Value) -> Value {
    let event_id = format!("00000000-0000-0000-0000-{cursor:012}");
    json!({
        "source_cursor": cursor,
        "event_id": event_id,
        "kind": kind,
        "value": value,
    })
}

fn invocation_data(
    capability_request_id: &str,
    invocation_id: &str,
    execution_id: &str,
    call_id: &str,
) -> Value {
    json!({
        "run_id": RUN,
        "invocation_id": invocation_id,
        "execution_id": execution_id,
        "capability_request_id": capability_request_id,
        "call_id": call_id,
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

fn codes(findings: &[Finding]) -> Vec<&str> {
    findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

fn evaluate(trace: &Value) -> Vec<Finding> {
    RuntimeCorrectnessEvaluator
        .evaluate(trace)
        .expect("runtime correctness input is valid")
}

fn assert_contains(findings: &[Finding], expected: &[&str]) {
    let observed = codes(findings);
    for code in expected {
        assert!(
            observed.contains(code),
            "missing finding {code}; observed {observed:?}"
        );
    }
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
        vec![4, 6, 7],
    );

    let findings = evaluate(&trace);
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");

    let decoded: RuntimeCorrectnessInput = serde_json::from_value(trace).unwrap();
    assert_eq!(decoded.validate(), Ok(()));
}

#[test]
fn post_terminal_projections_and_additional_attempts_are_not_findings() {
    // The execution spine deliberately writes run.receipt, run.snapshot and cell.* facts after a
    // terminal run event, and a run may contain several capability attempts.  None of those may
    // be reported as "events after terminal".
    let trace = input(
        vec![
            event(1, 1, "run.started", "run", RUN, json!({"run_id": RUN})),
            event(
                2,
                2,
                "run.tool_call",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(
                3,
                3,
                "capability.completed",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(
                4,
                4,
                "run.tool_call",
                "run",
                RUN,
                invocation_data(
                    SECOND_CAPABILITY_REQUEST,
                    SECOND_INVOCATION,
                    SECOND_EXECUTION,
                    "call-2",
                ),
            ),
            event(
                5,
                5,
                "capability.completed",
                "run",
                RUN,
                invocation_data(
                    SECOND_CAPABILITY_REQUEST,
                    SECOND_INVOCATION,
                    SECOND_EXECUTION,
                    "call-2",
                ),
            ),
            event(6, 6, "run.completed", "run", RUN, json!({"run_id": RUN})),
            event(7, 7, "run.receipt", "run", RUN, json!({"run_id": RUN})),
            event(8, 8, "run.snapshot", "run", RUN, json!({"run_id": RUN})),
            event(9, 9, "cell.retired", "run", RUN, json!({"run_id": RUN})),
        ],
        vec![2, 4, 5],
    );

    let findings = evaluate(&trace);
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
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
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(
                2,
                2,
                "capability.failed",
                "run",
                CAPABILITY_REQUEST,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
        ],
        vec![0, 1],
    );

    let findings = evaluate(&trace);
    assert_contains(
        &findings,
        &[
            "runtime.invocation_unmatched",
            "runtime.invocation_terminal_duplicate",
            "runtime.terminal_duplicate",
            "runtime.terminal_conflict",
        ],
    );
}

#[test]
fn events_after_run_terminal_are_rejected() {
    let trace = input(
        vec![
            event(1, 1, "run.started", "run", RUN, json!({"run_id": RUN})),
            event(2, 2, "run.completed", "run", RUN, json!({"run_id": RUN})),
            event(3, 3, "run.prompt", "run", RUN, json!({"run_id": RUN})),
            event(
                4,
                4,
                "run.capability_requested",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
        ],
        vec![1],
    );

    let findings = evaluate(&trace);
    assert_contains(
        &findings,
        &[
            "runtime.event_after_terminal",
            "runtime.invocation_terminal_missing",
        ],
    );
    let after_terminal = codes(&findings)
        .into_iter()
        .filter(|code| *code == "runtime.event_after_terminal")
        .count();
    assert!(
        after_terminal >= 2,
        "late lifecycle and invocation events must both be reported: {findings:?}"
    );
}

#[test]
fn event_order_and_correlation_regressions_are_rejected() {
    let trace = input(
        vec![
            event(1, 1, "run.started", "run", RUN, json!({"run_id": RUN})),
            event(2, 1, "run.prompt", "run", RUN, json!({"run_id": RUN})),
            event(2, 3, "run.delta", "run", RUN, json!({"run_id": RUN})),
            event_with_correlation(
                3,
                4,
                "run.compacted",
                "run",
                RUN,
                json!({"run_id": RUN}),
                FOREIGN_CORRELATION,
            ),
        ],
        vec![],
    );

    let findings = evaluate(&trace);
    assert_contains(
        &findings,
        &[
            "runtime.event_order_invalid",
            "runtime.correlation_mismatch",
        ],
    );
    let order_findings = codes(&findings)
        .into_iter()
        .filter(|code| *code == "runtime.event_order_invalid")
        .count();
    assert!(
        order_findings >= 2,
        "sequence regression and cursor regression must both be reported: {findings:?}"
    );
}

#[test]
fn malformed_event_envelopes_are_rejected() {
    let mut mismatched = event(1, 1, "run.started", "run", RUN, json!({"run_id": RUN}));
    mismatched["value"]["kind"] = json!("run.prompt");
    mismatched["value"]["source_cursor"] = json!(99);

    let mut missing_identity = event(2, 2, "run.prompt", "run", RUN, json!({"run_id": RUN}));
    let fields = missing_identity["value"]
        .as_object_mut()
        .expect("event value object");
    fields.remove("request_id");
    fields.remove("sequence");

    let trace = input(
        vec![
            mismatched,
            missing_identity,
            raw_event(3, "run.prompt", json!("not-an-object")),
            raw_event(4, "run.prompt", json!({"run_id": RUN})),
        ],
        vec![],
    );

    let findings = evaluate(&trace);
    assert_contains(&findings, &["runtime.event_shape_invalid"]);
    let shape_findings = codes(&findings)
        .into_iter()
        .filter(|code| *code == "runtime.event_shape_invalid")
        .count();
    assert!(
        shape_findings >= 3,
        "kind, identity and non-object payload defects must be reported: {findings:?}"
    );
}

#[test]
fn invocation_identity_is_required_for_attempt_events() {
    let trace = input(
        vec![event(
            1,
            1,
            "run.tool_call",
            "run",
            RUN,
            json!({"run_id": RUN}),
        )],
        vec![],
    );

    let findings = evaluate(&trace);
    assert_contains(&findings, &["runtime.invocation_correlation_missing"]);
}

#[test]
fn invocation_alias_conflicts_are_rejected() {
    let trace = input(
        vec![
            event(
                1,
                1,
                "run.tool_call",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(
                2,
                2,
                "capability.completed",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(
                3,
                3,
                "run.tool_call",
                "run",
                RUN,
                json!({
                    "run_id": RUN,
                    "capability_request_id": SECOND_CAPABILITY_REQUEST,
                }),
            ),
            event(
                4,
                4,
                "invocation.dispatching",
                "execution_permit",
                EXECUTION,
                json!({
                    "run_id": RUN,
                    "invocation_id": INVOCATION,
                    "capability_request_id": SECOND_CAPABILITY_REQUEST,
                }),
            ),
        ],
        vec![1],
    );

    let findings = evaluate(&trace);
    assert_contains(
        &findings,
        &[
            "runtime.invocation_correlation_mismatch",
            "runtime.event_after_terminal",
        ],
    );
}

#[test]
fn retry_lifecycle_with_scheduled_attempts_has_no_findings() {
    let retry = |ordinal: u64, next_attempt: u64| {
        json!({
            "retry_class": "transport",
            "retry_ordinal": ordinal,
            "next_attempt": next_attempt,
            "delay_ms": 100,
            "reason_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        })
    };
    let model_data = |retry: Option<Value>| match retry {
        Some(retry) => json!({
            "run_id": RUN,
            "model_call_id": MODEL_CALL,
            "retry": retry,
        }),
        None => json!({
            "run_id": RUN,
            "model_call_id": MODEL_CALL,
        }),
    };
    let trace = input(
        vec![
            event(
                1,
                1,
                "model.prepared",
                "model_call",
                MODEL_CALL,
                model_data(None),
            ),
            event(
                2,
                2,
                "model.attempt_started",
                "model_call",
                MODEL_CALL,
                model_data(None),
            ),
            event(
                3,
                3,
                "model.retry_scheduled",
                "model_call",
                MODEL_CALL,
                model_data(Some(retry(1, 2))),
            ),
            event(
                4,
                4,
                "model.attempt_started",
                "model_call",
                MODEL_CALL,
                model_data(None),
            ),
            event(
                5,
                5,
                "model.retry_scheduled",
                "model_call",
                MODEL_CALL,
                model_data(Some(retry(2, 3))),
            ),
            event(
                6,
                6,
                "model.finished",
                "model_call",
                MODEL_CALL,
                model_data(None),
            ),
        ],
        vec![5],
    );

    let findings = evaluate(&trace);
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn multiple_model_calls_in_one_run_are_not_terminal_conflicts() {
    const SECOND_MODEL_CALL: &str = "00000000-0000-0000-0000-00000000000f";
    let model_data = |model_call: &str| json!({"run_id": RUN, "model_call_id": model_call});
    let trace = input(
        vec![
            event(
                1,
                1,
                "model.prepared",
                "model_call",
                MODEL_CALL,
                model_data(MODEL_CALL),
            ),
            event(
                2,
                2,
                "model.finished",
                "model_call",
                MODEL_CALL,
                model_data(MODEL_CALL),
            ),
            event(
                3,
                3,
                "model.prepared",
                "model_call",
                SECOND_MODEL_CALL,
                model_data(SECOND_MODEL_CALL),
            ),
            event(
                4,
                4,
                "model.finished",
                "model_call",
                SECOND_MODEL_CALL,
                model_data(SECOND_MODEL_CALL),
            ),
        ],
        vec![1, 3],
    );

    let findings = evaluate(&trace);
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn retry_metadata_and_order_are_checked() {
    let retry = |ordinal: u64, next_attempt: u64| {
        json!({
            "retry_class": "transport",
            "retry_ordinal": ordinal,
            "next_attempt": next_attempt,
            "delay_ms": 100,
            "reason_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        })
    };
    let trace = input(
        vec![
            event(
                1,
                1,
                "model.retry_scheduled",
                "model_call",
                MODEL_CALL,
                json!({"run_id": RUN, "model_call_id": MODEL_CALL, "retry": retry(0, 0)}),
            ),
            event(
                2,
                2,
                "model.retry_scheduled",
                "model_call",
                MODEL_CALL,
                json!({"run_id": RUN, "model_call_id": MODEL_CALL, "retry": retry(1, 2)}),
            ),
            event(
                3,
                3,
                "model.retry_scheduled",
                "model_call",
                MODEL_CALL,
                json!({"run_id": RUN, "model_call_id": MODEL_CALL, "retry": retry(1, 2)}),
            ),
            event(
                4,
                4,
                "model.retry_scheduled",
                "model_call",
                MODEL_CALL,
                json!({"run_id": RUN, "model_call_id": MODEL_CALL, "retry": retry(3, 3)}),
            ),
        ],
        vec![],
    );

    let findings = evaluate(&trace);
    assert_contains(
        &findings,
        &[
            "runtime.retry_metadata_invalid",
            "runtime.retry_order_invalid",
        ],
    );
}

#[test]
fn approval_lifecycle_order_is_enforced() {
    let trace = input(
        vec![
            event(
                1,
                1,
                "approval.approved",
                "approval",
                UNKNOWN_APPROVAL,
                json!({"approval_id": UNKNOWN_APPROVAL, "run_id": RUN}),
            ),
            event(
                2,
                2,
                "approval.requested",
                "approval",
                SECOND_APPROVAL,
                json!({
                    "approval_id": SECOND_APPROVAL,
                    "run_id": RUN,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
            event(
                3,
                3,
                "approval.consumed",
                "approval",
                SECOND_APPROVAL,
                json!({
                    "approval_id": SECOND_APPROVAL,
                    "run_id": RUN,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
            event(
                4,
                4,
                "approval.requested",
                "approval",
                SECOND_APPROVAL,
                json!({
                    "approval_id": SECOND_APPROVAL,
                    "run_id": RUN,
                    "capability_request_id": CAPABILITY_REQUEST,
                }),
            ),
            event(
                5,
                5,
                "run.tool_call",
                "run",
                RUN,
                json!({
                    "run_id": RUN,
                    "invocation_id": INVOCATION,
                    "execution_id": EXECUTION,
                    "capability_request_id": CAPABILITY_REQUEST,
                    "call_id": "call-1",
                    "approval_id": THIRD_APPROVAL,
                }),
            ),
        ],
        vec![2],
    );

    let findings = evaluate(&trace);
    assert_contains(
        &findings,
        &[
            "runtime.approval_unmatched",
            "runtime.approval_consumption_invalid",
            "runtime.approval_after_terminal",
            "runtime.approval_not_granted",
            "runtime.event_after_terminal",
        ],
    );
}

#[test]
fn cancel_requires_a_fence_and_a_terminal() {
    let unfenced = evaluate(&input(
        vec![event(
            1,
            1,
            "run.cancelled",
            "run",
            RUN,
            json!({"run_id": RUN}),
        )],
        vec![0],
    ));
    assert_contains(&unfenced, &["runtime.cancel_unfenced"]);

    let dangling = evaluate(&input(
        vec![event(
            1,
            1,
            "run.cancelling",
            "run",
            RUN,
            json!({"run_id": RUN}),
        )],
        vec![],
    ));
    assert_contains(&dangling, &["runtime.cancel_terminal_missing"]);

    let uncorrelated = evaluate(&input(
        vec![event(1, 1, "run.cancelling", "request", REQUEST, json!({}))],
        vec![],
    ));
    assert_contains(&uncorrelated, &["runtime.cancel_correlation_missing"]);

    let fenced = evaluate(&input(
        vec![
            event(1, 1, "run.started", "run", RUN, json!({"run_id": RUN})),
            event(2, 2, "run.cancelling", "run", RUN, json!({"run_id": RUN})),
            event(3, 3, "run.cancelled", "run", RUN, json!({"run_id": RUN})),
        ],
        vec![2],
    ));
    let fenced_codes = codes(&fenced);
    assert!(
        !fenced_codes
            .iter()
            .any(|code| code.starts_with("runtime.cancel")),
        "a fenced cancellation must be accepted: {fenced:?}"
    );
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
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(3, 3, "run.cancelled", "run", RUN, json!({"run_id": RUN})),
        ],
        vec![0, 2],
    );

    let findings = evaluate(&trace);
    assert_contains(
        &findings,
        &[
            "runtime.unknown_retry_forbidden",
            "runtime.cancel_unfenced",
            "runtime.terminal_duplicate",
        ],
    );
}

#[test]
fn unknown_must_be_reconciled_before_a_duplicate_or_retry() {
    let trace = input(
        vec![
            event(
                1,
                1,
                "run.tool_call",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(
                2,
                2,
                "capability.result_unknown",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(
                3,
                3,
                "invocation.dispatching",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
            event(
                4,
                4,
                "capability.result_unknown",
                "run",
                RUN,
                invocation_data(CAPABILITY_REQUEST, INVOCATION, EXECUTION, "call-1"),
            ),
        ],
        vec![1, 3],
    );

    let findings = evaluate(&trace);
    assert_contains(
        &findings,
        &[
            "runtime.unknown_retry_forbidden",
            "runtime.unknown_duplicate",
            "runtime.terminal_duplicate",
        ],
    );
}

#[test]
fn runtime_evaluator_output_is_deterministic_and_evidence_bound() {
    let trace = input(
        vec![
            event(1, 1, "run.completed", "run", RUN, json!({"run_id": RUN})),
            event(2, 2, "run.prompt", "run", RUN, json!({"run_id": RUN})),
        ],
        vec![0],
    );

    let first = evaluate(&trace);
    let second = evaluate(&trace);
    assert_eq!(first, second, "runtime findings must be deterministic");
    assert!(!first.is_empty());
    for finding in &first {
        assert!(
            finding.code.starts_with("runtime."),
            "unstable finding code: {}",
            finding.code
        );
        assert!(
            finding.evidence_ref == "trace:runtime" || finding.evidence_ref.starts_with("event:"),
            "unbounded evidence reference: {}",
            finding.evidence_ref
        );
    }

    let mut registry = EvaluatorRegistry::new();
    registry
        .register(RuntimeCorrectnessEvaluator)
        .expect("runtime evaluator registers");
    assert_eq!(
        registry.evaluator_ids(),
        vec![RUNTIME_CORRECTNESS_EVALUATOR_ID.to_owned()]
    );
    let result = registry
        .evaluate_with_digest(&trace)
        .expect("registry evaluation succeeds");
    assert_eq!(result.findings, first);
    assert_eq!(
        result.evaluator_ids,
        vec![RUNTIME_CORRECTNESS_EVALUATOR_ID.to_owned()]
    );
}

#[test]
fn malformed_runtime_input_is_rejected_before_evaluation() {
    let wrong_normalization = json!({
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
    assert_eq!(
        RuntimeCorrectnessEvaluator
            .evaluate(&wrong_normalization)
            .unwrap_err(),
        EvaluatorError::InputInvalid("runtime_correctness_input_normalization_invalid")
    );

    let wrong_schema = json!({
        "schema": "kiana.quality-runtime-correctness-input.v0",
        "trace": {
            "normalization_version": "eq18.canonical-events.v1",
            "array_policy": "ordered",
            "source_cursor_start": 1,
            "source_cursor_end": 1,
            "correlation_id": CORRELATION,
            "events": [],
            "terminal_event_indexes": [],
        }
    });
    assert_eq!(
        RuntimeCorrectnessEvaluator
            .evaluate(&wrong_schema)
            .unwrap_err(),
        EvaluatorError::InputInvalid("runtime_correctness_input_schema_invalid")
    );

    assert_eq!(
        RuntimeCorrectnessEvaluator
            .evaluate(&json!({"schema": RUNTIME_CORRECTNESS_INPUT_SCHEMA}))
            .unwrap_err(),
        EvaluatorError::InputInvalid("runtime_correctness")
    );
}

#[test]
fn empty_trace_is_a_finding_not_a_panic() {
    let findings = evaluate(&input(vec![], vec![]));
    assert_eq!(codes(&findings), vec!["runtime.trace_empty"]);
    assert_eq!(findings[0].evidence_ref, "trace:runtime");
}
