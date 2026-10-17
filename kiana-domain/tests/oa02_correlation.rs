use kiana_domain::{
    AttemptRef, CausationRef, CorrelationContext, CorrelationScope, CorrelationStage,
    CorrelationTrail, EventId, ExecutionId, InvocationId, OrganizationId, ProjectId,
    RequestContext, RequestId, RunId, SessionId, SpanId, SpanLink, SpanLinkKind, TraceParent,
    TurnId,
};
use uuid::Uuid;

fn request() -> RequestContext {
    RequestContext::local("session-oa02", "/workspace/project")
}

fn scope(request: &RequestContext) -> CorrelationScope {
    CorrelationScope::new(request.session_id.clone(), None, Some(ProjectId::new()))
}

#[test]
fn traceparent_is_strictly_parsed_and_only_linked_to_a_fresh_root() {
    let parent = TraceParent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
        .expect("valid W3C traceparent");
    assert_eq!(
        parent.to_header(),
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
    );
    for invalid in [
        "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
        "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
        "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
    ] {
        assert!(
            TraceParent::parse(invalid).is_err(),
            "must reject {invalid}"
        );
    }

    let request = request();
    let context = CorrelationContext::from_request(
        &request,
        scope(&request),
        7,
        11,
        Some(parent.to_header().as_str()),
    )
    .expect("root context");
    assert_ne!(context.trace_id, parent.trace_id);
    assert!(context.parent_span_id.is_none());
    assert_eq!(context.span_links.len(), 1);
    assert_eq!(
        context.span_links[0].relationship,
        SpanLinkKind::ForeignParent
    );
    assert_eq!(context.span_links[0].span, parent.parent_ref());
}

#[test]
fn request_binding_rejects_forged_actor_epoch_and_scope() {
    let request = request();
    let scope = scope(&request);
    let context = CorrelationContext::root(&request, scope.clone(), 3, 5, None).unwrap();

    let mut forged_actor = context.clone();
    forged_actor.actor_ref = "attacker".to_owned();
    assert_eq!(
        forged_actor
            .validate_for_request(&request, &scope, 3, 5)
            .unwrap_err(),
        "correlation_actor_mismatch"
    );

    let mut forged_authority = context.clone();
    forged_authority.authority_epoch = 4;
    assert_eq!(
        forged_authority
            .validate_for_request(&request, &scope, 3, 5)
            .unwrap_err(),
        "correlation_authority_epoch_mismatch"
    );

    let foreign_scope = CorrelationScope::new(
        SessionId::new("foreign-session"),
        None,
        Some(ProjectId::new()),
    );
    assert_eq!(
        context
            .validate_for_request(&request, &foreign_scope, 3, 5)
            .unwrap_err(),
        "correlation_scope_mismatch"
    );

    let encoded = serde_json::to_string(&context).unwrap();
    let decoded: CorrelationContext = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, context);
    assert!(serde_json::from_str::<CorrelationContext>(
        &encoded.replace('}', ",\"untrusted\":true}")
    )
    .is_err());
}

#[test]
fn request_binding_rejects_structurally_invalid_context_before_identity_checks() {
    let request = request();
    let scope = scope(&request);
    let root = CorrelationContext::root(&request, scope.clone(), 3, 5, None).unwrap();

    let mut forged_parent = root.clone();
    forged_parent.span_links = vec![SpanLink::new(root.current_span(), SpanLinkKind::Parent)];
    assert_eq!(
        forged_parent
            .validate_for_request(&request, &scope, 3, 5)
            .unwrap_err(),
        "correlation_parent_link_invalid"
    );

    let mut nil_request = root;
    nil_request.request_id = RequestId::from_uuid(Uuid::nil());
    assert_eq!(
        nil_request
            .validate_for_request(&request, &scope, 3, 5)
            .unwrap_err(),
        "correlation_request_id_invalid"
    );
}

