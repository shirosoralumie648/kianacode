use kiana_domain::{CorrelationScope, RequestContext, SpanLinkKind};
use kiana_ports::{CorrelationContextPort, DomainCorrelationContextPort};

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
    assert!(port
        .root(
            &request,
            scope.clone(),
            0,
            1,
            Some("00-00000000000000000000000000000001-00f067aa0ba902b7-01"),
        )
        .is_err());
    let error = port
        .root(
            &request,
            scope,
            2,
            4,
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        )
        .unwrap_err();
    assert!(error.to_string().contains("traceparent"));
}
