use kiana_domain::*;
use serde_json::json;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn correlation(with_invocation: bool) -> (CorrelationContext, RunId, TurnId, StepId) {
    let request = RequestContext::local("model-events-fixture", "/repo");
    let scope = CorrelationScope::new(request.session_id.clone(), None, None);
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let step_id = StepId::new();
    let context = CorrelationContext::from_request(&request, scope, 1, 1, None)
        .expect("root correlation")
        .with_run(run_id)
        .expect("run correlation")
        .with_turn(turn_id)
        .expect("turn correlation");
    let context = if with_invocation {
        context
            .with_invocation(InvocationId::new(), ExecutionId::new())
            .expect("invocation correlation")
    } else {
        context
    };
    (context, run_id, turn_id, step_id)
}

fn event(kind: ModelEventKind) -> ModelEvent {
    let (context, _, _, step_id) = correlation(false);
    let trace = ProviderTraceMetadata::empty(&RedactionProfile::for_signal(RedactionSignal::Trace))
        .expect("trace metadata");
    ModelEvent::new(
        kind,
        context,
        step_id,
        RequestId::new(),
        ModelAttemptId::new(),
        1,
        "provider-fixture",
        "model-fixture",
        "config-rev-1",
        digest('p'),
        digest('s'),
        digest('r'),
        digest('c'),
        digest('t'),
        trace,
    )
    .expect("model event")
}

#[test]
fn model_event_round_trip_and_runtime_boundary_are_bounded() {
    let profile = RedactionProfile::for_signal(RedactionSignal::Trace);
    let trace = ProviderTraceMetadata::from_texts(
        &profile,
        Some("Authorization: Bearer fixture-secret"),
        Some("token=fixture-secret"),
        Some("provider returned password=fixture-secret"),
        Some("opaque response body"),
        Some("delta"),
        Some("replay reference"),
    )
    .expect("redacted provider trace");
    let (context, _, _, step_id) = correlation(false);
    let model_call_id = RequestId::new();
    let model_attempt_id = ModelAttemptId::new();
    let model = ModelEvent::new(
        ModelEventKind::Prepared,
        context,
        step_id,
        model_call_id,
        model_attempt_id,
        1,
        "provider-fixture",
        "actual-model-fixture",
        "config-rev-1",
        digest('p'),
        digest('s'),
        digest('r'),
        digest('c'),
        digest('t'),
        trace,
    )
    .expect("prepared model event")
    .with_provider_references(
        ProviderReference::known("provider-request-1").expect("request ref"),
        ProviderReference::unknown(),
    )
    .expect("provider refs");
    let mut model = model;
    model.kind = ModelEventKind::Finished;
    let model = model
        .with_finish(
            ModelFinish::EndTurn,
            Some(ModelUsage {
                input_tokens: 7,
                output_tokens: 4,
            }),
        )
        .expect("finish");
    model.validate().expect("validated model event");
    let encoded = serde_json::to_string(&model).expect("model event json");
    assert!(!encoded.contains("fixture-secret"));
    let runtime = model.into_runtime_event(1).expect("runtime event");
    validate_runtime_event(&runtime).expect("runtime event contract");
    assert_eq!(runtime.kind, "model.finished");
    assert_eq!(runtime.aggregate_type.as_deref(), Some("model_call"));
    let aggregate_id = model_call_id.to_string();
    assert_eq!(runtime.aggregate_id.as_deref(), Some(aggregate_id.as_str()));
    assert_eq!(runtime.payload_recoverable, Some(false));
}

#[test]
fn split_provider_secret_is_redacted_and_delta_ledger_is_bounded() {
    let profile = RedactionProfile::for_signal(RedactionSignal::Trace);
    let mut ledger = ProviderDeltaLedger::new(profile, 2).expect("ledger");
    ledger
        .append_text("prefix Authorization: Bearer fixture-")
        .expect("first delta");
    ledger.append_text("secret suffix").expect("second delta");
    ledger.finish().expect("flush delta");
    ledger.validate().expect("valid ledger");
    assert!(ledger.retained_count() <= 2);
    let encoded = serde_json::to_string(&ledger).expect("ledger json");
    assert!(!encoded.contains("fixture-secret"));
}