#[test]
fn run_turn_invocation_attempt_links_are_ordered_and_command_bound() {
    let request = request();
    let scope = scope(&request);
    let root = CorrelationContext::root(&request, scope, 1, 1, None).unwrap();
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let invocation_id = InvocationId::new();
    let execution_id = ExecutionId::new();
    let bound = root
        .with_run(run_id)
        .unwrap()
        .with_turn(turn_id)
        .unwrap()
        .with_invocation(invocation_id, execution_id)
        .unwrap();
    let attempt = AttemptRef::new(
        run_id,
        turn_id,
        invocation_id,
        execution_id,
        bound.command_id.unwrap(),
        1,
    )
    .unwrap();
    let attempted = bound.with_attempt(attempt.clone()).unwrap();
    assert_eq!(attempted.attempt, Some(attempt));
    assert!(bound.with_turn(TurnId::new()).is_err());
    assert!(root.with_turn(turn_id).is_err());

    let mismatched = AttemptRef::new(
        run_id,
        turn_id,
        invocation_id,
        execution_id,
        RequestId::new(),
        1,
    )
    .unwrap();
    assert_eq!(
        bound.with_attempt(mismatched).unwrap_err(),
        "correlation_attempt_command_mismatch"
    );
    assert_eq!(
        bound
            .with_causation(CausationRef::Command(RequestId::new()))
            .unwrap_err(),
        "correlation_causation_command_mismatch"
    );
    assert!(bound
        .with_causation(CausationRef::Event(kiana_domain::EventId::new()))
        .is_ok());
}

#[test]
fn child_and_async_links_never_reuse_a_span_or_foreign_parent_as_owner() {
    let request = request();
    let root = CorrelationContext::root(&request, scope(&request), 1, 1, None).unwrap();
    let child = root.child_span().unwrap();
    assert_eq!(child.trace_id, root.trace_id);
    assert_ne!(child.span_id, root.span_id);
    assert_eq!(child.parent_span_id, Some(root.span_id.clone()));
    assert!(child.span_links.is_empty());

    let linked = root.linked_child(SpanLinkKind::FollowsFrom).unwrap();
    assert!(linked.parent_span_id.is_none());
    assert_eq!(linked.span_links[0].span, root.current_span());
    assert_eq!(linked.span_links[0].relationship, SpanLinkKind::FollowsFrom);
    assert_eq!(
        root.linked_child(SpanLinkKind::Parent).unwrap_err(),
        "correlation_async_parent_link_forbidden"
    );
    assert_eq!(
        root.with_source_cursor(0).unwrap_err(),
        "correlation_source_cursor_required"
    );
}

#[test]
fn source_cursor_builder_revalidates_forged_links_and_preserves_foreign_parent() {
    let request = request();
    let foreign_parent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let root =
        CorrelationContext::root(&request, scope(&request), 1, 1, Some(foreign_parent)).unwrap();

    let mut forged = root.clone();
    forged.span_links = vec![SpanLink::new(root.current_span(), SpanLinkKind::Parent)];
    assert_eq!(
        forged.with_source_cursor(7).unwrap_err(),
        "correlation_parent_link_invalid"
    );

    let advanced = root.with_source_cursor(7).unwrap();
    assert_eq!(advanced.source_cursor, Some(7));
    assert_eq!(advanced.span_links.len(), 1);
    assert_eq!(
        advanced.span_links[0].relationship,
        SpanLinkKind::ForeignParent
    );
}

#[test]
fn validate_rejects_nil_ids_and_parent_or_self_span_links() {
    let request = request();
    let root = CorrelationContext::root(&request, scope(&request), 1, 1, None).unwrap();

    let mut nil_request = root.clone();
    nil_request.request_id = RequestId::from_uuid(Uuid::nil());
    assert_eq!(
        nil_request.validate().unwrap_err(),
        "correlation_request_id_invalid"
    );

    let mut mismatched_request = root.clone();
    mismatched_request.correlation_id = RequestId::new();
    assert_eq!(
        mismatched_request.validate().unwrap_err(),
        "correlation_request_mismatch"
    );

    let mut nil_command = root.clone();
    nil_command.command_id = Some(RequestId::from_uuid(Uuid::nil()));
    assert_eq!(
        nil_command.validate().unwrap_err(),
        "correlation_command_id_invalid"
    );

    let mut nil_scope = root.clone();
    nil_scope.project_id = Some(ProjectId::from_uuid(Uuid::nil()));
    assert_eq!(
        nil_scope.validate().unwrap_err(),
        "correlation_project_id_invalid"
    );

    let mut nil_organization = root.clone();
    nil_organization.organization_id = Some(OrganizationId::from_uuid(Uuid::nil()));
    assert_eq!(
        nil_organization.validate().unwrap_err(),
        "correlation_organization_id_invalid"
    );

    let mut nil_event = root.clone();
    nil_event.causation = Some(CausationRef::Event(EventId::from_uuid(Uuid::nil())));
    assert_eq!(
        nil_event.validate().unwrap_err(),
        "correlation_causation_event_id_invalid"
    );

    let mut parent_link = root.clone();
    parent_link.span_links = vec![SpanLink::new(root.current_span(), SpanLinkKind::Parent)];
    assert_eq!(
        parent_link.validate().unwrap_err(),
        "correlation_parent_link_invalid"
    );

    let mut self_link = root;
    self_link.span_links = vec![SpanLink::new(
        self_link.current_span(),
        SpanLinkKind::FollowsFrom,
    )];
    assert_eq!(
        self_link.validate().unwrap_err(),
        "correlation_span_link_self"
    );
}

