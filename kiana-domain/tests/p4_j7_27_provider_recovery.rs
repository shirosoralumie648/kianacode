use kiana_domain::*;
use serde_json::json;

fn digest(letter: char) -> String {
    format!("sha256:{}", letter.to_string().repeat(64))
}

fn route() -> ModelRoute {
    ModelRoute {
        provider_id: "provider-fixture".to_owned(),
        protocol: ModelProtocol::OpenAiResponses,
        connection_id: "connection-fixture".to_owned(),
        model_id: "model-fixture".to_owned(),
        profile: "profile-fixture".to_owned(),
        configuration_revision: "config-1".to_owned(),
        streaming: true,
    }
}

fn ids() -> (RunId, TurnId, StepId, RequestId, ModelAttemptId) {
    (
        RunId::new(),
        TurnId::new(),
        StepId::new(),
        RequestId::new(),
        ModelAttemptId::new(),
    )
}

fn protected_material(route: &ModelRoute, call_id: RequestId) -> ProtectedReplayMaterial {
    ProtectedReplayMaterial::new(
        ProtectedReplayScope {
            connection_id: route.connection_id.clone(),
            protocol: route.protocol,
            model_id: route.model_id.clone(),
            route_digest: route.digest(),
            effort: Some("medium".to_owned()),
            prompt_digest: digest('a'),
            tool_catalog_digest: digest('b'),
            data_revision: "data-1".to_owned(),
            source_call_id: call_id,
            expires_at_unix_ms: 20_000,
        },
        b"private replay bytes",
    )
    .expect("protected replay material")
}

fn binding(
    route: &ModelRoute,
    call_id: RequestId,
    replay: Option<ProtectedReplayRef>,
) -> ProviderResumeBinding {
    let (_, turn_id, step_id, _, model_attempt_id) = ids();
    ProviderResumeBinding::new(
        RunId::new(),
        turn_id,
        step_id,
        call_id,
        model_attempt_id,
        route.clone(),
        digest('a'),
        digest('c'),
        digest('b'),
        "data-1",
        7,
        9,
        20_000,
        replay,
    )
    .expect("resume binding")
}

#[test]
fn restart_with_missing_replay_material_fails_closed() {
    let route = route();
    let call_id = RequestId::new();
    let binding = binding(&route, call_id, None);
    let error = binding
        .validate_for_resume(
            &route,
            &digest('a'),
            &digest('c'),
            &digest('b'),
            "data-1",
            7,
            9,
            1_000,
            None,
        )
        .expect_err("missing replay material must block resume");
    assert_eq!(error, PROVIDER_RECOVERY_REPLAY_MATERIAL_MISSING);
}

#[test]
fn stale_route_authority_cannot_resume() {
    let route = route();
    let call_id = RequestId::new();
    let binding = binding(&route, call_id, None);
    let error = binding
        .validate_for_resume(
            &route,
            &digest('a'),
            &digest('c'),
            &digest('b'),
            "data-1",
            8,
            9,
            1_000,
            None,
        )
        .expect_err("authority drift must block resume");
    assert_eq!(error, PROVIDER_RECOVERY_STALE_ROUTE);
}

#[test]
fn invalid_remote_continuation_uses_complete_local_material_only() {
    let route = route();
    let call_id = RequestId::new();
    let material = protected_material(&route, call_id);
    let replay = material.reference("artifact-fixture");
    let continuation = ProviderContinuation {
        schema: PROVIDER_CONTINUATION_SCHEMA.to_owned(),
        provider_id: route.provider_id.clone(),
        protocol: route.protocol,
        route_digest: digest('d'),
        item_ref: "remote-item".to_owned(),
    };
    let binding = binding(&route, call_id, Some(replay));
    let binding = binding
        .with_continuation(continuation)
        .expect("invalid remote continuation is still an inspectable binding");
    let source = binding
        .validate_for_resume(
            &route,
            &digest('a'),
            &digest('c'),
            &digest('b'),
            "data-1",
            7,
            9,
            1_000,
            Some(&material),
        )
        .expect("complete local material is an allowed fallback");
    assert_eq!(source, ProviderResumeSource::LocalReplay);
}

