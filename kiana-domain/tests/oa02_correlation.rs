use kiana_domain::{
    AttemptRef, CausationRef, CorrelationContext, CorrelationScope, ExecutionId, InvocationId,
    ProjectId, RequestContext, RequestId, RunId, SessionId, SpanLinkKind, TraceParent, TurnId,
};

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
