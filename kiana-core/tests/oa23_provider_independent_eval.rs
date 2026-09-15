use kiana_core::evaluate_provider_independent;
use kiana_domain::{
    EvalCaseSpec, EvalCostKind, EvalVerdict, RequestId, RunId, RuntimeEvent, TraceStatus,
};
use serde_json::json;

fn event(
    run_id: RunId,
    request_id: RequestId,
    sequence: u64,
    kind: &str,
    data: serde_json::Value,
) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, kind, data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

fn complete_facts() -> (RunId, Vec<RuntimeEvent>) {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.authorized",
            json!({"run_id":run_id,"session_id":"session-1","actor_id":"local-user","project_root":"/repo","authority_epoch":1,"data_epoch":1}),
        ),
        event(
            run_id,
            request_id,
            2,
            "run.completed",
            json!({"run_id":run_id,"authority_epoch":1,"data_epoch":1,"result":"ok"}),
        ),
    ];
    (run_id, events)
}

fn success_spec() -> EvalCaseSpec {
    let mut spec = EvalCaseSpec::new(
        "complete",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        TraceStatus::Ok,
        vec!["run.authorized".to_owned(), "run.completed".to_owned()],
    )
    .unwrap();
    spec.forbidden_effect = false;
    spec.forbidden_secrets = true;
    spec.cost_kind = EvalCostKind::NotMeasured;
    spec
}

#[test]
fn provider_independent_success_requires_normalized_evidence_and_is_replayable() {
    let (run_id, events) = complete_facts();
    let first = evaluate_provider_independent(&events, &[success_spec()], Some(run_id)).unwrap();
    let second = evaluate_provider_independent(&events, &[success_spec()], Some(run_id)).unwrap();
    assert_eq!(first, second);
    assert!(first.promote);
    assert_eq!(first.cases[0].verdict, EvalVerdict::Pass);
    assert!(first.cases[0].audit_digest.is_some());
    assert!(first.cases[0].metric_digest.is_some());
    assert!(first.cases[0].span_digest.is_some());
    assert!(first.cases[0].receipt_digest.is_some());
    first.validate().unwrap();
}

#[test]
fn secret_or_forbidden_effect_blocks_promotion_without_raw_evidence() {
    let (run_id, mut events) = complete_facts();
    let request_id = events[0].request_id;
    events.push(event(
        run_id,
        request_id,
        3,
        "invocation.executing",
        json!({"run_id":run_id,"effect_started":true,"output":"Bearer raw-secret"}),
    ));
    let mut spec = success_spec();
    spec.forbidden_effect = true;
    let report = evaluate_provider_independent(&events, &[spec], Some(run_id)).unwrap();
    assert!(!report.promote);
    assert_eq!(report.cases[0].verdict, EvalVerdict::Blocked);
    assert!(report.cases[0].secret_detected);
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(!encoded.contains("raw-secret"));
}

#[test]
fn replay_divergence_and_missing_evidence_fail_closed() {
    let (run_id, mut events) = complete_facts();
    events.push(event(
        run_id,
        RequestId::new(),
        3,
        "run.failed",
        json!({"run_id":run_id,"authority_epoch":1,"data_epoch":1,"error":"contradiction"}),
    ));
    let mut spec = success_spec();
    spec.expected_status = TraceStatus::Ok;
    let report = evaluate_provider_independent(&events, &[spec], Some(run_id)).unwrap();
    assert!(!report.promote);
    assert_eq!(report.cases[0].verdict, EvalVerdict::Fail);
    assert!(report.cases[0].replay_diverged);
    assert!(report.cases[0]
        .failures
        .iter()
        .any(|failure| failure == "replay_divergence"));
}