#[test]
fn persistence_guard_blocks_effects_until_commit() {
    let model = event(ModelEventKind::Prepared);
    let mut commitment = ModelFactCommitment::pending(&model).expect("pending commitment");
    assert_eq!(
        commitment.require_tool_dispatch().unwrap_err(),
        MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_EFFECT
    );
    assert_eq!(
        commitment.require_mark_completed().unwrap_err(),
        MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_COMPLETION
    );
    commitment.mark_appended(9).expect("append commitment");
    commitment
        .require_tool_dispatch()
        .expect("dispatch allowed");
    commitment
        .require_mark_completed()
        .expect("completion allowed");
}

#[test]
fn model_attempt_invocation_receipt_link_is_digest_bound() {
    let (context, run_id, turn_id, step_id) = correlation(true);
    let invocation_id = context.invocation_id.expect("invocation");
    let execution_id = context.execution_id.expect("execution");
    let link = ModelAttemptInvocationReceiptLink::new(
        run_id,
        turn_id,
        step_id,
        RequestId::new(),
        ModelAttemptId::new(),
        invocation_id,
        execution_id,
        "call-1",
        "shell",
        digest('e'),
        digest('m'),
    )
    .expect("receipt link");
    link.validate().expect("valid receipt link");
    let mut tampered = link;
    tampered.tool_name = "memory.search".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "model_invocation_link_digest_mismatch"
    );
}

#[test]
fn registry_covers_model_lifecycle_and_correction_is_not_terminal() {
    for kind in [
        "model.prepared",
        "model.denied",
        "model.attempt_started",
        "model.retry_scheduled",
        "model.finished",
        "model.usage_correction",
    ] {
        assert!(
            event_kind_spec(kind).is_some(),
            "missing model kind: {kind}"
        );
    }
    assert!(!ModelEventKind::UsageCorrection.is_terminal());
    assert!(ModelEventKind::Finished.is_terminal());
}

#[test]
fn usage_correction_digest_binds_the_late_usage_payload() {
    let reason_digest = digest('r');
    let mut correction = ModelUsageCorrection::new(
        Some(digest('o')),
        Some(ModelUsage {
            input_tokens: 12,
            output_tokens: 7,
        }),
        reason_digest,
        42,
    )
    .expect("usage correction");
    correction.validate().expect("bound correction");

    correction.usage = Some(ModelUsage {
        input_tokens: 13,
        output_tokens: 7,
    });
    assert_eq!(
        correction.validate().unwrap_err(),
        "model_usage_correction_digest_mismatch"
    );
}

/// Build one model fact for every registered lifecycle kind. The `Prepared` shape is the neutral
/// baseline; kind-specific fields are attached afterwards because a bare `Finished`/`RetryScheduled`
/// fact is intentionally invalid until its terminal/retry fields exist.
fn model_event(kind: ModelEventKind) -> ModelEvent {
    let (context, _, _, step_id) = correlation(false);
    let trace = ProviderTraceMetadata::empty(&RedactionProfile::for_signal(RedactionSignal::Trace))
        .expect("trace metadata");
    let mut event = ModelEvent::new(
        ModelEventKind::Prepared,
        context,
        step_id,
        RequestId::new(),
        ModelAttemptId::new(),
        2,
        "provider-fixture",
        "model-fixture",
        "config-rev-1",
        digest('p'),
        digest('s'),
        digest('r'),
        digest('c'),
        digest('t'),
        trace,
    )
    .expect("model event");
    event.kind = kind;
    event
}

