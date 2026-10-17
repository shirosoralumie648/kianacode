use kiana_domain::{
    normalize_connector_intent, normalize_surface_connector_arguments, AccountBinding,
    ConnectorBindingSnapshot, ConnectorCommand, ConnectorCommandRequest, ConnectorDefinition,
    ConnectorEffect, ConnectorManageAction, ConnectorOperation, ConnectorProtocolErrorCode,
    ConnectorSurface, ConnectorSurfaceCommand, RequestContext,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn surfaces() -> [ConnectorSurface; 4] {
    [
        ConnectorSurface::Cli,
        ConnectorSurface::Web,
        ConnectorSurface::Workbench,
        ConnectorSurface::Mcp,
    ]
}

/// A caller-supplied binding snapshot is always server-owned; the fixture only proves the wire
/// boundary refuses it before anything else reads the request.
fn forged_binding_snapshot() -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: ConnectorDefinition {
            schema: "kiana.connector-definition.v1".to_owned(),
            connector_id: "connector-demo".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-demo".to_owned(),
            adapter: "local_fixture".to_owned(),
            operations: BTreeMap::from([(
                "lookup".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            )]),
            rate_limit_per_minute: 10,
            idempotency_required: true,
            reconciliation_required: true,
            data_processing: "local_only".to_owned(),
        },
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: "binding-demo".to_owned(),
            connector_id: "connector-demo".to_owned(),
            account_id: "account-demo".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/connector.json".to_owned(),
            fixture_sha256: format!("sha256:{}", "a".repeat(64)),
            credential_ref: None,
        },
        project_root: "/repo".to_owned(),
        revision: 1,
        status: "active".to_owned(),
    }
}

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

fn reconcile_request() -> ConnectorCommandRequest {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Reconcile);
    request.invocation_event_id = Some("event-1".to_owned());
    request.receipt_path = Some("receipts/provider.json".to_owned());
    request.receipt_sha256 = Some(format!("sha256:{}", "a".repeat(64)));
    request.idempotency_key = Some("reconcile-1".to_owned());
    request.expected_registry_version = Some(1);
    request.reason = Some("operator evidence".to_owned());
    request
}

#[test]
fn surface_commands_share_one_surface_independent_encoding() {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some("binding-demo".to_owned());
    let reference = ConnectorSurfaceCommand::encode(surfaces()[0], &request).unwrap();
    reference.validate().unwrap();
    for surface in surfaces() {
        let encoded = ConnectorSurfaceCommand::encode(surface, &request).unwrap();
        assert_eq!(encoded.surface, surface);
        assert_eq!(encoded.wire_name, "connector.health");
        assert_eq!(encoded.route, "connector.health");
        assert_eq!(encoded.arguments, reference.arguments);
        encoded.validate().unwrap();
    }
    assert_eq!(reference.schema, "kiana.connector-surface-command.v1");
}

#[test]
fn surface_encoding_refuses_forged_authority_before_any_wire_use() {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some("binding-demo".to_owned());
    let forgeries: [fn(&mut ConnectorCommandRequest); 5] = [
        |request| request.actor_id = Some("forged".to_owned()),
        |request| request.role_id = Some("admin".to_owned()),
        |request| request.risk = Some(kiana_domain::RiskLevel::ReadOnly),
        |request| request.endpoint = Some("https://attacker.invalid".to_owned()),
        |request| request.binding_snapshot = Some(forged_binding_snapshot()),
    ];
    for forge in forgeries {
        let mut forged = request.clone();
        forge(&mut forged);
        for surface in surfaces() {
            assert_eq!(
                ConnectorSurfaceCommand::encode(surface, &forged).unwrap_err(),
                "connector_server_owned_override"
            );
        }
    }
}

#[test]
fn surface_command_validation_pins_route_wire_name_and_arguments() {
    let request = reconcile_request();
    let mut encoded = ConnectorSurfaceCommand::encode(ConnectorSurface::Mcp, &request).unwrap();
    assert_eq!(encoded.wire_name, "connector.reconcile");
    assert_eq!(encoded.route, "connector.manage");
    encoded.validate().unwrap();

    let mut route_drift = encoded.clone();
    route_drift.route = "connector.reconcile".to_owned();
    assert_eq!(
        route_drift.validate().unwrap_err(),
        "connector_surface_command_invalid"
    );

    let mut wire_drift = encoded.clone();
    wire_drift.wire_name = "connector.health".to_owned();
    assert_eq!(
        wire_drift.validate().unwrap_err(),
        "connector_surface_command_invalid"
    );

    let mut argument_drift = encoded;
    argument_drift.arguments["command"] = json!("health");
    assert_eq!(
        argument_drift.validate().unwrap_err(),
        "connector_surface_command_arguments_invalid"
    );
}

