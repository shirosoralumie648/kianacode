use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
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