#[test]
fn every_registered_model_kind_round_trips_through_the_event_registry() {
    let retry = ModelRetryMetadata {
        retry_class: ModelRetryClass::Rejected,
        retry_ordinal: 1,
        next_attempt: 2,
        delay_ms: 750,
        reason_digest: digest('y'),
    };
    let correction = ModelUsageCorrection::new(
        None,
        Some(ModelUsage {
            input_tokens: 4,
            output_tokens: 2,
        }),
        digest('u'),
        7,
    )
    .expect("usage correction");
    let mut retry_sample = model_event(ModelEventKind::RetryScheduled);
    retry_sample.retry = Some(retry);
    let mut finished_sample = model_event(ModelEventKind::Finished);
    finished_sample.finish = Some(ModelFinish::EndTurn);
    finished_sample.usage = Some(ModelUsage {
        input_tokens: 6,
        output_tokens: 3,
    });
    let mut correction_sample = model_event(ModelEventKind::UsageCorrection);
    correction_sample.usage_correction = Some(correction);
    // `with_sequences` reseals and validates each mutated sample without changing its fields.
    let samples = [
        model_event(ModelEventKind::Prepared)
            .with_sequences(ModelEventSequences::default())
            .expect("prepared fact"),
        model_event(ModelEventKind::Denied)
            .with_sequences(ModelEventSequences::default())
            .expect("denied fact"),
        model_event(ModelEventKind::AttemptStarted)
            .with_sequences(ModelEventSequences::default())
            .expect("attempt fact"),
        retry_sample
            .with_sequences(ModelEventSequences::default())
            .expect("retry fact"),
        finished_sample
            .with_sequences(ModelEventSequences::default())
            .expect("finished fact"),
        correction_sample
            .with_sequences(ModelEventSequences::default())
            .expect("usage correction fact"),
    ];
    for (index, sample) in samples.iter().enumerate() {
        sample.validate().expect("valid model fact");
        let payload = sample.payload().expect("encoded payload");
        let runtime = sample
            .into_runtime_event(index as u64 + 1)
            .expect("runtime event");
        validate_runtime_event(&runtime).expect("registered runtime event");
        assert_eq!(runtime.kind, sample.kind.as_str());
        let decoded: ModelEvent = serde_json::from_value(payload).expect("decoded fact");
        decoded.validate().expect("decoded fact is still valid");
        assert_eq!(decoded.kind, sample.kind);
    }
}

#[test]
fn provider_secrets_split_across_deltas_are_redacted() {
    let profile = RedactionProfile::for_signal(RedactionSignal::Trace);
    let chunks = [
        "turn prefix Authorization: Bear",
        "er sk-live-fixture-token-",
        "material suffix; api_key=fixture-api-key",
        "&password=fixture-password and x-api-key:",
        " fixture-x-api-key done",
    ];
    // Incremental display path: the streaming redactor must close markers split across chunks.
    let mut redactor = StreamingRedactor::new();
    let mut visible = String::new();
    for chunk in chunks {
        visible.push_str(&redactor.push(chunk));
    }
    visible.push_str(&redactor.finish());
    for leaked in [
        "sk-live-fixture-token-material",
        "fixture-api-key",
        "fixture-password",
        "fixture-x-api-key",
    ] {
        assert!(!visible.contains(leaked), "streamed delta leaked: {leaked}");
    }
    assert!(visible.contains("[REDACTED]"));

    // Durable path: the ledger keeps bounded references, never the raw chunks or values.
    let mut ledger = ProviderDeltaLedger::new(profile.clone(), 3).expect("bounded delta ledger");
    let initial_digest = ledger.aggregate_digest.clone();
    for chunk in chunks {
        ledger.append_text(chunk).expect("delta segment");
    }
    ledger.finish().expect("flush trailing marker");
    ledger.validate().expect("validated ledger");
    assert_eq!(ledger.observed_delta_count, chunks.len() as u64);
    assert!(ledger.retained_count() <= 3);
    assert!(ledger.dropped_segment_count <= ledger.observed_delta_count);
    let encoded = serde_json::to_string(&ledger).expect("ledger json");
    for leaked in [
        "sk-live-fixture-token-material",
        "fixture-api-key",
        "fixture-password",
        "fixture-x-api-key",
        "Authorization",
    ] {
        assert!(!encoded.contains(leaked), "delta ledger leaked: {leaked}");
    }
    for raw_secret in [
        "sk-live-fixture-token-material",
        "fixture-api-key",
        "fixture-password",
        "fixture-x-api-key",
    ] {
        assert!(
            ledger
                .retained_segments
                .iter()
                .all(|segment| segment.digest != json_digest(&json!(raw_secret))),
            "retained segment digest covers the raw secret: {raw_secret}"
        );
    }
    assert_ne!(ledger.aggregate_digest, initial_digest);
    assert!(ledger.aggregate_digest.starts_with("sha256:"));
}

