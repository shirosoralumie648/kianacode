use kiana_domain::*;
use serde_json::json;

fn digest(letter: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (letter as u32) & 0xffff).repeat(16)
    )
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
        PROVIDER_ROUND_PLAN_SCHEMA,
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

// ---------------------------------------------------------------------------------------------
// Complete-round recovery: TTL/revocation, committed-fact folding, crash points and the round
// plan. Rejections come first; success paths only reuse the already valid material.
// ---------------------------------------------------------------------------------------------

fn binding_for_attempt(
    run_id: RunId,
    turn_id: TurnId,
    route: &ModelRoute,
    call_id: RequestId,
    model_attempt_id: ModelAttemptId,
    replay: Option<ProtectedReplayRef>,
) -> ProviderResumeBinding {
    ProviderResumeBinding::new(
        run_id,
        turn_id,
        StepId::new(),
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

fn continuation(route: &ModelRoute, route_digest: String) -> ProviderContinuation {
    ProviderContinuation {
        schema: PROVIDER_CONTINUATION_SCHEMA.to_owned(),
        provider_id: route.provider_id.clone(),
        protocol: route.protocol,
        route_digest,
        item_ref: "remote-item".to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn observation(
    run_id: RunId,
    turn_id: TurnId,
    step_id: StepId,
    call_id: RequestId,
    attempt_id: ModelAttemptId,
    invocation_id: Option<InvocationId>,
    phase: ProviderInFlightPhase,
    usage: ProviderUsageDisposition,
    source_cursor: u64,
) -> ProviderInFlightObservation {
    ProviderInFlightObservation::new(
        run_id,
        turn_id,
        step_id,
        call_id,
        attempt_id,
        invocation_id,
        phase,
        digest('q'),
        digest('r'),
        usage,
        source_cursor,
    )
    .expect("in flight observation")
}

/// Owned live scope so the borrowed [`ProviderResumeCheck`] can be rebuilt for every call.
struct RoundScope {
    prompt: String,
    schema: String,
    tools: String,
    revision: String,
    material: Option<ProtectedReplayMaterial>,
}

impl Default for RoundScope {
    fn default() -> Self {
        Self {
            prompt: digest('a'),
            schema: digest('c'),
            tools: digest('b'),
            revision: "data-1".to_owned(),
            material: None,
        }
    }
}

impl RoundScope {
    fn with_material(material: ProtectedReplayMaterial) -> Self {
        Self {
            material: Some(material),
            ..Self::default()
        }
    }

    fn check<'a>(&'a self, route: &'a ModelRoute) -> ProviderResumeCheck<'a> {
        ProviderResumeCheck {
            route,
            prompt_digest: &self.prompt,
            schema_digest: &self.schema,
            tool_catalog_digest: &self.tools,
            data_revision: &self.revision,
            authority_epoch: 7,
            data_epoch: 9,
            now_unix_ms: 1_000,
            local_material: self.material.as_ref(),
        }
    }
}

fn tool_call(id: &str) -> ModelToolCall {
    ModelToolCall {
        id: id.to_owned(),
        name: "shell".to_owned(),
        arguments: json!({"command":"true"}),
    }
}

fn committed_fact(
    run_id: RunId,
    turn_id: TurnId,
    cursor: u64,
    attempt: Option<ModelAttemptId>,
    invocation: Option<InvocationId>,
    message: ModelMessage,
) -> ProviderHistoryFact {
    ProviderHistoryFact::new(
        run_id,
        turn_id,
        cursor,
        EventId::new(),
        attempt,
        invocation,
        ModelFactPersistenceStatus::Committed,
        message,
    )
    .expect("committed fact")
}

/// One complete round: prompt → assistant tool call → committed tool result.
fn complete_round_facts() -> (
    RunId,
    TurnId,
    ModelAttemptId,
    InvocationId,
    Vec<ProviderHistoryFact>,
) {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let attempt_id = ModelAttemptId::new();
    let invocation_id = InvocationId::new();
    let facts = vec![
        committed_fact(
            run_id,
            turn_id,
            1,
            None,
            None,
            ModelMessage::user("inspect the workspace"),
        ),
        committed_fact(
            run_id,
            turn_id,
            2,
            Some(attempt_id),
            None,
            ModelMessage::assistant_with_tools("I will inspect it", vec![tool_call("call-round")]),
        ),
        committed_fact(
            run_id,
            turn_id,
            3,
            None,
            Some(invocation_id),
            ModelMessage::tool("call-round", r#"{"success":true}"#),
        ),
    ];
    (run_id, turn_id, attempt_id, invocation_id, facts)
}

#[test]
fn expired_resume_binding_and_widened_ttl_are_rejected_before_any_send() {
    let route = route();
    let call_id = RequestId::new();
    // Distinct local names keep the `binding()` helper visible: `let x = binding(...)` would
    // otherwise resolve the call to the local variable after the first shadowing let.
    let expired = binding(&route, call_id, None);
    assert_eq!(
        expired
            .validate_for_resume(
                &route,
                &digest('a'),
                &digest('c'),
                &digest('b'),
                "data-1",
                7,
                9,
                20_000,
                None,
            )
            .unwrap_err(),
        "provider_resume_expired"
    );

    // A persisted binding whose digest was recomputed still cannot widen the protected material's
    // own TTL: the reference and the bytes are compared again at resume time.
    let material = protected_material(&route, call_id);
    let mut widened = material.reference("artifact-fixture");
    widened.expires_at_unix_ms = 30_000;
    let mut widened_binding = binding(
        &route,
        call_id,
        Some(material.reference("artifact-fixture")),
    );
    widened_binding.replay = Some(widened);
    widened_binding.binding_digest = widened_binding.digest();
    assert_eq!(
        widened_binding
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
            .unwrap_err(),
        "protected_replay_ttl_widened"
    );

    // Material that expired before the binding did is refused on its own deadline too.
    let mut narrowed = material.reference("artifact-fixture");
    narrowed.expires_at_unix_ms = 1_500;
    let mut narrowed_binding = binding(
        &route,
        call_id,
        Some(material.reference("artifact-fixture")),
    );
    narrowed_binding.replay = Some(narrowed);
    narrowed_binding.binding_digest = narrowed_binding.digest();
    assert_eq!(
        narrowed_binding
            .validate_for_resume(
                &route,
                &digest('a'),
                &digest('c'),
                &digest('b'),
                "data-1",
                7,
                9,
                1_600,
                Some(&material),
            )
            .unwrap_err(),
        "protected_replay_expired"
    );
}

#[test]
fn tampered_binding_digest_cannot_be_trusted_even_with_matching_fields() {
    let route = route();
    let call_id = RequestId::new();
    let mut binding = binding(&route, call_id, None);
    binding.prompt_digest = digest('z');
    assert_eq!(
        binding.validate().unwrap_err(),
        "provider_resume_binding_digest_mismatch"
    );
    assert_eq!(
        binding
            .validate_for_resume(
                &route,
                &digest('z'),
                &digest('c'),
                &digest('b'),
                "data-1",
                7,
                9,
                1_000,
                None,
            )
            .unwrap_err(),
        "provider_resume_binding_invalid"
    );
}

#[test]
fn revoked_deleted_or_route_switched_replay_material_blocks_resume() {
    let route = route();
    let call_id = RequestId::new();
    let mut store = ProtectedReplayStore::default();
    let reference = store
        .insert("artifact-fixture", protected_material(&route, call_id))
        .expect("store insert");
    assert!(store.resolve_material(&reference, &route, 1_000).is_ok());
    store
        .revoke("artifact-fixture", "data_governance")
        .expect("revoke");
    assert_eq!(
        store
            .resolve_material(&reference, &route, 1_000)
            .unwrap_err()
            .code,
        "replay_material_revoked"
    );
    assert_eq!(
        store.revocation_reason("artifact-fixture"),
        Some("data_governance")
    );

    // Deleting an artifact is indistinguishable from "never captured": both fail closed.
    let mut store = ProtectedReplayStore::default();
    let reference = store
        .insert("artifact-fixture", protected_material(&route, call_id))
        .expect("store insert");
    assert!(store.delete("artifact-fixture"));
    assert_eq!(
        store
            .resolve_material(&reference, &route, 1_000)
            .unwrap_err()
            .code,
        "missing_reasoning_material_blocks_resume"
    );

    // A model/connection switch revokes every material bound to the previous route.
    let mut store = ProtectedReplayStore::default();
    let reference = store
        .insert("artifact-fixture", protected_material(&route, call_id))
        .expect("store insert");
    let mut switched = route.clone();
    switched.model_id = "model-other".to_owned();
    assert_eq!(store.invalidate_route_change(&switched), 1);
    assert_eq!(
        store.revocation_reason("artifact-fixture"),
        Some("model_switch")
    );
    assert_eq!(
        store
            .resolve_material(&reference, &route, 1_000)
            .unwrap_err()
            .code,
        "replay_material_revoked"
    );
}

#[test]
fn history_rejects_partial_duplicate_and_out_of_scope_facts() {
    let (run_id, turn_id, _, _, attempt_id) = ids();
    let invocation_id = InvocationId::new();
    let prompt = || {
        committed_fact(
            run_id,
            turn_id,
            1,
            None,
            None,
            ModelMessage::user("inspect the workspace"),
        )
    };
    let assistant = || {
        committed_fact(
            run_id,
            turn_id,
            2,
            Some(attempt_id),
            None,
            ModelMessage::assistant_with_tools("I will inspect it", vec![tool_call("call-1")]),
        )
    };
    let tool = || {
        committed_fact(
            run_id,
            turn_id,
            3,
            None,
            Some(invocation_id),
            ModelMessage::tool("call-1", r#"{"success":true}"#),
        )
    };

    // A partial round is not a completed assistant turn and cannot be folded.
    assert_eq!(
        ProviderHistoryProjection::rebuild(vec![prompt(), assistant()]).unwrap_err(),
        "model_history_incomplete_batch"
    );
    // An orphan tool result (no committed assistant tool call) is refused.
    assert_eq!(
        ProviderHistoryProjection::rebuild(vec![prompt(), tool()]).unwrap_err(),
        "model_history_orphan_tool_result"
    );
    // Duplicate source cursors and duplicate source events are not silently merged.
    let mut duplicate_cursor = assistant();
    duplicate_cursor.source_cursor = 1;
    duplicate_cursor.source_event_id = EventId::new();
    duplicate_cursor.fact_digest = duplicate_cursor.digest();
    assert_eq!(
        ProviderHistoryProjection::rebuild(vec![prompt(), duplicate_cursor]).unwrap_err(),
        "provider_history_source_cursor_conflict"
    );
    let shared_event = EventId::new();
    let mut first = prompt();
    first.source_event_id = shared_event;
    first.fact_digest = first.digest();
    let mut second = assistant();
    second.source_event_id = shared_event;
    second.fact_digest = second.digest();
    assert_eq!(
        ProviderHistoryProjection::rebuild(vec![first, second]).unwrap_err(),
        "provider_history_source_event_duplicate"
    );
    // Foreign runs/turns and failed commits never enter a resumable history.
    let mut foreign = assistant();
    foreign.run_id = RunId::new();
    foreign.fact_digest = foreign.digest();
    assert_eq!(
        ProviderHistoryProjection::rebuild(vec![prompt(), foreign]).unwrap_err(),
        "provider_history_run_mismatch"
    );
    let mut other_turn = assistant();
    other_turn.turn_id = TurnId::new();
    other_turn.fact_digest = other_turn.digest();
    assert_eq!(
        ProviderHistoryProjection::rebuild(vec![prompt(), other_turn]).unwrap_err(),
        "provider_history_turn_mismatch"
    );
    let failed = ProviderHistoryFact::new(
        run_id,
        turn_id,
        1,
        EventId::new(),
        Some(attempt_id),
        None,
        ModelFactPersistenceStatus::Failed,
        ModelMessage::assistant("partial response"),
    )
    .expect("failed fact stays inspectable");
    assert_eq!(
        ProviderHistoryProjection::rebuild(vec![failed]).unwrap_err(),
        "provider_history_uncommitted_fact_not_resumable"
    );
    // Partial output cannot be typed as an assistant turn at all.
    assert_eq!(
        ProviderHistoryFact::new(
            run_id,
            turn_id,
            1,
            EventId::new(),
            None,
            None,
            ModelFactPersistenceStatus::Pending,
            ModelMessage::assistant("partial response"),
        )
        .unwrap_err(),
        "provider_history_assistant_attempt_required"
    );
    assert_eq!(
        ProviderHistoryFact::new(
            run_id,
            turn_id,
            1,
            EventId::new(),
            None,
            None,
            ModelFactPersistenceStatus::Committed,
            ModelMessage::tool("call-1", r#"{"success":true}"#),
        )
        .unwrap_err(),
        "provider_history_tool_invocation_required"
    );
}

#[test]
fn every_crash_point_classifies_without_automatic_retry() {
    let (run_id, turn_id, step_id, call_id, attempt_id) = ids();
    let invocation_id = InvocationId::new();
    for (phase, action, usage, expected_kind) in [
        (
            ProviderInFlightPhase::PreparedBeforeSend,
            ProviderRecoveryAction::ReauthorizeAndResume,
            ProviderUsageDisposition::NotObserved,
            ProviderReconciliationKind::ModelAttempt,
        ),
        (
            ProviderInFlightPhase::SentAwaitingResponse,
            ProviderRecoveryAction::ReconcileModelUnknown,
            ProviderUsageDisposition::Unknown,
            ProviderReconciliationKind::ModelAttempt,
        ),
        (
            ProviderInFlightPhase::ModelCompletedBeforeCommit,
            ProviderRecoveryAction::ReconcileModelUnknown,
            ProviderUsageDisposition::Unknown,
            ProviderReconciliationKind::ModelAttempt,
        ),
        (
            ProviderInFlightPhase::CapabilityCompletedBeforeDelivery,
            ProviderRecoveryAction::ReconcileCapabilityUnknown,
            ProviderUsageDisposition::Unknown,
            ProviderReconciliationKind::CapabilityInvocation,
        ),
        (
            ProviderInFlightPhase::TerminalCommitted,
            ProviderRecoveryAction::NoAction,
            ProviderUsageDisposition::Final,
            ProviderReconciliationKind::ModelAttempt,
        ),
    ] {
        let invocation = (phase == ProviderInFlightPhase::CapabilityCompletedBeforeDelivery)
            .then_some(invocation_id);
        let observation = observation(
            run_id, turn_id, step_id, call_id, attempt_id, invocation, phase, usage, 6,
        );
        let case = ProviderReconciliationCase::from_observation(&observation).expect("case");
        assert_eq!(recovery_action_for_phase(phase), action);
        assert_eq!(case.action, action);
        assert_eq!(case.kind, expected_kind);
        assert_eq!(case.usage, usage);
        assert!(!case.automatic_retry_allowed);
        assert_eq!(case.reconciliation_required, phase.is_unknown());
        assert!(!case.capability_resubmission_allowed());
        assert_eq!(
            case.provider_resubmission_allowed(),
            phase == ProviderInFlightPhase::PreparedBeforeSend
        );
    }

    // An invocation belongs to the capability crash point only.
    assert_eq!(
        ProviderInFlightObservation::new(
            run_id,
            turn_id,
            step_id,
            call_id,
            attempt_id,
            Some(invocation_id),
            ProviderInFlightPhase::SentAwaitingResponse,
            digest('q'),
            digest('r'),
            ProviderUsageDisposition::NotObserved,
            7,
        )
        .unwrap_err(),
        "provider_in_flight_invocation_unexpected"
    );
    assert_eq!(
        ProviderInFlightObservation::new(
            run_id,
            turn_id,
            step_id,
            call_id,
            attempt_id,
            None,
            ProviderInFlightPhase::CapabilityCompletedBeforeDelivery,
            digest('q'),
            digest('r'),
            ProviderUsageDisposition::NotObserved,
            7,
        )
        .unwrap_err(),
        "provider_in_flight_capability_invocation_required"
    );
    // An unknown outcome can never claim a final usage.
    assert_eq!(
        ProviderInFlightObservation::new(
            run_id,
            turn_id,
            step_id,
            call_id,
            attempt_id,
            None,
            ProviderInFlightPhase::SentAwaitingResponse,
            digest('q'),
            digest('r'),
            ProviderUsageDisposition::Final,
            7,
        )
        .unwrap_err(),
        "provider_in_flight_unknown_usage_cannot_be_final"
    );
}

#[test]
fn round_plan_rejects_missing_foreign_duplicate_and_consumed_rounds() {
    let (run_id, turn_id, _, _, facts) = complete_round_facts();
    assert_eq!(
        ProviderRoundPlan::plan(Vec::new(), &[]).unwrap_err(),
        PROVIDER_RECOVERY_ROUND_INPUT_MISSING
    );
    let foreign = observation(
        RunId::new(),
        turn_id,
        StepId::new(),
        RequestId::new(),
        ModelAttemptId::new(),
        None,
        ProviderInFlightPhase::PreparedBeforeSend,
        ProviderUsageDisposition::NotObserved,
        4,
    );
    assert_eq!(
        ProviderRoundPlan::plan(facts.clone(), &[foreign]).unwrap_err(),
        PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH
    );
    let attempt_id = ModelAttemptId::new();
    let call_id = RequestId::new();
    let conflict = [
        observation(
            run_id,
            turn_id,
            StepId::new(),
            call_id,
            attempt_id,
            None,
            ProviderInFlightPhase::SentAwaitingResponse,
            ProviderUsageDisposition::Unknown,
            4,
        ),
        observation(
            run_id,
            turn_id,
            StepId::new(),
            call_id,
            attempt_id,
            None,
            ProviderInFlightPhase::ModelCompletedBeforeCommit,
            ProviderUsageDisposition::Unknown,
            5,
        ),
    ];
    assert_eq!(
        ProviderRoundPlan::plan(facts.clone(), &conflict).unwrap_err(),
        PROVIDER_RECOVERY_ROUND_OBSERVATION_CONFLICT
    );
    let shared_cursor = [
        observation(
            run_id,
            turn_id,
            StepId::new(),
            call_id,
            attempt_id,
            None,
            ProviderInFlightPhase::SentAwaitingResponse,
            ProviderUsageDisposition::Unknown,
            4,
        ),
        observation(
            run_id,
            turn_id,
            StepId::new(),
            call_id,
            ModelAttemptId::new(),
            None,
            ProviderInFlightPhase::ModelCompletedBeforeCommit,
            ProviderUsageDisposition::Unknown,
            4,
        ),
    ];
    assert_eq!(
        ProviderRoundPlan::plan(facts.clone(), &shared_cursor).unwrap_err(),
        PROVIDER_RECOVERY_ROUND_OBSERVATION_CONFLICT
    );

    // A terminal attempt stays consumed: its stable round plan never re-opens the same attempt.
    let terminal = observation(
        run_id,
        turn_id,
        StepId::new(),
        call_id,
        attempt_id,
        None,
        ProviderInFlightPhase::TerminalCommitted,
        ProviderUsageDisposition::Final,
        4,
    );
    let plan = ProviderRoundPlan::plan(facts, &[terminal]).expect("terminal round");
    assert!(!plan.requires_reconciliation());
    assert_eq!(
        plan.model_visible_history()
            .expect("complete history")
            .messages
            .len(),
        3
    );
    let route = route();
    let binding = binding_for_attempt(run_id, turn_id, &route, call_id, attempt_id, None);
    let scope = RoundScope::default();
    assert_eq!(
        plan.authorize_resume(&binding, &scope.check(&route))
            .unwrap_err(),
        PROVIDER_RECOVERY_ROUND_ATTEMPT_CONSUMED
    );
}

#[test]
fn round_plan_blocks_resume_while_an_inflight_attempt_is_unknown() {
    let (run_id, turn_id, _, _, facts) = complete_round_facts();
    let call_id = RequestId::new();
    let unknown_attempt = ModelAttemptId::new();
    let sent = observation(
        run_id,
        turn_id,
        StepId::new(),
        call_id,
        unknown_attempt,
        None,
        ProviderInFlightPhase::SentAwaitingResponse,
        ProviderUsageDisposition::Unknown,
        4,
    );
    let plan = ProviderRoundPlan::plan(facts, &[sent]).expect("reconciling round");
    assert!(plan.requires_reconciliation());
    assert_eq!(plan.model_unknown_attempts(), vec![unknown_attempt]);
    assert!(plan.capability_unknown_invocations().is_empty());
    assert!(plan.prepared_attempts().is_empty());

    let route = route();
    let material = protected_material(&route, call_id);
    let binding = binding_for_attempt(
        run_id,
        turn_id,
        &route,
        call_id,
        unknown_attempt,
        Some(material.reference("artifact-fixture")),
    );
    let scope = RoundScope::with_material(material);
    assert_eq!(
        plan.authorize_resume(&binding, &scope.check(&route))
            .unwrap_err(),
        PROVIDER_RECOVERY_ROUND_RESUME_BLOCKED
    );
    // An unknown attempt also blocks an unrelated prepared attempt: the whole round must be
    // reconciled before a new provider request is authorized.
    let other = binding_for_attempt(
        run_id,
        turn_id,
        &route,
        RequestId::new(),
        ModelAttemptId::new(),
        None,
    );
    assert_eq!(
        plan.authorize_resume(&other, &scope.check(&route))
            .unwrap_err(),
        PROVIDER_RECOVERY_ROUND_RESUME_BLOCKED
    );
}

#[test]
fn capability_completed_before_delivery_never_fabricates_history_or_reexecutes() {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let attempt_id = ModelAttemptId::new();
    let invocation_id = InvocationId::new();
    let call_id = RequestId::new();
    let prompt = || {
        committed_fact(
            run_id,
            turn_id,
            1,
            None,
            None,
            ModelMessage::user("inspect the workspace"),
        )
    };
    let assistant = || {
        committed_fact(
            run_id,
            turn_id,
            2,
            Some(attempt_id),
            None,
            ModelMessage::assistant_with_tools(
                "I will inspect it",
                vec![tool_call("call-pending")],
            ),
        )
    };
    let observation = observation(
        run_id,
        turn_id,
        StepId::new(),
        call_id,
        attempt_id,
        Some(invocation_id),
        ProviderInFlightPhase::CapabilityCompletedBeforeDelivery,
        ProviderUsageDisposition::NotObserved,
        3,
    );
    let plan =
        ProviderRoundPlan::plan(vec![prompt(), assistant()], &[observation]).expect("open round");
    assert!(plan.model_visible_history().is_none());
    assert_eq!(
        plan.history_incomplete_reason.as_deref(),
        Some(PROVIDER_RECOVERY_ROUND_HISTORY_INCOMPLETE)
    );
    assert!(plan.completed_invocations().is_empty());
    assert!(plan.requires_reconciliation());
    assert_eq!(plan.capability_unknown_invocations(), vec![invocation_id]);
    assert!(plan.model_unknown_attempts().is_empty());

    let route = route();
    let binding = binding_for_attempt(
        run_id,
        turn_id,
        &route,
        call_id,
        ModelAttemptId::new(),
        None,
    );
    let scope = RoundScope::default();
    assert_eq!(
        plan.authorize_resume(&binding, &scope.check(&route))
            .unwrap_err(),
        PROVIDER_RECOVERY_ROUND_RESUME_BLOCKED
    );

    // Once the tool result is committed the same round folds into a complete history, and the
    // finished invocation belongs to the never-re-execute set.
    let tool = committed_fact(
        run_id,
        turn_id,
        3,
        None,
        Some(invocation_id),
        ModelMessage::tool("call-pending", r#"{"success":true}"#),
    );
    let settled =
        ProviderRoundPlan::plan(vec![prompt(), assistant(), tool], &[]).expect("delivered round");
    let history = settled.model_visible_history().expect("complete history");
    assert!(history.invocation_completed(invocation_id));
    assert!(!history.may_reexecute_completed_invocation());
    assert_eq!(settled.completed_invocations(), &[invocation_id]);
    assert!(!settled.requires_reconciliation());
    assert_eq!(
        settled
            .authorize_resume(
                &binding_for_attempt(
                    RunId::new(),
                    turn_id,
                    &route,
                    call_id,
                    ModelAttemptId::new(),
                    None,
                ),
                &scope.check(&route),
            )
            .unwrap_err(),
        PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH
    );
}

#[test]
fn round_plan_authorizes_a_prepared_attempt_after_explicit_scope_recheck() {
    let (run_id, turn_id, _, _, facts) = complete_round_facts();
    let route = route();
    let call_id = RequestId::new();
    let attempt_id = ModelAttemptId::new();
    let prepared = observation(
        run_id,
        turn_id,
        StepId::new(),
        call_id,
        attempt_id,
        None,
        ProviderInFlightPhase::PreparedBeforeSend,
        ProviderUsageDisposition::NotObserved,
        4,
    );
    let plan = ProviderRoundPlan::plan(facts, &[prepared]).expect("prepared round");
    assert!(!plan.requires_reconciliation());
    assert_eq!(plan.prepared_attempts(), vec![attempt_id]);
    assert!(plan.model_unknown_attempts().is_empty());

    let material = protected_material(&route, call_id);
    let binding = binding_for_attempt(
        run_id,
        turn_id,
        &route,
        call_id,
        attempt_id,
        Some(material.reference("artifact-fixture")),
    );
    let scope = RoundScope::with_material(material);
    assert_eq!(
        plan.authorize_resume(&binding, &scope.check(&route))
            .expect("re-authorize the prepared attempt"),
        ProviderResumeSource::LocalReplay
    );

    // A valid remote continuation is the cheaper source, but the route must match exactly.
    let binding = binding
        .with_continuation(continuation(&route, route.digest()))
        .expect("valid continuation");
    assert_eq!(
        plan.authorize_resume(&binding, &scope.check(&route))
            .expect("valid remote continuation"),
        ProviderResumeSource::RemoteContinuation
    );

    // A stale/invalid remote continuation falls back to complete local material only; it is never
    // accepted as an ID and never widens the scope.
    let binding = binding
        .with_continuation(continuation(&route, digest('d')))
        .expect("mismatched continuation stays inspectable");
    assert_eq!(
        plan.authorize_resume(&binding, &scope.check(&route))
            .expect("local fallback"),
        ProviderResumeSource::LocalReplay
    );

    // Incomplete live scope is rejected before the binding is consulted.
    let invalid = ProviderResumeCheck {
        schema_digest: "",
        ..scope.check(&route)
    };
    assert_eq!(
        plan.authorize_resume(&binding, &invalid).unwrap_err(),
        PROVIDER_RESUME_CHECK_INVALID
    );

    // The plan survives a JSON round trip without losing the decision.
    let encoded = serde_json::to_string(&plan).expect("encode plan");
    let decoded: ProviderRoundPlan = serde_json::from_str(&encoded).expect("decode plan");
    assert_eq!(decoded, plan);
    decoded.validate().expect("decoded plan validates");
}