#[test]
fn surface_normalization_only_accepts_matching_typed_arguments() {
    let legacy = json!({"binding_id": "binding-demo"});
    assert!(normalize_surface_connector_arguments(
        ConnectorSurface::Cli,
        "connector.health",
        &legacy
    )
    .unwrap()
    .is_none());

    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some("binding-demo".to_owned());
    let typed = request.to_arguments().unwrap();
    let encoded =
        normalize_surface_connector_arguments(ConnectorSurface::Web, "connector.health", &typed)
            .unwrap()
            .unwrap();
    assert_eq!(encoded.surface, ConnectorSurface::Web);
    assert_eq!(encoded.arguments, typed);

    assert_eq!(
        normalize_surface_connector_arguments(ConnectorSurface::Web, "connector.invoke", &typed)
            .unwrap_err(),
        "connector_command_mismatch"
    );

    let mut forged = typed.clone();
    forged["actor_id"] = json!("forged");
    assert_eq!(
        normalize_surface_connector_arguments(
            ConnectorSurface::Workbench,
            "connector.health",
            &forged
        )
        .unwrap_err(),
        "connector_server_owned_override"
    );

    let mut unknown = typed.clone();
    unknown["unexpected"] = json!(true);
    assert_eq!(
        normalize_surface_connector_arguments(ConnectorSurface::Mcp, "connector.health", &unknown)
            .unwrap_err(),
        "connector_protocol_decode_failed"
    );
}

#[test]
fn manage_and_invoke_commands_are_validated_and_share_the_surface_encoding() {
    // invoke: binding/operation/payload/idempotency are required and manage-only fields fail.
    let mut invoke = ConnectorCommandRequest::new(ConnectorCommand::Invoke);
    assert_eq!(
        invoke.validate().unwrap_err(),
        "connector_invoke_fields_invalid"
    );
    invoke.binding_id = Some("binding-demo".to_owned());
    invoke.operation = Some("lookup".to_owned());
    invoke.payload = Some(json!({"id": "one"}));
    invoke.idempotency_key = Some("key-1".to_owned());
    let encoded = ConnectorSurfaceCommand::encode(ConnectorSurface::Web, &invoke).unwrap();
    assert_eq!(encoded.route, "connector.invoke");
    let normalized = normalize_connector_intent(
        ConnectorCommand::Invoke.wire_name(),
        encoded.arguments,
        &trusted_context(),
    )
    .unwrap();
    assert_eq!(normalized.route, "connector.invoke");

    // manage list: read-only action, mutation fields are refused.
    let mut list = ConnectorCommandRequest::new(ConnectorCommand::Manage);
    assert_eq!(list.validate().unwrap_err(), "connector_action_required");
    list.action = Some(ConnectorManageAction::List);
    list.reason = Some("operator evidence".to_owned());
    assert_eq!(
        list.validate().unwrap_err(),
        "connector_manage_list_mutation_fields"
    );
    list.reason = None;
    let encoded = ConnectorSurfaceCommand::encode(ConnectorSurface::Cli, &list).unwrap();
    let normalized = normalize_connector_intent(
        ConnectorCommand::Manage.wire_name(),
        encoded.arguments,
        &trusted_context(),
    )
    .unwrap();
    assert_eq!(normalized.route, "connector.manage");

    // bind and revoke require their exact field sets; neither accepts the other's fields.
    let mut bind = ConnectorCommandRequest::new(ConnectorCommand::Manage);
    bind.action = Some(ConnectorManageAction::Bind);
    bind.expected_registry_version = Some(1);
    bind.reason = Some("operator evidence".to_owned());
    bind.idempotency_key = Some("key-2".to_owned());
    assert_eq!(
        bind.validate().unwrap_err(),
        "connector_bind_fields_required"
    );
    let mut revoke = ConnectorCommandRequest::new(ConnectorCommand::Manage);
    revoke.action = Some(ConnectorManageAction::Revoke);
    revoke.binding_id = Some("binding-demo".to_owned());
    revoke.expected_registry_version = Some(1);
    revoke.reason = Some("operator evidence".to_owned());
    revoke.idempotency_key = Some("key-3".to_owned());
    revoke.definition = Some(forged_binding_snapshot().definition);
    assert_eq!(
        revoke.validate().unwrap_err(),
        "connector_revoke_fields_required"
    );
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
