//! SC-17 daemon behaviour fixtures for the MCP/connector/webhook ingress boundary.
//!
//! These run the production verifier and domain contracts end to end: a signed webhook becomes a
//! typed occurrence, the occurrence becomes an account/purpose/audience bound connector intent,
//! and the intent only ever produces a normalized ControlPlane request. Bad signatures, unknown
//! sources, replays and token passthrough are denied before any occurrence or intent exists.

use kiana_daemon::ConnectorIngressVerifier;
use kiana_domain::{
    connector_credential_audience, normalize_connector_intent, AccountBinding,
    ConnectorBindingSnapshot, ConnectorCommand, ConnectorCommandRequest, ConnectorDefinition,
    ConnectorEffect, ConnectorIngressEvent, ConnectorIngressIntent, ConnectorIngressOccurrence,
    ConnectorIngressPolicy, ConnectorIngressProtocol, ConnectorOperation,
    ConnectorProtocolErrorCode, RequestContext,
};
use kiana_ports::PortError;
use ring::hmac;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const SECRET: &[u8] = b"0123456789abcdef-secret";
const INGRESS_AUDIENCE: &str = "kiana-ingress:source-1";

fn policy() -> ConnectorIngressPolicy {
    ConnectorIngressPolicy::new(
        ConnectorIngressProtocol::Webhook,
        "source-1",
        "key-1",
        "tenant-1",
        "project-1",
        ["job.created".to_owned()],
        ["id".to_owned()],
        ["id".to_owned(), "value".to_owned()],
        100,
        4_096,
    )
    .unwrap()
}

fn make_event(source_id: &str, event_id: &str, value: i64) -> ConnectorIngressEvent {
    ConnectorIngressEvent::new(
        ConnectorIngressProtocol::Webhook,
        source_id,
        "key-1",
        "tenant-1",
        "project-1",
        event_id,
        "job.created",
        1_000,
        format!("nonce-{event_id}"),
        json!({"id": event_id, "value": value}),
        "00".repeat(32),
    )
    .unwrap()
}

fn resign(event: &mut ConnectorIngressEvent) {
    event.ingress_digest = kiana_domain::json_digest(&json!({
        "schema": event.schema,
        "protocol": event.protocol,
        "source_id": event.source_id,
        "key_id": event.key_id,
        "tenant_id": event.tenant_id,
        "project_id": event.project_id,
        "event_id": event.event_id,
        "event_kind": event.event_kind,
        "occurred_at_unix_ms": event.occurred_at_unix_ms,
        "nonce": event.nonce,
        "payload": event.payload,
        "payload_digest": event.payload_digest,
        "signature": event.signature,
        "signature_algorithm": event.signature_algorithm,
    }));
}

fn sign(event: &mut ConnectorIngressEvent) {
    let key = hmac::Key::new(hmac::HMAC_SHA256, SECRET);
    let tag = hmac::sign(&key, event.signing_digest().as_bytes());
    event.signature = tag
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    resign(event);
}

fn signed_event(event_id: &str, value: i64) -> ConnectorIngressEvent {
    let mut event = make_event("source-1", event_id, value);
    sign(&mut event);
    event
}

fn split_event(event_id: &str, value: i64) -> ConnectorIngressEvent {
    let mut event = make_event("source-1", event_id, value);
    event.signature = "00".repeat(32);
    resign(&mut event);
    event
}

fn binding(status: &str) -> ConnectorBindingSnapshot {
    let definition = ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: "connector-demo".to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-demo".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            "read".to_owned(),
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
    };
    let account = AccountBinding {
        schema: "kiana.account-binding.v1".to_owned(),
        binding_id: "binding-demo".to_owned(),
        connector_id: "connector-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        read_scopes: BTreeSet::from(["read".to_owned()]),
        write_scopes: BTreeSet::new(),
        fixture_path: "fixtures/connector.json".to_owned(),
        fixture_sha256: format!("sha256:{}", "a".repeat(64)),
        credential_ref: None,
    };
    ConnectorBindingSnapshot {
        definition,
        binding: account,
        project_root: "/tmp/sc17-connector-ingress".to_owned(),
        revision: 1,
        status: status.to_owned(),
    }
}