#[test]
fn provider_error_body_cannot_leak_authorization() {
    let profile = RedactionProfile::for_signal(RedactionSignal::Trace);
    let raw_error = r#"{"error":{"type":"authentication_error","message":"Authorization: Bearer sk-live-error-fixture; token=fixture-error-token"}}"#;
    let trace = ProviderTraceMetadata::from_texts(
        &profile,
        Some("x-request-id: req-1; authorization: bearer sk-live-header-fixture"),
        Some("api_key=fixture-query-key&stream=true"),
        Some(raw_error),
        Some(r#"{"id":"resp-1","output":"ok"}"#),
        Some("delta without secrets"),
        Some("replay reference only"),
    )
    .expect("redacted provider trace");
    trace.validate().expect("validated provider trace");
    let error = trace.error.as_ref().expect("error reference");
    assert!(error.redacted);
    assert_ne!(error.digest, json_digest(&json!(raw_error)));
    assert_eq!(error.digest, json_digest(&json!(redact_text(raw_error))));

    let (context, _, _, step_id) = correlation(false);
    let event = ModelEvent::new(
        ModelEventKind::Denied,
        context,
        step_id,
        RequestId::new(),
        ModelAttemptId::new(),
        1,
        "provider-fixture",
        "model-fixture",
        "config-rev-1",
        digest('p'),
        digest('s'),
        digest('r'),
        digest('c'),
        digest('t'),
        trace,
    )
    .expect("denied model event");
    let payload = serde_json::to_string(&event.payload().expect("payload")).expect("payload json");
    let runtime = event.into_runtime_event(1).expect("runtime event");
    validate_runtime_event(&runtime).expect("registered runtime event");
    let envelope = serde_json::to_string(&runtime).expect("envelope json");
    for leaked in [
        "sk-live-error-fixture",
        "fixture-error-token",
        "sk-live-header-fixture",
        "fixture-query-key",
        "Bearer sk-",
    ] {
        assert!(!payload.contains(leaked), "payload leaked: {leaked}");
        assert!(!envelope.contains(leaked), "runtime event leaked: {leaked}");
    }
    assert_eq!(runtime.payload_recoverable, Some(false));
    assert_eq!(
        runtime.redaction_profile.as_deref(),
        Some(profile.profile_digest.as_str())
    );
}

#[test]
fn model_fact_persistence_failure_prevents_tool_dispatch() {
    let (context, _, _, step_id) = correlation(true);
    let mut event = ModelEvent::new(
        ModelEventKind::Prepared,
        context,
        step_id,
        RequestId::new(),
        ModelAttemptId::new(),
        1,
        "provider-fixture",
        "model-fixture",
        "config-rev-1",
        digest('p'),
        digest('s'),
        digest('r'),
        digest('c'),
        digest('t'),
        ProviderTraceMetadata::empty(&RedactionProfile::for_signal(RedactionSignal::Trace))
            .expect("trace metadata"),
    )
    .expect("prepared model event");
    event.kind = ModelEventKind::Finished;
    let event = event
        .with_finish(
            ModelFinish::ToolUse,
            Some(ModelUsage {
                input_tokens: 3,
                output_tokens: 2,
            }),
        )
        .expect("finish")
        .with_tool("call-1", "shell")
        .expect("tool identity");
    let mut commitment = ModelFactCommitment::pending(&event).expect("pending commitment");
    commitment
        .mark_append_failed("event_store_append_unavailable")
        .expect("record append failure");
    assert_eq!(commitment.status, ModelFactPersistenceStatus::Failed);
    assert!(commitment.durable_sequence.is_none());
    assert!(!commitment.can_dispatch_tool());
    assert!(!commitment.can_mark_completed());
    assert_eq!(
        commitment.require_tool_dispatch().unwrap_err(),
        MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_EFFECT
    );
    assert_eq!(
        commitment.require_mark_completed().unwrap_err(),
        MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_COMPLETION
    );
    assert_eq!(
        commitment.mark_appended(9).unwrap_err(),
        "model_fact_append_transition_invalid"
    );
    commitment.validate().expect("validated failure commitment");
    let failure_digest = commitment
        .failure_digest
        .clone()
        .expect("failure digest recorded");
    assert_eq!(
        failure_digest,
        json_digest(&json!("event_store_append_unavailable"))
    );

    let mut replayed = ModelFactCommitment::pending(&event).expect("pending commitment");
    replayed
        .mark_append_failed("event_store_append_unavailable")
        .expect("record append failure");
    assert_eq!(replayed.failure_digest, Some(failure_digest));
    assert!(!replayed.can_dispatch_tool());
}

#[test]
fn model_attempt_to_invocation_receipt_chain_is_complete() {
    let (context, run_id, turn_id, step_id) = correlation(true);
    let invocation_id = context.invocation_id.expect("invocation correlation");
    let execution_id = context.execution_id.expect("execution correlation");
    let command_id = context.command_id;
    let correlation_id = context.correlation_id;
    let model_call_id = RequestId::new();
    let model_attempt_id = ModelAttemptId::new();
    let source_event_id = EventId::new();
    let mut event = ModelEvent::new(
        ModelEventKind::Prepared,
        context,
        step_id,
        model_call_id,
        model_attempt_id,
        2,
        "provider-fixture",
        "actual-model-fixture",
        "config-rev-7",
        digest('p'),
        digest('s'),
        digest('r'),
        digest('c'),
        digest('t'),
        ProviderTraceMetadata::empty(&RedactionProfile::for_signal(RedactionSignal::Trace))
            .expect("trace metadata"),
    )
    .expect("prepared model event");
    event.kind = ModelEventKind::Finished;
    let event = event
        .with_finish(
            ModelFinish::ToolUse,
            Some(ModelUsage {
                input_tokens: 11,
                output_tokens: 5,
            }),
        )
        .expect("finish")
        .with_tool("call-1", "shell")
        .expect("tool identity")
        .with_provider_references(
            ProviderReference::known("provider-request-1").expect("request reference"),
            ProviderReference::unknown(),
        )
        .expect("provider references")
        .with_sequences(ModelEventSequences {
            ui_sequence: Some(4),
            upstream_sequence: Some(9),
            durable_sequence: Some(12),
        })
        .expect("sequence channels")
        .with_source(source_event_id, 12)
        .expect("source binding");

    assert_eq!(event.run_id, run_id);
    assert_eq!(event.turn_id, turn_id);
    assert_eq!(event.step_id, step_id);
    assert_eq!(event.model_call_id, model_call_id);
    assert_eq!(event.model_attempt_id, model_attempt_id);
    assert_eq!(event.invocation_id, Some(invocation_id));
    assert_eq!(event.execution_id, Some(execution_id));
    assert_eq!(event.sequences.ui_sequence, Some(4));
    assert_eq!(event.sequences.upstream_sequence, Some(9));
    assert_eq!(event.sequences.durable_sequence, Some(12));
    assert_eq!(event.source_event_id, Some(source_event_id));
    assert_eq!(event.source_cursor, Some(12));

    let runtime = event.into_runtime_event(13).expect("runtime event");
    validate_runtime_event(&runtime).expect("registered runtime event");
    assert_eq!(runtime.sequence, 13);
    assert_eq!(runtime.kind, "model.finished");
    assert_eq!(runtime.request_id, model_call_id);
    assert_eq!(runtime.aggregate_type.as_deref(), Some("model_call"));
    assert_eq!(
        runtime.aggregate_id.as_deref(),
        Some(model_call_id.to_string().as_str())
    );
    assert_eq!(runtime.command_id, command_id);
    assert_eq!(runtime.correlation_id, Some(correlation_id));
    assert_eq!(runtime.causation_event_id, Some(source_event_id));

    let link = ModelAttemptInvocationReceiptLink::new(
        run_id,
        turn_id,
        step_id,
        model_call_id,
        model_attempt_id,
        invocation_id,
        execution_id,
        "call-1",
        "shell",
        digest('e'),
        event.event_digest.clone(),
    )
    .expect("receipt chain link");
    link.validate().expect("validated receipt chain link");
    assert_eq!(link.source_model_event_digest, event.event_digest);
    let encoded = serde_json::to_string(&link).expect("link json");
    for leaked in ["args", "output", "stdout", "secret", "prompt"] {
        assert!(
            !encoded.contains(leaked),
            "receipt chain link leaked: {leaked}"
        );
    }
}

#[test]
fn provider_trace_records_actual_model_and_config_version() {
    let profile = RedactionProfile::for_signal(RedactionSignal::Trace);
    let (context, _, _, step_id) = correlation(false);
    let first = ModelEvent::new(
        ModelEventKind::Prepared,
        context.clone(),
        step_id,
        RequestId::new(),
        ModelAttemptId::new(),
        1,
        "provider-fixture",
        "reported-model-a",
        "config-rev-1",
        digest('p'),
        digest('s'),
        digest('r'),
        digest('c'),
        digest('t'),
        ProviderTraceMetadata::empty(&profile).expect("trace metadata"),
    )
    .expect("first model fact");
    let second = ModelEvent::new(
        ModelEventKind::Prepared,
        context,
        step_id,
        RequestId::new(),
        ModelAttemptId::new(),
        1,
        "provider-fixture",
        "reported-model-b",
        "config-rev-2",
        digest('p'),
        digest('s'),
        digest('r'),
        digest('c'),
        digest('t'),
        ProviderTraceMetadata::empty(&profile).expect("trace metadata"),
    )
    .expect("second model fact");
    assert_eq!(first.actual_model_id, "reported-model-a");
    assert_eq!(first.configuration_revision, "config-rev-1");
    assert_eq!(second.actual_model_id, "reported-model-b");
    assert_eq!(second.configuration_revision, "config-rev-2");
    assert_ne!(first.event_digest, second.event_digest);

    let payload = first.payload().expect("payload");
    let object = payload.as_object().expect("payload object");
    for forbidden in [
        "prompt",
        "request",
        "response",
        "messages",
        "body",
        "authorization",
        "api_key",
    ] {
        assert!(
            !object.contains_key(forbidden),
            "model fact persists request material: {forbidden}"
        );
    }
    assert_eq!(payload["actual_model_id"], json!("reported-model-a"));
    assert_eq!(payload["configuration_revision"], json!("config-rev-1"));
    assert_eq!(payload["provider_request"]["status"], json!("unknown"));
    assert_eq!(payload["provider_response"]["status"], json!("unknown"));
    assert!(first.provider_request.is_unknown());
    assert!(first.provider_response.is_unknown());

    // A missing wire id is explicit unknown, never a locally fabricated id.
    ProviderReference::unknown()
        .validate("provider_request_id")
        .expect("unknown is explicit");
    let fabricated = ProviderReference {
        id: None,
        status: ProviderReferenceStatus::Known,
    };
    assert_eq!(
        fabricated.validate("provider_request_id").unwrap_err(),
        "provider_request_id_status_mismatch"
    );
    let smuggled_id = ProviderReference {
        id: Some("provider-request-1".to_owned()),
        status: ProviderReferenceStatus::Unknown,
    };
    assert_eq!(
        smuggled_id.validate("provider_response_id").unwrap_err(),
        "provider_response_id_status_mismatch"
    );
}

#[test]
fn provider_delta_ledger_granularity_remains_bounded() {
    let profile = RedactionProfile::for_signal(RedactionSignal::Trace);
    let mut ledger = ProviderDeltaLedger::new(profile, 4).expect("bounded delta ledger");
    for index in 0..512 {
        ledger
            .append_text(&format!("token-{index} "))
            .expect("delta segment");
    }
    ledger.finish().expect("flush delta stream");
    ledger.validate().expect("validated ledger");
    assert_eq!(ledger.observed_delta_count, 512);
    assert_eq!(ledger.retained_count(), 4);
    assert_eq!(ledger.dropped_segment_count, 508);
    let encoded = serde_json::to_string(&ledger).expect("ledger json");
    assert!(
        encoded.len() < 4096,
        "durable delta row is not bounded: {} bytes",
        encoded.len()
    );
    assert!(!encoded.contains("token-511"));
    assert!(ledger.aggregate_digest.starts_with("sha256:"));
}
