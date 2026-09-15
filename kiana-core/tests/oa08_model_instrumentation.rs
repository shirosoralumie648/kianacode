use kiana_core::project_model_attempts;
use kiana_domain::{ModelAttemptRecord, RequestId, RunId, RuntimeEvent, TraceStatus, TurnId};
use serde_json::{json, Value};

fn event(run_id: RunId, request_id: RequestId, sequence: u64, data: Value) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, "run.model_turn", data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

fn route() -> Value {
    json!({
        "provider_id": "anthropic",
        "protocol": "anthropic_messages",
        "connection_id": "local-profile",
        "model_id": "claude-test",
        "profile": "builder",
        "configuration_revision": "config.v1",
        "streaming": true,
    })
}

fn successful_event(run_id: RunId, request_id: RequestId, turn_id: TurnId) -> RuntimeEvent {
    let model_call_id = RequestId::new();
    let model_request_id = RequestId::new();
    event(
        run_id,
        request_id,
        1,
        json!({
            "run_id": run_id,
            "turn_id": turn_id,
            "model_call_id": model_call_id,
            "model_request_id": model_request_id,
            "attempt": 1,
            "provider_id": "anthropic",
            "model_id": "claude-test",
            "prepared": {
                "route": route(),
                "request_hash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
            "route_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "prompt_version": "fnv1a64:0123456789abcdef",
            "purpose": "task",
            "streaming": true,
            "attempted": true,
            "finish": "end_turn",
            "usage": {"input_tokens": 12, "output_tokens": 4},
            "usage_complete": true,
            "elapsed_ms": 42,
            "retry_class": "never",
            "cache_usage": "hit",
            "headers": {"authorization": "Bearer sk-secret"},
            "prompt": "do-not-persist-prompt-text",
            "raw_response": "do-not-persist-raw-response",
            "assistant": {"text": "provider-secret-response"},
            "error": null,
        }),
    )
}

#[test]
fn model_attempt_projection_is_deterministic_and_secret_free() {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let request_id = RequestId::new();
    let successful = successful_event(run_id, request_id, turn_id);
    let first = project_model_attempts(run_id, &[successful.clone()]).unwrap();
    let second = project_model_attempts(run_id, &[successful]).unwrap();
    assert_eq!(first, second);
    let record = first.first().expect("one model attempt");
    assert_eq!(record.status, TraceStatus::Ok);
    assert_eq!(record.provider_id, "anthropic");
    assert_eq!(record.model_id, "claude-test");
    assert_eq!(
        record.usage.as_ref().map(|usage| usage.input_tokens),
        Some(12)
    );
    assert_eq!(record.usage_complete, true);
    assert_eq!(record.cache_usage, Some(kiana_domain::ModelCacheUsage::Hit));
    let serialized = serde_json::to_string(record).unwrap();
    for sentinel in [
        "sk-secret",
        "do-not-persist-prompt-text",
        "do-not-persist-raw-response",
        "provider-secret-response",
        "authorization",
    ] {
        assert!(
            !serialized.contains(sentinel),
            "telemetry leaked {sentinel}: {serialized}"
        );
    }
    record.validate().unwrap();
}

fn failed_event(
    run_id: RunId,
    request_id: RequestId,
    sequence: u64,
    finish: Option<&str>,
    usage: Option<Value>,
    error: Option<Value>,
    retry_class: Option<&str>,
) -> RuntimeEvent {
    let mut data = json!({
        "run_id": run_id,
        "model_call_id": RequestId::new(),
        "model_request_id": RequestId::new(),
        "attempt": 1,
        "provider_id": "provider",
        "model_id": "model",
        "purpose": "task",
        "streaming": true,
        "attempted": true,
        "usage_complete": usage.is_some(),
    });
    if let Some(finish) = finish {
        data["finish"] = json!(finish);
    }
    if let Some(usage) = usage {
        data["usage"] = usage;
    }
    if let Some(error) = error {
        data["error"] = error;
    }
    if let Some(retry_class) = retry_class {
        data["retry_class"] = json!(retry_class);
    }
    event(run_id, request_id, sequence, data)
}

#[test]
fn malformed_truncated_timeout_and_retry_never_become_ok() {
    let run_id = RunId::new();
    let events = vec![
        failed_event(run_id, RequestId::new(), 1, None, None, None, None),
        failed_event(
            run_id,
            RequestId::new(),
            2,
            Some("length"),
            Some(json!({"input_tokens": 1, "output_tokens": 1})),
            None,
            Some("never"),
        ),
        failed_event(
            run_id,
            RequestId::new(),
            3,
            None,
            None,
            Some(json!({"code": "model_attempt_deadline", "retry_class": "never"})),
            Some("never"),
        ),
        failed_event(
            run_id,
            RequestId::new(),
            4,
            None,
            None,
            Some(json!({"code": "provider_http_429", "retry_class": "rejected"})),
            Some("rejected"),
        ),
        failed_event(
            run_id,
            RequestId::new(),
            5,
            Some("end_turn"),
            Some(json!({"input_tokens": 2, "output_tokens": 2})),
            None,
            Some("rejected"),
        ),
    ];
    let records = project_model_attempts(run_id, &events).unwrap();
    assert_eq!(records.len(), events.len());
    assert!(records
        .iter()
        .all(|record| record.status != TraceStatus::Ok));
    assert!(records
        .iter()
        .any(|record| record.status == TraceStatus::Unknown));
    assert!(records.iter().all(|record| {
        record.error_code.as_deref().is_none_or(|code| {
            code.chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        })
    }));
}

#[test]
fn model_attempt_contract_rejects_ok_without_complete_usage() {
    let run_id = RunId::new();
    let error = ModelAttemptRecord::new(
        kiana_domain::TraceId::new(),
        kiana_domain::SpanId::new(),
        run_id,
        None,
        RequestId::new(),
        RequestId::new(),
        1,
        "provider",
        "model",
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        None,
        kiana_domain::ModelPurpose::Task,
        Some(true),
        true,
        TraceStatus::Ok,
        Some(kiana_domain::ModelFinish::EndTurn),
        None,
        false,
        Some(1),
        Some(kiana_domain::ModelRetryClass::Never),
        None,
        1,
        vec![kiana_domain::EventId::new()],
        None,
        std::collections::BTreeMap::new(),
    );
    assert!(error.is_err());
}
