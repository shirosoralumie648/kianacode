use kiana_daemon::{
    build_ingress_correlation_context, build_ingress_correlation_trail, validate_protected_ingress,
};
use kiana_domain::{
    AttemptRef, CorrelationStage, ExecutionId, InvocationId, RequestContext, RunId, SpanLinkKind,
    TurnId,
};
use kiana_protocol::RequestMetadata;

#[test]
fn protected_ingress_rejects_bad_origin_host_and_missing_credential() {
    let mut metadata = RequestMetadata::local("session-1", "/repo");
    metadata.origin = Some("https://evil.example".to_owned());
    assert_eq!(
        validate_protected_ingress(&metadata)
            .unwrap_err()
            .to_string(),
        "port_failed:ingress_origin_not_loopback"
    );

    metadata.origin = None;
    metadata.host = Some("evil.example".to_owned());
    assert_eq!(
        validate_protected_ingress(&metadata)
            .unwrap_err()
            .to_string(),
        "port_failed:ingress_host_not_loopback"
    );

    metadata.host = Some("127.0.0.1:8787".to_owned());
    metadata.identity_mode = Some("protected_local".to_owned());
    assert_eq!(
        validate_protected_ingress(&metadata)
            .unwrap_err()
            .to_string(),
        "port_failed:ingress_protected_credentials_required"
    );
}

#[test]
fn protected_loopback_metadata_accepts_opaque_credential_reference() {
    let mut metadata = RequestMetadata::local("session-1", "/repo");
    metadata.instance_id = Some("instance-1".to_owned());
    metadata.origin = Some("http://localhost:8787".to_owned());
    metadata.host = Some("127.0.0.1:8787".to_owned());
    metadata.identity_mode = Some("protected_local".to_owned());
    metadata.credential_ref = Some(
        kiana_domain::SecretRef::new(
            "env",
            "KIANA_LOCAL_CREDENTIAL",
            "daemon.ingress",
            "kiana-daemon",
            1,
        )
        .unwrap(),
    );
    validate_protected_ingress(&metadata).unwrap();
}

#[test]
fn traceparent_is_optional_link_only_metadata_and_invalid_values_are_rejected() {
    let mut metadata = RequestMetadata::local("session-1", "/repo");
    metadata.traceparent = Some("00-not-a-traceparent".to_owned());
    assert_eq!(
        validate_protected_ingress(&metadata)
            .unwrap_err()
            .to_string(),
        "port_failed:ingress_traceparent_invalid:traceparent_invalid"
    );

    let request = kiana_domain::RequestContext::local("session-1", "/repo");
    let project = kiana_domain::ProjectIdentity::new(
        "/repo",
        "/repo",
        Some(1),
        Some(2),
        kiana_domain::json_digest(&serde_json::json!("trusted")),
    )
    .unwrap();
    let parent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let context =
        build_ingress_correlation_context(&request, &project, 7, 11, Some(parent)).unwrap();
    assert_ne!(context.trace_id.as_str(), &parent[3..35]);
    assert_eq!(context.actor_ref, "local-user");
    assert_eq!(context.authority_epoch, 7);
    assert_eq!(context.data_epoch, 11);
    assert_eq!(context.span_links.len(), 1);
    assert_eq!(
        context.span_links[0].relationship,
        kiana_domain::SpanLinkKind::ForeignParent
    );
}

#[test]
fn authenticated_ingress_trail_threads_the_spine_and_recovers_via_link() {
    let request = RequestContext::local("session-1", "/repo");
    let project = kiana_domain::ProjectIdentity::new(
        "/repo",
        "/repo",
        Some(1),
        Some(2),
        kiana_domain::json_digest(&serde_json::json!("trusted")),
    )
    .unwrap();
    let parent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let trail = build_ingress_correlation_trail(&request, &project, 7, 11, Some(parent)).unwrap();
    assert_eq!(trail.current_stage(), CorrelationStage::Ingress);
    assert_eq!(trail.current().actor_ref, "local-user");
    assert_eq!(trail.current().authority_epoch, 7);
    assert_eq!(trail.current().data_epoch, 11);
    assert_eq!(trail.entries()[0].context.span_links.len(), 1);
    assert_eq!(
        trail.entries()[0].context.span_links[0].relationship,
        SpanLinkKind::ForeignParent
    );
    let command_id = trail.current().command_id.unwrap();

    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let invocation_id = InvocationId::new();
    let execution_id = ExecutionId::new();
    let attempt = AttemptRef::new(
        run_id,
        turn_id,
        invocation_id,
        execution_id,
        command_id,
        1,
    )
    .unwrap();
    let trail = trail
        .with_run(run_id)
        .unwrap()
        .with_turn(turn_id)
        .unwrap()
        .with_invocation(invocation_id, execution_id)
        .unwrap()
        .with_attempt(attempt)
        .unwrap()
        .provider()
        .unwrap()
        .broker()
        .unwrap()
        .event_log(13)
        .unwrap();

    assert_eq!(trail.current_stage(), CorrelationStage::EventLog);
    assert_eq!(trail.entries().len(), CorrelationStage::ORDERED.len());
    assert_eq!(trail.event_correlation().unwrap().source_cursor, Some(13));

    let recovery = trail.async_recovery(SpanLinkKind::FollowsFrom).unwrap();
    assert!(recovery.context.parent_span_id.is_none());
    assert_ne!(recovery.context.span_id, trail.current().span_id);
    assert_eq!(recovery.context.trace_id, trail.current().trace_id);
    assert_eq!(
        recovery.context.span_links[0].span,
        trail.current().current_span()
    );
}