fn verifier() -> ConnectorIngressVerifier {
    ConnectorIngressVerifier::new([(policy(), SECRET.to_vec())]).unwrap()
}

fn verified_occurrence() -> ConnectorIngressOccurrence {
    let verified = verifier().verify(signed_event("event-1", 1), 1_050);
    verified.unwrap().unwrap()
}

fn derive_intent(binding: &ConnectorBindingSnapshot) -> ConnectorIngressIntent {
    ConnectorIngressIntent::from_verified_occurrence(
        &verified_occurrence(),
        binding,
        "project-1",
        INGRESS_AUDIENCE,
        INGRESS_AUDIENCE,
        "read",
        "idem-1",
    )
    .unwrap()
}

#[test]
fn signed_webhook_becomes_an_account_bound_intent_with_separated_audience() {
    let intent = derive_intent(&binding("active"));
    intent.validate().unwrap();
    assert_eq!(intent.account_id, "account-demo");
    assert_eq!(intent.binding_id, "binding-demo");
    assert_eq!(intent.operation, "read");
    assert_eq!(intent.broker_calls, 0);
    let expected_audience = connector_credential_audience("connector-demo", "v1");
    assert_eq!(intent.downstream_audience, expected_audience);
    assert_ne!(intent.ingress_audience, intent.downstream_audience);
}

#[test]
fn bad_signature_unknown_source_and_replay_never_produce_an_occurrence() {
    let verifier = verifier();

    let rejected = verifier.verify(split_event("event-2", 1), 1_050);
    let expected = PortError::Conflict("connector_ingress_signature_invalid".to_owned());
    assert_eq!(rejected.unwrap_err(), expected);

    let mut unknown = make_event("source-unknown", "event-3", 1);
    sign(&mut unknown);
    let rejected = verifier.verify(unknown, 1_050);
    let expected = PortError::Conflict("connector_ingress_source_not_allowlisted".to_owned());
    assert_eq!(rejected.unwrap_err(), expected);

    let replay = verifier.verify(signed_event("event-4", 1), 1_050);
    assert!(replay.unwrap().is_some());
    let duplicate = verifier.verify(signed_event("event-4", 1), 1_050);
    assert!(duplicate.unwrap().is_none());
    let conflict = verifier.verify(signed_event("event-4", 2), 1_050);
    let expected = PortError::Conflict("connector_ingress_dedupe_payload_conflict".to_owned());
    assert_eq!(conflict.unwrap_err(), expected);
}

#[test]
fn ingress_token_cannot_be_reused_as_the_connector_credential() {
    let downstream = connector_credential_audience("connector-demo", "v1");
    let error = ConnectorIngressIntent::from_verified_occurrence(
        &verified_occurrence(),
        &binding("active"),
        "project-1",
        downstream.as_str(),
        downstream.as_str(),
        "read",
        "idem-1",
    )
    .unwrap_err();
    assert_eq!(error, "connector_intent_token_passthrough_denied");
}

#[test]
fn intent_only_reaches_control_plane_as_a_server_shaped_request() {
    let intent = derive_intent(&binding("active"));

    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Invoke);
    request.binding_id = Some(intent.binding_id.clone());
    request.operation = Some(intent.operation.clone());
    request.payload = Some(json!({"id": intent.event_id}));
    request.idempotency_key = Some("idem-1".to_owned());

    let mut context = RequestContext::local("session-sc17", "/repo/sc17");
    context.project_trusted = true;
    let normalized = normalize_connector_intent(
        ConnectorCommand::Invoke.wire_name(),
        request.to_arguments().unwrap(),
        &context,
    )
    .unwrap();
    assert_eq!(normalized.route, "connector.invoke");
    assert_eq!(normalized.actor_id, "local-user");
    assert_eq!(normalized.data_boundary.allow_external, false);

    // A forged authority field is still rejected before any Broker call.
    let mut forged = request.to_arguments().unwrap();
    forged["actor_id"] = json!("forged");
    let error = normalize_connector_intent(
        ConnectorCommand::Invoke.wire_name(),
        forged,
        &context,
    )
    .unwrap_err();
    assert_eq!(error.code, ConnectorProtocolErrorCode::AuthorityOverride);
    assert_eq!(error.broker_calls, 0);
}
