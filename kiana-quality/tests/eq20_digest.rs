use kiana_domain::RequestId;
use kiana_quality::{
    artifact_digest, event_digest, receipt_digest, trace_digest, ArrayPolicy, DigestError,
    DigestKind, VolatileEvent, VolatileEventTrace, CANONICAL_NORMALIZATION_VERSION,
    VOLATILE_NORMALIZATION_VERSION,
};
use serde_json::json;

fn trace() -> VolatileEventTrace {
    VolatileEventTrace {
        normalization_version: VOLATILE_NORMALIZATION_VERSION.to_owned(),
        source_normalization_version: CANONICAL_NORMALIZATION_VERSION.to_owned(),
        array_policy: ArrayPolicy::Ordered,
        source_cursor_start: 3,
        source_cursor_end: 4,
        correlation_id: RequestId::new(),
        events: vec![
            VolatileEvent {
                source_cursor: 3,
                kind: "run.started".to_owned(),
                value: json!({
                    "kind": "run.started",
                    "event_id": "<UUID>",
                    "source_cursor": 3,
                    "data": {"run_id": "<UUID>"}
                }),
            },
            VolatileEvent {
                source_cursor: 4,
                kind: "run.completed".to_owned(),
                value: json!({
                    "kind": "run.completed",
                    "event_id": "<UUID>",
                    "source_cursor": 4,
                    "data": {"run_id": "<UUID>"}
                }),
            },
        ],
        terminal_event_indexes: vec![1],
        replacement_count: 0,
        replacements: Vec::new(),
    }
}

#[test]
fn event_and_trace_digest_are_stable_and_version_bound() {
    let left = VolatileEvent {
        source_cursor: 1,
        kind: "run.started".to_owned(),
        value: json!({"b": 2, "a": 1}),
    };
    let right = VolatileEvent {
        source_cursor: 1,
        kind: "run.started".to_owned(),
        value: json!({"a": 1, "b": 2}),
    };
    let first = event_digest(&left, VOLATILE_NORMALIZATION_VERSION).unwrap();
    let second = event_digest(&right, VOLATILE_NORMALIZATION_VERSION).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.kind, DigestKind::Event);
    assert_ne!(
        first.digest,
        event_digest(&left, "eq19.other.v1").unwrap().digest
    );

    let first_trace = trace_digest(&trace()).unwrap();
    let mut changed = trace();
    changed.replacement_count = 1;
    let changed_trace = trace_digest(&changed).unwrap();
    assert_eq!(first_trace.kind, DigestKind::Trace);
    assert_ne!(first_trace.digest, changed_trace.digest);
    first_trace.validate().unwrap();
}

#[test]
fn artifact_and_receipt_digest_share_the_versioned_evidence_contract() {
    let artifact = artifact_digest(
        &json!({"content": "redacted", "path": "<TEMP_PATH>"}),
        VOLATILE_NORMALIZATION_VERSION,
    )
    .unwrap();
    let artifact_reordered = artifact_digest(
        &json!({"path": "<TEMP_PATH>", "content": "redacted"}),
        VOLATILE_NORMALIZATION_VERSION,
    )
    .unwrap();
    let receipt = receipt_digest(
        &json!({"status": "completed", "effect_known": false}),
        VOLATILE_NORMALIZATION_VERSION,
    )
    .unwrap();
    assert_eq!(artifact.digest, artifact_reordered.digest);
    assert_eq!(artifact.kind, DigestKind::Artifact);
    assert_eq!(receipt.kind, DigestKind::Receipt);
    assert_ne!(artifact.digest, receipt.digest);
}

#[test]
fn empty_or_wrong_trace_version_cannot_produce_a_digest() {
    let mut empty = trace();
    empty.events.clear();
    assert_eq!(trace_digest(&empty).unwrap_err(), DigestError::TraceEmpty);
    let mut wrong = trace();
    wrong.normalization_version = "eq18.canonical-events.v1".to_owned();
    assert_eq!(
        trace_digest(&wrong).unwrap_err(),
        DigestError::TraceVersionInvalid
    );
}