#[test]
fn wire_decode_rejects_nil_ids_and_invalid_correlation_links() {
    let request = request();
    let root = CorrelationContext::root(&request, scope(&request), 1, 1, None).unwrap();
    let valid = serde_json::to_value(&root).unwrap();

    let mut legacy_without_optional_fields = valid.clone();
    let legacy_object = legacy_without_optional_fields.as_object_mut().unwrap();
    for field in [
        "parent_span_id",
        "span_links",
        "causation",
        "command_id",
        "organization_id",
        "project_id",
        "run_id",
        "turn_id",
        "invocation_id",
        "execution_id",
        "attempt",
        "source_cursor",
    ] {
        legacy_object.remove(field);
    }
    let legacy: CorrelationContext =
        serde_json::from_value(legacy_without_optional_fields).unwrap();
    assert_eq!(legacy.request_id, root.request_id);
    assert!(legacy.span_links.is_empty());
    assert!(legacy.command_id.is_none());

    let mut nil_request = valid.clone();
    nil_request["request_id"] = serde_json::to_value(RequestId::from_uuid(Uuid::nil())).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(nil_request).is_err());

    let mut request_drift = valid.clone();
    request_drift["correlation_id"] = serde_json::to_value(RequestId::new()).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(request_drift).is_err());

    let mut nil_command = valid.clone();
    nil_command["command_id"] =
        serde_json::to_value(Some(RequestId::from_uuid(Uuid::nil()))).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(nil_command).is_err());

    let mut nil_project = valid.clone();
    nil_project["project_id"] =
        serde_json::to_value(Some(ProjectId::from_uuid(Uuid::nil()))).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(nil_project).is_err());

    let mut nil_organization = valid.clone();
    nil_organization["organization_id"] =
        serde_json::to_value(Some(OrganizationId::from_uuid(Uuid::nil()))).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(nil_organization).is_err());

    let mut nil_event = valid.clone();
    nil_event["causation"] =
        serde_json::to_value(Some(CausationRef::Event(EventId::from_uuid(Uuid::nil())))).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(nil_event).is_err());

    let mut nil_command_causation = valid.clone();
    nil_command_causation["causation"] = serde_json::to_value(Some(CausationRef::Command(
        RequestId::from_uuid(Uuid::nil()),
    )))
    .unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(nil_command_causation).is_err());

    let mut command_causation_drift = valid.clone();
    command_causation_drift["causation"] =
        serde_json::to_value(Some(CausationRef::Command(RequestId::new()))).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(command_causation_drift).is_err());

    let mut parent_link = valid.clone();
    parent_link["span_links"] = serde_json::to_value(vec![SpanLink::new(
        root.current_span(),
        SpanLinkKind::Parent,
    )])
    .unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(parent_link).is_err());

    let mut self_link = valid.clone();
    self_link["span_links"] = serde_json::to_value(vec![SpanLink::new(
        root.current_span(),
        SpanLinkKind::FollowsFrom,
    )])
    .unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(self_link).is_err());

    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let invocation_id = InvocationId::new();
    let execution_id = ExecutionId::new();
    let bound = root
        .with_run(run_id)
        .unwrap()
        .with_turn(turn_id)
        .unwrap()
        .with_invocation(invocation_id, execution_id)
        .unwrap();
    let attempt = AttemptRef::new(
        run_id,
        turn_id,
        invocation_id,
        execution_id,
        bound.command_id.unwrap(),
        1,
    )
    .unwrap();
    let attempted = bound.with_attempt(attempt).unwrap();

    let mut nil_attempt_id = serde_json::to_value(&attempted).unwrap();
    nil_attempt_id["attempt"]["run_id"] =
        serde_json::to_value(RunId::from_uuid(Uuid::nil())).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(nil_attempt_id).is_err());

    let mut attempt_command_drift = serde_json::to_value(&attempted).unwrap();
    attempt_command_drift["attempt"]["command_id"] =
        serde_json::to_value(RequestId::new()).unwrap();
    assert!(serde_json::from_value::<CorrelationContext>(attempt_command_drift).is_err());
}

