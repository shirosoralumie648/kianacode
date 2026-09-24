use kiana_domain::{
    normalize_connector_intent, ConnectorCommand, ConnectorCommandRequest,
    ConnectorProtocolErrorCode, RequestContext,
};
use serde_json::json;

fn trusted_context() -> RequestContext {
    let mut context = RequestContext::local("session-int14", "/repo/int14");
    context.project_trusted = true;
    context
}

#[test]
fn connector_protocol_is_versioned_and_normalizes_to_one_server_route() {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some("binding-demo".to_owned());
    request.validate().unwrap();
    let arguments = request.to_arguments().unwrap();
    let normalized = normalize_connector_intent(
        ConnectorCommand::Health.wire_name(),
        arguments,
        &trusted_context(),
    )
    .unwrap();
    assert_eq!(normalized.route, "connector.health");
    assert_eq!(normalized.actor_id, "local-user");
    assert_eq!(normalized.data_boundary.allow_external, false);
    assert_eq!(normalized.data_boundary.project_root, "/repo/int14");
    normalized.validate().unwrap();
}

#[test]
fn connector_wire_authority_fields_are_rejected_before_any_broker_call() {
    let forged = json!({
        "schema": "kiana.connector-command.v1",
        "version": {"major":1,"minor":0},
        "command": "health",
        "binding_id": "binding-demo",
        "actor_id": "forged",
        "role_id": "admin",
        "risk": "read_only",
        "endpoint": "https://attacker.invalid"
    });
    let error = normalize_connector_intent(
        ConnectorCommand::Health.wire_name(),
        forged,
        &trusted_context(),
    )
    .unwrap_err();
    assert_eq!(error.code, ConnectorProtocolErrorCode::AuthorityOverride);
    assert_eq!(error.broker_calls, 0);
}

#[test]
fn untrusted_and_unauthenticated_requests_have_zero_broker_calls() {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some("binding-demo".to_owned());
    let arguments = request.to_arguments().unwrap();

    let untrusted = RequestContext::local("session-int14", "/repo/int14");
    let error = normalize_connector_intent(
        ConnectorCommand::Health.wire_name(),
        arguments.clone(),
        &untrusted,
    )
    .unwrap_err();
    assert_eq!(error.code, ConnectorProtocolErrorCode::ProjectUntrusted);
    assert_eq!(error.broker_calls, 0);

    let mut unauthenticated = trusted_context();
    unauthenticated.actor_id = None;
    let error = normalize_connector_intent(
        ConnectorCommand::Health.wire_name(),
        arguments,
        &unauthenticated,
    )
    .unwrap_err();
    assert_eq!(
        error.code,
        ConnectorProtocolErrorCode::AuthenticationRequired
    );
    assert_eq!(error.broker_calls, 0);
}

#[test]
fn reconcile_uses_existing_management_route_without_a_second_execution_path() {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Reconcile);
    request.invocation_event_id = Some("event-1".to_owned());
    request.receipt_path = Some("receipts/provider.json".to_owned());
    request.receipt_sha256 = Some(format!("sha256:{}", "a".repeat(64)));
    request.idempotency_key = Some("reconcile-1".to_owned());
    request.expected_registry_version = Some(1);
    request.reason = Some("operator evidence".to_owned());
    let normalized = normalize_connector_intent(
        ConnectorCommand::Reconcile.wire_name(),
        request.to_arguments().unwrap(),
        &trusted_context(),
    )
    .unwrap();
    assert_eq!(normalized.route, "connector.manage");
    assert_eq!(normalized.command, ConnectorCommand::Reconcile);
}
