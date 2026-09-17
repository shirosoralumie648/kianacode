use kiana_core::aggregate_receipt_facts;
use kiana_domain::{AggregationVerification, RequestId, RunId, RuntimeEvent};
use serde_json::json;

fn event(run_id: RunId, sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

#[test]
fn aggregation_counts_only_committed_facts_and_keeps_refs_hashed() {
    let run_id = RunId::new();
    let events = vec![
        event(
            run_id,
            1,
            "run.model_turn",
            json!({"run_id":run_id,"attempted":true,"usage":{"input_tokens":3,"output_tokens":5}}),
        ),
        event(
            run_id,
            2,
            "capability.completed",
            json!({"run_id":run_id,"changed":[{"path":"./src/main.rs"}],"evidence_refs":["artifact:private"],"provider_receipt_ref":"provider-secret-id"}),
        ),
        event(
            run_id,
            3,
            "capability.completed",
            json!({"run_id":run_id,"schema":"kiana.memory-search.v1","hits":[{"id":"memory-1"}]}),
        ),
    ];
    let aggregation = aggregate_receipt_facts(run_id, &events).unwrap();
    assert_eq!(aggregation.model_turns, 1);
    assert_eq!(aggregation.input_tokens, Some(3));
    assert_eq!(aggregation.output_tokens, Some(5));
    assert_eq!(aggregation.files_changed, vec!["src/main.rs"]);
    assert_eq!(aggregation.memory_hits, 1);
    assert_eq!(aggregation.verification, AggregationVerification::Complete);
    assert_eq!(aggregation.cost_micros, None);
    let serialized = serde_json::to_string(&aggregation).unwrap();
    assert!(!serialized.contains("private"));
    assert!(!serialized.contains("provider-secret-id"));
}

#[test]
fn aggregation_marks_unknown_usage_and_effect_without_settling_estimated_cost() {
    let run_id = RunId::new();
    let events = vec![
        event(
            run_id,
            1,
            "run.model_turn",
            json!({"run_id":run_id,"attempted":true,"usage":{"input_tokens":1}}),
        ),
        event(
            run_id,
            2,
            "execution.result_committed",
            json!({"run_id":run_id,"effect_known":false,"result":{"success":true}}),
        ),
        event(
            run_id,
            3,
            "capability.completed",
            json!({"run_id":run_id,"committed":false,"changed":[{"path":"safe.txt"}]}),
        ),
    ];
    let aggregation = aggregate_receipt_facts(run_id, &events).unwrap();
    assert!(aggregation.usage_unknown);
    assert_eq!(aggregation.input_tokens, None);
    assert_eq!(aggregation.output_tokens, None);
    assert_eq!(aggregation.verification, AggregationVerification::Unknown);
    assert_eq!(aggregation.cost_micros, None);
    assert!(!aggregation.cost_estimated);
}

#[test]
fn aggregation_rejects_empty_or_foreign_source() {
    let run_id = RunId::new();
    assert_eq!(
        aggregate_receipt_facts(run_id, &[]).unwrap_err(),
        "receipt_aggregation_source_empty"
    );
    let other = RunId::new();
    let event = event(
        other,
        1,
        "run.model_turn",
        json!({"run_id":other,"attempted":true}),
    );
    assert_eq!(
        aggregate_receipt_facts(run_id, &[event]).unwrap_err(),
        "receipt_aggregation_source_empty"
    );
}

#[test]
fn er12_aggregation_is_event_and_artifact_bound() {
    let receipts = include_str!("../src/receipts.rs");
    let domain = include_str!("../../kiana-domain/src/receipt_aggregation.rs");
    for marker in [
        "aggregate_receipt_facts",
        "ReceiptAggregation",
        "model_turns",
        "committed_executions",
        "input_tokens",
        "usage_unknown",
        "files_changed",
        "memory_hits",
        "evidence_ref_digests",
        "provider_receipt_refs",
        "cost_estimated",
        "AggregationVerification::Unknown",
        "normalize_role_path",
        "payload_recoverable",
    ] {
        assert!(
            receipts.contains(marker) || domain.contains(marker),
            "ER-12 marker missing: {marker}"
        );
    }
    for forbidden in [
        "provider_estimate_settle",
        "uncommitted_output_is_cost",
        "raw_provider_response",
        "commit_transition",
        "CapabilityBrokerPort",
    ] {
        assert!(
            !domain.contains(forbidden),
            "aggregation must not {forbidden}"
        );
    }
}
