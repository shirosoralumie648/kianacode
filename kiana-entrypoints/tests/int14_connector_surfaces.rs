//! INT-14: CLI / Web / Workbench / MCP produce one normalized connector intent.
//!
//! Every surface encodes the same typed `ConnectorCommandRequest` through
//! `kiana_entrypoints::connector_command`, and the shared ControlPlane normalizer
//! (`kiana_domain::normalize_connector_intent`) turns all four encodings into the same intent.
//! The surface label is correlation metadata only; actor, role, risk, binding snapshot and
//! endpoint stay server-owned and every forged value is refused with `broker_calls = 0`.

use kiana_domain::{
    normalize_connector_intent, ConnectorCommand, ConnectorCommandRequest,
    ConnectorNormalizedIntent, ConnectorProtocolErrorCode, ConnectorSurface, RequestContext,
};
use kiana_entrypoints::connector_command::{connector_health_route, surface_command_route};
use serde_json::{json, Value};

fn surfaces() -> [ConnectorSurface; 4] {
    [
        ConnectorSurface::Cli,
        ConnectorSurface::Web,
        ConnectorSurface::Workbench,
        ConnectorSurface::Mcp,
    ]
}

fn trusted_context() -> RequestContext {
    let mut context = RequestContext::local("session-int14", "/repo/int14");
    context.project_trusted = true;
    context
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

fn normalized_for(
    name: &str,
    arguments: Value,
    context: &RequestContext,
) -> ConnectorNormalizedIntent {
    normalize_connector_intent(name, arguments, context).unwrap()
}

#[test]
fn every_surface_gets_the_same_normalized_health_intent() {
    let context = trusted_context();
    let mut encoded_arguments = Vec::new();
    let mut intents = Vec::new();
    for surface in surfaces() {
        let route = connector_health_route(surface, "binding-demo").unwrap();
        assert!(route.typed, "surface route must use the typed encoding");
        assert_eq!(route.name, "connector.health");
        let intent = normalized_for(&route.name, route.arguments.clone(), &context);
        encoded_arguments.push(route.arguments);
        intents.push(intent);
    }
    for arguments in &encoded_arguments {
        assert_eq!(
            arguments, &encoded_arguments[0],
            "all four surfaces must encode the same bytes"
        );
    }
    let reference = &intents[0];
    assert_eq!(reference.route, "connector.health");
    assert_eq!(reference.actor_id, "local-user");
    assert_eq!(reference.data_boundary.project_root, "/repo/int14");
    assert!(!reference.data_boundary.allow_external);
    for intent in &intents {
        assert_eq!(intent, reference);
    }
}

#[test]
fn generic_surface_paths_reencode_typed_arguments_to_the_same_shape() {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some("binding-demo".to_owned());
    let typed = request.to_arguments().unwrap();
    let context = trusted_context();
    let reference = normalized_for("connector.health", typed.clone(), &context);
    for surface in surfaces() {
        let route = surface_command_route(surface, "connector.health", typed.clone()).unwrap();
        assert!(route.typed);
        assert_eq!(route.name, "connector.health");
        assert_eq!(route.arguments, typed);
        assert_eq!(
            normalized_for(&route.name, route.arguments, &context),
            reference
        );
    }
}

#[test]
fn reconcile_keeps_the_public_name_but_enters_the_existing_manage_route() {
    let context = trusted_context();
    let request = reconcile_request();
    let reference =
        kiana_domain::ConnectorSurfaceCommand::encode(ConnectorSurface::Cli, &request).unwrap();
    for surface in surfaces() {
        let encoded = kiana_domain::ConnectorSurfaceCommand::encode(surface, &request).unwrap();
        assert_eq!(encoded.wire_name, "connector.reconcile");
        assert_eq!(encoded.route, "connector.manage");
        assert_eq!(encoded.arguments, reference.arguments);
        let intent = normalized_for(&encoded.wire_name, encoded.arguments, &context);
        assert_eq!(intent.route, "connector.manage");
        assert_eq!(intent.command, ConnectorCommand::Reconcile);
    }
}

#[test]
fn forged_authority_fields_are_refused_per_surface_with_zero_broker_calls() {
    let context = trusted_context();
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some("binding-demo".to_owned());
    let typed = request.to_arguments().unwrap();
    for forge in [
        json!({"actor_id": "forged"}),
        json!({"role_id": "admin"}),
        json!({"risk": "read_only"}),
        json!({"endpoint": "https://attacker.invalid"}),
    ] {
        let mut forged = typed.clone();
        let forged_object = forged
            .as_object_mut()
            .expect("typed arguments are an object");
        for (key, value) in forge.as_object().expect("forgery is an object") {
            forged_object.insert(key.clone(), value.clone());
        }
        for surface in surfaces() {
            assert_eq!(
                surface_command_route(surface, "connector.health", forged.clone()).unwrap_err(),
                "connector_server_owned_override"
            );
            let error = normalize_connector_intent("connector.health", forged.clone(), &context)
                .unwrap_err();
            assert_eq!(error.code, ConnectorProtocolErrorCode::AuthorityOverride);
            assert_eq!(error.broker_calls, 0);
        }
    }
}

#[test]
fn untrusted_unauthenticated_and_cell_surfaces_never_reach_the_broker() {
    let route = connector_health_route(ConnectorSurface::Web, "binding-demo").unwrap();

    let untrusted = RequestContext::local("session-int14", "/repo/int14");
    let error =
        normalize_connector_intent(&route.name, route.arguments.clone(), &untrusted).unwrap_err();
    assert_eq!(error.code, ConnectorProtocolErrorCode::ProjectUntrusted);
    assert_eq!(error.broker_calls, 0);

    let mut unauthenticated = trusted_context();
    unauthenticated.actor_id = None;
    let error = normalize_connector_intent(&route.name, route.arguments.clone(), &unauthenticated)
        .unwrap_err();
    assert_eq!(
        error.code,
        ConnectorProtocolErrorCode::AuthenticationRequired
    );
    assert_eq!(error.broker_calls, 0);

    let mut cell = trusted_context();
    cell.cell_id = Some(kiana_domain::CellId::new());
    let error = normalize_connector_intent(&route.name, route.arguments, &cell).unwrap_err();
    assert_eq!(error.reason, "connector_operator_required");
    assert_eq!(error.broker_calls, 0);
}

#[test]
fn legacy_untyped_surface_arguments_stay_on_the_compatibility_path() {
    let legacy = json!({"binding_id": "binding-demo"});
    for surface in surfaces() {
        let route = surface_command_route(surface, "connector.health", legacy.clone()).unwrap();
        assert!(!route.typed);
        assert_eq!(route.name, "connector.health");
        assert_eq!(route.arguments, legacy);
    }
}