#[test]
fn malformed_attempt_and_causation_command_ids_fail_closed() {
    let request = request();
    let root = CorrelationContext::root(&request, scope(&request), 1, 1, None).unwrap();
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let invocation_id = InvocationId::new();
    let execution_id = ExecutionId::new();
    let command_id = root.command_id.unwrap();

    let malformed_attempt = AttemptRef {
        run_id: RunId::from_uuid(Uuid::nil()),
        turn_id,
        invocation_id,
        execution_id,
        command_id,
        attempt: 1,
    };
    assert_eq!(
        root.with_run(run_id)
            .unwrap()
            .with_turn(turn_id)
            .unwrap()
            .with_invocation(invocation_id, execution_id)
            .unwrap()
            .with_attempt(malformed_attempt)
            .unwrap_err(),
        "correlation_attempt_id_invalid"
    );

    assert_eq!(
        root.with_causation(CausationRef::Command(RequestId::from_uuid(Uuid::nil())))
            .unwrap_err(),
        "correlation_causation_command_id_invalid"
    );
}

#[test]
fn trail_threads_authenticated_ingress_through_the_execution_spine() {
    let request = request();
    let scope = scope(&request);
    let root = CorrelationContext::root(&request, scope.clone(), 3, 5, None).unwrap();
    let command_id = root.command_id.unwrap();
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let invocation_id = InvocationId::new();
    let execution_id = ExecutionId::new();
    let trail = CorrelationTrail::from_ingress(root.clone()).unwrap();
    assert_eq!(trail.current_stage(), CorrelationStage::Ingress);
    let trail = trail
        .with_run(run_id)
        .unwrap()
        .with_turn(turn_id)
        .unwrap()
        .with_invocation(invocation_id, execution_id)
        .unwrap();
    let attempt =
        AttemptRef::new(run_id, turn_id, invocation_id, execution_id, command_id, 2).unwrap();
    let trail = trail
        .with_attempt(attempt.clone())
        .unwrap()
        .provider()
        .unwrap()
        .broker()
        .unwrap()
        .event_log(9)
        .unwrap();

    assert_eq!(trail.current_stage(), CorrelationStage::EventLog);
    assert_eq!(trail.entries().len(), CorrelationStage::ORDERED.len());
    for (index, entry) in trail.entries().iter().enumerate() {
        assert_eq!(entry.stage, CorrelationStage::ORDERED[index]);
        assert_eq!(entry.context.trace_id, root.trace_id);
        assert_eq!(entry.context.request_id, request.request_id);
        assert_eq!(entry.context.scope(), scope);
        assert_eq!(entry.context.actor_ref, root.actor_ref);
        assert_eq!(entry.context.authority_epoch, 3);
        assert_eq!(entry.context.data_epoch, 5);
        assert_eq!(entry.context.command_id, Some(command_id));
        if index > 0 {
            assert_eq!(
                entry.context.parent_span_id,
                Some(trail.entries()[index - 1].context.span_id.clone())
            );
            assert_ne!(
                entry.context.span_id,
                trail.entries()[index - 1].context.span_id
            );
        }
    }
    assert_eq!(
        trail
            .context_at(CorrelationStage::Invocation)
            .unwrap()
            .attempt,
        Some(attempt)
    );
    assert_eq!(
        trail
            .context_at(CorrelationStage::Provider)
            .unwrap()
            .attempt,
        trail
            .context_at(CorrelationStage::Invocation)
            .unwrap()
            .attempt
    );
    assert_eq!(trail.event_correlation().unwrap().source_cursor, Some(9));
    trail.validate().unwrap();
}

#[test]
fn trail_rejects_out_of_order_and_terminal_advances() {
    let request = request();
    let root = CorrelationContext::root(&request, scope(&request), 1, 1, None).unwrap();
    let trail = CorrelationTrail::from_ingress(root).unwrap();
    assert_eq!(
        trail.clone().provider().unwrap_err(),
        "correlation_trail_stage_order"
    );
    assert_eq!(
        trail.clone().with_turn(TurnId::new()).unwrap_err(),
        "correlation_trail_stage_order"
    );
    assert_eq!(
        trail
            .clone()
            .with_attempt(
                AttemptRef::new(
                    RunId::new(),
                    TurnId::new(),
                    InvocationId::new(),
                    ExecutionId::new(),
                    RequestId::new(),
                    1,
                )
                .unwrap()
            )
            .unwrap_err(),
        "correlation_attempt_requires_invocation"
    );

    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let trail = trail
        .with_run(run_id)
        .unwrap()
        .with_turn(turn_id)
        .unwrap()
        .with_invocation(InvocationId::new(), ExecutionId::new())
        .unwrap()
        .provider()
        .unwrap()
        .broker()
        .unwrap()
        .event_log(1)
        .unwrap();
    assert_eq!(
        trail.clone().event_log(2).unwrap_err(),
        "correlation_trail_terminal"
    );
    assert_eq!(
        trail.clone().broker().unwrap_err(),
        "correlation_trail_terminal"
    );
}

