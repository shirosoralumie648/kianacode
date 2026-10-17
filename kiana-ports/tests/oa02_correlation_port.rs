use kiana_domain::{
    AttemptRef, CorrelationScope, CorrelationStage, ExecutionId, InvocationId, RequestContext,
    RunId, SpanLink, SpanLinkKind, TraceParent, TurnId,
};
use kiana_ports::{
    CorrelationContextPort, CorrelationTrailPort, DomainCorrelationContextPort,
    DomainCorrelationTrailPort, PortError,
};

#[test]
fn default_port_delegates_server_root_and_child_construction() {
    let request = RequestContext::local("oa02-port-session", "/workspace/project");
    let scope = CorrelationScope::new(request.session_id.clone(), None, None);
    let port = DomainCorrelationContextPort;
    let root = port
        .root(&request, scope, 2, 4, None)
        .expect("root context through port");
    let child = port.child_span(&root).expect("child span through port");
    assert_eq!(child.parent_span_id, Some(root.span_id.clone()));
    let recovery = port
        .linked_child(&root, SpanLinkKind::FollowsFrom)
        .expect("linked recovery span through port");
    assert!(recovery.parent_span_id.is_none());
    assert_eq!(recovery.span_links.len(), 1);
}

#[test]
fn port_preserves_fail_closed_traceparent_and_epoch_errors() {
    let request = RequestContext::local("oa02-port-session", "/workspace/project");
    let port = DomainCorrelationContextPort;
    let scope = CorrelationScope::new(request.session_id.clone(), None, None);
    for (authority_epoch, data_epoch) in [(0, 4), (2, 0)] {
        let error = port
            .root(&request, scope.clone(), authority_epoch, data_epoch, None)
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Failed("correlation_context:correlation_epoch_required".to_owned())
        );
    }
    let error = port
        .root(
            &request,
            scope,
            2,
            4,
            Some("ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        )
        .unwrap_err();
    assert_eq!(
        error,
        PortError::Failed("correlation_context:traceparent_invalid".to_owned())
    );
}

#[test]
fn port_links_valid_traceparent_to_a_fresh_server_owned_root() {
    let request = RequestContext::local("oa02-port-session", "/workspace/project");
    let scope = CorrelationScope::new(request.session_id.clone(), None, None);
    let port = DomainCorrelationContextPort;
    let header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let parent = TraceParent::parse(header).expect("valid external traceparent");
    let root = port
        .root(&request, scope.clone(), 2, 4, Some(header))
        .expect("valid traceparent is link-only");

    assert_ne!(root.trace_id, parent.trace_id);
    assert!(root.parent_span_id.is_none());
    assert_eq!(root.span_links.len(), 1);
    assert_eq!(root.span_links[0].relationship, SpanLinkKind::ForeignParent);
    assert_eq!(root.span_links[0].span, parent.parent_ref());
    root.validate_for_request(&request, &scope, 2, 4)
        .expect("request identity, scope and epochs remain server owned");
}

#[test]
fn port_rejects_forged_parent_and_self_links_before_child_creation() {
    let request = RequestContext::local("oa02-port-session", "/workspace/project");
    let scope = CorrelationScope::new(request.session_id.clone(), None, None);
    let port = DomainCorrelationContextPort;
    let root = port
        .root(&request, scope, 2, 4, None)
        .expect("root context through port");

    let mut parent_link = root.clone();
    parent_link.span_links = vec![SpanLink::new(root.current_span(), SpanLinkKind::Parent)];
    assert_eq!(
        port.child_span(&parent_link).unwrap_err(),
        PortError::Failed("correlation_context:correlation_parent_link_invalid".to_owned())
    );

    let mut self_link = root;
    self_link.span_links = vec![SpanLink::new(
        self_link.current_span(),
        SpanLinkKind::FollowsFrom,
    )];
    assert_eq!(
        port.child_span(&self_link).unwrap_err(),
        PortError::Failed("correlation_context:correlation_span_link_self".to_owned())
    );
}

#[test]
fn trail_port_threads_ingress_through_the_spine_and_rejects_out_of_order() {
    let request = RequestContext::local("oa02-port-session", "/workspace/project");
    let scope = CorrelationScope::new(request.session_id.clone(), None, None);
    let context_port = DomainCorrelationContextPort;
    let trail_port = DomainCorrelationTrailPort;
    let root = context_port.root(&request, scope, 2, 4, None).unwrap();
    let command_id = root.command_id.unwrap();
    let trail = trail_port.start(root).unwrap();
    assert_eq!(trail.current_stage(), CorrelationStage::Ingress);

    assert_eq!(
        trail_port.provider(&trail).unwrap_err(),
        PortError::Failed("correlation_trail:correlation_trail_stage_order".to_owned())
    );

    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let invocation_id = InvocationId::new();
    let execution_id = ExecutionId::new();
    let trail = trail_port.with_run(&trail, run_id).unwrap();
    let trail = trail_port.with_turn(&trail, turn_id).unwrap();
    let trail = trail_port
        .with_invocation(&trail, invocation_id, execution_id)
        .unwrap();
    let attempt =
        AttemptRef::new(run_id, turn_id, invocation_id, execution_id, command_id, 1).unwrap();
    let trail = trail_port.with_attempt(&trail, attempt).unwrap();
    let trail = trail_port.provider(&trail).unwrap();
    let trail = trail_port.broker(&trail).unwrap();
    let trail = trail_port.event_log(&trail, 5).unwrap();

    assert_eq!(trail.current_stage(), CorrelationStage::EventLog);
    assert_eq!(trail.entries().len(), CorrelationStage::ORDERED.len());
    assert_eq!(
        trail.current().trace_id,
        trail.entries()[0].context.trace_id
    );
    assert_eq!(trail.event_correlation().unwrap().source_cursor, Some(5));

    let recovery = trail_port
        .async_recovery(&trail, SpanLinkKind::FollowsFrom)
        .unwrap();
    assert!(recovery.context.parent_span_id.is_none());
    assert_ne!(recovery.context.span_id, trail.current().span_id);
    assert_eq!(recovery.context.trace_id, trail.current().trace_id);
    assert_eq!(
        trail_port
            .async_recovery(&trail, SpanLinkKind::Parent)
            .unwrap_err(),
        PortError::Failed("correlation_trail:correlation_async_parent_link_forbidden".to_owned())
    );
    let resumed = recovery.trail().unwrap();
    assert_eq!(resumed.current_stage(), CorrelationStage::Ingress);
}
