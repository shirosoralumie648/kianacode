use kiana_domain::{CorrelationScope, RequestContext, SpanLinkKind, TraceParent};
use kiana_ports::{CorrelationContextPort, DomainCorrelationContextPort, PortError};

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