#[test]
fn trail_rebuild_rejects_forged_stage_order_parent_and_identity_drift() {
    let request = request();
    let root = CorrelationContext::root(&request, scope(&request), 2, 4, None).unwrap();
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let valid = CorrelationTrail::from_ingress(root)
        .unwrap()
        .with_run(run_id)
        .unwrap()
        .with_turn(turn_id)
        .unwrap()
        .with_invocation(InvocationId::new(), ExecutionId::new())
        .unwrap();

    assert_eq!(
        CorrelationTrail::from_entries(Vec::new()).unwrap_err(),
        "correlation_trail_empty"
    );

    let mut non_ingress = valid.entries().to_vec();
    non_ingress.remove(0);
    assert_eq!(
        CorrelationTrail::from_entries(non_ingress).unwrap_err(),
        "correlation_trail_ingress_required"
    );

    let mut ingress_parent = valid.entries().to_vec();
    ingress_parent[0].context.parent_span_id = Some(SpanId::new());
    assert_eq!(
        CorrelationTrail::from_entries(ingress_parent).unwrap_err(),
        "correlation_trail_ingress_parent_forbidden"
    );

    let mut reordered = valid.entries().to_vec();
    reordered[1].stage = CorrelationStage::Ingress;
    assert_eq!(
        CorrelationTrail::from_entries(reordered).unwrap_err(),
        "correlation_trail_stage_order"
    );

    let mut actor_drift = valid.entries().to_vec();
    actor_drift[2].context.actor_ref = "attacker".to_owned();
    assert_eq!(
        CorrelationTrail::from_entries(actor_drift).unwrap_err(),
        "correlation_trail_identity_drift"
    );

    let mut trace_drift = valid.entries().to_vec();
    trace_drift[3].context.trace_id = kiana_domain::TraceId::new();
    assert_eq!(
        CorrelationTrail::from_entries(trace_drift).unwrap_err(),
        "correlation_trail_trace_drift"
    );

    let mut parent_drift = valid.entries().to_vec();
    parent_drift[2].context.parent_span_id = Some(SpanId::new());
    assert_eq!(
        CorrelationTrail::from_entries(parent_drift).unwrap_err(),
        "correlation_trail_parent_link"
    );
}

#[test]
fn trail_async_recovery_uses_a_link_and_a_new_span() {
    let request = request();
    let root = CorrelationContext::root(&request, scope(&request), 1, 1, None).unwrap();
    let trail = CorrelationTrail::from_ingress(root)
        .unwrap()
        .with_run(RunId::new())
        .unwrap()
        .with_turn(TurnId::new())
        .unwrap()
        .with_invocation(InvocationId::new(), ExecutionId::new())
        .unwrap()
        .provider()
        .unwrap()
        .broker()
        .unwrap()
        .event_log(4)
        .unwrap();

    assert_eq!(
        trail.async_recovery(SpanLinkKind::Parent).unwrap_err(),
        "correlation_async_parent_link_forbidden"
    );

    let upstream = trail.current().clone();
    let recovery = trail.async_recovery(SpanLinkKind::FollowsFrom).unwrap();
    assert_eq!(recovery.upstream, upstream.current_span());
    assert_eq!(recovery.relationship, SpanLinkKind::FollowsFrom);
    assert!(recovery.context.parent_span_id.is_none());
    assert_ne!(recovery.context.span_id, upstream.span_id);
    assert_eq!(recovery.context.trace_id, upstream.trace_id);
    assert_eq!(recovery.context.run_id, None);
    assert_eq!(recovery.context.turn_id, None);
    assert_eq!(recovery.context.invocation_id, None);
    assert_eq!(recovery.context.attempt, None);
    assert_eq!(recovery.context.source_cursor, None);
    assert_eq!(
        recovery.context.span_links,
        vec![SpanLink::new(
            upstream.current_span(),
            SpanLinkKind::FollowsFrom
        )]
    );

    let recovered = recovery.trail().unwrap();
    assert_eq!(recovered.current_stage(), CorrelationStage::Ingress);
    assert_eq!(recovered.current().trace_id, upstream.trace_id);
    let resumed = recovered.with_run(RunId::new()).unwrap();
    assert_eq!(resumed.current_stage(), CorrelationStage::Run);
}