#[test]
fn fresh_process_resumes_completed_model_turn_with_tool_history() {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let attempt_id = ModelAttemptId::new();
    let invocation_id = InvocationId::new();
    let call = ModelToolCall {
        id: "call-fixture".to_owned(),
        name: "shell".to_owned(),
        arguments: json!({"command":"true"}),
    };
    let facts = vec![
        ProviderHistoryFact::new(
            run_id,
            turn_id,
            1,
            EventId::new(),
            None,
            None,
            ModelFactPersistenceStatus::Committed,
            ModelMessage::user("inspect the workspace"),
        )
        .expect("prompt fact"),
        ProviderHistoryFact::new(
            run_id,
            turn_id,
            2,
            EventId::new(),
            Some(attempt_id),
            None,
            ModelFactPersistenceStatus::Committed,
            ModelMessage::assistant_with_tools("I will inspect it", vec![call]),
        )
        .expect("model fact"),
        ProviderHistoryFact::new(
            run_id,
            turn_id,
            3,
            EventId::new(),
            None,
            Some(invocation_id),
            ModelFactPersistenceStatus::Committed,
            ModelMessage::tool("call-fixture", r#"{"success":true}"#),
        )
        .expect("tool fact"),
    ];
    let history = ProviderHistoryProjection::rebuild(facts).expect("recovered history");
    assert_eq!(history.messages.len(), 3);
    assert!(history.invocation_completed(invocation_id));
    assert!(!history.may_reexecute_completed_invocation());
}

#[test]
fn uncommitted_model_fact_is_not_promoted_to_history() {
    let (run_id, turn_id, _, _, attempt_id) = ids();
    let fact = ProviderHistoryFact::new(
        run_id,
        turn_id,
        1,
        EventId::new(),
        Some(attempt_id),
        None,
        ModelFactPersistenceStatus::Pending,
        ModelMessage::assistant("partial response"),
    )
    .expect("pending fact remains inspectable");
    assert_eq!(
        ProviderHistoryProjection::rebuild(vec![fact]).unwrap_err(),
        "provider_history_uncommitted_fact_not_resumable"
    );
}

#[test]
fn inflight_model_attempt_is_not_resubmitted_on_restart() {
    let (run_id, turn_id, step_id, call_id, attempt_id) = ids();
    let observation = ProviderInFlightObservation::new(
        run_id,
        turn_id,
        step_id,
        call_id,
        attempt_id,
        None,
        ProviderInFlightPhase::SentAwaitingResponse,
        digest('q'),
        digest('r'),
        ProviderUsageDisposition::Unknown,
        12,
    )
    .expect("in flight model observation");
    let case = ProviderReconciliationCase::from_observation(&observation)
        .expect("model unknown reconciliation");
    assert_eq!(case.action, ProviderRecoveryAction::ReconcileModelUnknown);
    assert!(case.reconciliation_required);
    assert!(!case.provider_resubmission_allowed());
    assert_eq!(
        PROVIDER_RECOVERY_NO_MODEL_RESUBMIT,
        "inflight_model_attempt_is_not_resubmitted_on_restart"
    );
}

#[test]
fn capability_unknown_is_reconciled_without_reexecution() {
    let (run_id, turn_id, step_id, call_id, attempt_id) = ids();
    let observation = ProviderInFlightObservation::new(
        run_id,
        turn_id,
        step_id,
        call_id,
        attempt_id,
        Some(InvocationId::new()),
        ProviderInFlightPhase::CapabilityCompletedBeforeDelivery,
        digest('q'),
        digest('r'),
        ProviderUsageDisposition::NotObserved,
        13,
    )
    .expect("capability in flight observation");
    let case = ProviderReconciliationCase::from_observation(&observation)
        .expect("capability unknown reconciliation");
    assert_eq!(case.kind, ProviderReconciliationKind::CapabilityInvocation);
    assert_eq!(
        case.action,
        ProviderRecoveryAction::ReconcileCapabilityUnknown
    );
    assert!(!case.capability_resubmission_allowed());
    assert!(!case.automatic_retry_allowed);
}

#[test]
fn provider_recovery_schema_registry_is_strict() {
    for schema in [
        PROVIDER_RECOVERY_SCHEMA,
        PROVIDER_RESUME_BINDING_SCHEMA,
        PROVIDER_HISTORY_FACT_SCHEMA,
        PROVIDER_HISTORY_PROJECTION_SCHEMA,
        PROVIDER_IN_FLIGHT_SCHEMA,
        PROVIDER_RECONCILIATION_SCHEMA,
    ] {
        assert!(
            schema_contract(schema).is_some(),
            "missing schema: {schema}"
        );
    }
}

#[test]
fn pre_send_resume_requires_explicit_re_admission() {
    let (run_id, turn_id, step_id, call_id, attempt_id) = ids();
    let observation = ProviderInFlightObservation::new(
        run_id,
        turn_id,
        step_id,
        call_id,
        attempt_id,
        None,
        ProviderInFlightPhase::PreparedBeforeSend,
        digest('q'),
        digest('r'),
        ProviderUsageDisposition::NotObserved,
        14,
    )
    .expect("prepared observation");
    let case = ProviderReconciliationCase::from_observation(&observation).expect("case");
    assert_eq!(case.action, ProviderRecoveryAction::ReauthorizeAndResume);
    assert!(case.provider_resubmission_allowed());
    assert!(!case.reconciliation_required);
}

#[test]
fn model_completion_before_fact_is_unknown_and_not_resubmitted() {
    let (run_id, turn_id, step_id, call_id, attempt_id) = ids();
    let observation = ProviderInFlightObservation::new(
        run_id,
        turn_id,
        step_id,
        call_id,
        attempt_id,
        None,
        ProviderInFlightPhase::ModelCompletedBeforeCommit,
        digest('q'),
        digest('r'),
        ProviderUsageDisposition::NotObserved,
        15,
    )
    .expect("uncommitted completion observation");
    let case = ProviderReconciliationCase::from_observation(&observation).expect("case");
    assert_eq!(case.usage, ProviderUsageDisposition::Unknown);
    assert!(!case.provider_resubmission_allowed());
    assert_eq!(
        recovery_action_for_phase(ProviderInFlightPhase::ModelCompletedBeforeCommit),
        ProviderRecoveryAction::ReconcileModelUnknown
    );
}

#[test]
fn resume_never_reexecutes_completed_invocation() {
    let (run_id, turn_id, _, _, attempt_id) = ids();
    let invocation_id = InvocationId::new();
    let facts = vec![
        ProviderHistoryFact::new(
            run_id,
            turn_id,
            1,
            EventId::new(),
            Some(attempt_id),
            None,
            ModelFactPersistenceStatus::Committed,
            ModelMessage::assistant_with_tools(
                "run it",
                vec![ModelToolCall {
                    id: "call-once".to_owned(),
                    name: "shell".to_owned(),
                    arguments: json!({"command":"true"}),
                }],
            ),
        )
        .expect("assistant fact"),
        ProviderHistoryFact::new(
            run_id,
            turn_id,
            2,
            EventId::new(),
            None,
            Some(invocation_id),
            ModelFactPersistenceStatus::Committed,
            ModelMessage::tool("call-once", r#"{"success":true}"#),
        )
        .expect("tool fact"),
    ];
    let history = ProviderHistoryProjection::rebuild(facts).expect("history");
    assert!(history.invocation_completed(invocation_id));
    assert!(!history.may_reexecute_completed_invocation());
}
