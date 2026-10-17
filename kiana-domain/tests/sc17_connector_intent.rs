//! SC-17 deny-first fixtures for the typed connector ingress intent.
//!
//! A verified webhook/A2A occurrence is the only untrusted input here. These fixtures pin that
//! deriving a connector invocation intent is account/purpose/audience bound, that the inbound
//! token can never be reused as the downstream connector credential (no passthrough), and that
//! a tampered or unknown-field intent fails closed before any Broker call.

use kiana_domain::{
    connector_credential_audience, AccountBinding, ConnectorBindingSnapshot, ConnectorDefinition,
    ConnectorEffect, ConnectorIngressEvent, ConnectorIngressIntent, ConnectorIngressOccurrence,
    ConnectorIngressPolicy, ConnectorIngressProtocol, ConnectorOperation,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

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

fn verified_occurrence() -> ConnectorIngressOccurrence {
    let event = ConnectorIngressEvent::new(
        ConnectorIngressProtocol::Webhook,
        "source-1",
        "key-1",
        "tenant-1",
        "project-1",
        "event-1",
        "job.created",
        1_000,
        "nonce-1",
        json!({"id": "job-1", "value": 1}),
        "ab".repeat(32),
    )
    .unwrap();
    ConnectorIngressOccurrence::from_verified(&event, &policy()).unwrap()
}

fn snapshot(status: &str) -> ConnectorBindingSnapshot {
    let definition = ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: "connector-demo".to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-demo".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([
            (
                "read".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            ),
            (
                "write".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::Write,
                    required_scope: "write".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            ),
        ]),
        rate_limit_per_minute: 10,
        idempotency_required: true,
        reconciliation_required: true,
        data_processing: "local_only".to_owned(),
    };
    let binding = AccountBinding {
        schema: "kiana.account-binding.v1".to_owned(),
        binding_id: "binding-demo".to_owned(),
        connector_id: "connector-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        read_scopes: BTreeSet::from(["read".to_owned()]),
        write_scopes: BTreeSet::from(["write".to_owned()]),
        fixture_path: "fixtures/connector.json".to_owned(),
        fixture_sha256: format!("sha256:{}", "a".repeat(64)),
        credential_ref: None,
    };
    ConnectorBindingSnapshot {
        definition,
        binding,
        project_root: "/tmp/sc17-connector-intent".to_owned(),
        revision: 1,
        status: status.to_owned(),
    }
}

fn derive(binding: &ConnectorBindingSnapshot, operation: &str) -> ConnectorIngressIntent {
    ConnectorIngressIntent::from_verified_occurrence(
        &verified_occurrence(),
        binding,
        "project-1",
        INGRESS_AUDIENCE,
        INGRESS_AUDIENCE,
        operation,
        "idem-1",
    )
    .unwrap()
}

#[test]
fn intent_binds_account_purpose_and_separated_downstream_audience() {
    let intent = derive(&snapshot("active"), "read");
    intent.validate().unwrap();
    assert_eq!(intent.account_id, "account-demo");
    assert_eq!(intent.binding_id, "binding-demo");
    assert_eq!(intent.operation, "read");
    assert_eq!(intent.purpose, "connector.invoke");
    let expected_audience = connector_credential_audience("connector-demo", "v1");
    assert_eq!(intent.downstream_audience, expected_audience);
    // The inbound token audience and the downstream connector audience stay distinct.
    assert_ne!(intent.ingress_audience, intent.downstream_audience);
    // Deriving an intent never dispatches an effect.
    assert_eq!(intent.broker_calls, 0);
    assert_eq!(intent.occurrence_digest.len(), "sha256:".len() + 64);
    assert_eq!(
        intent.occurrence_key(),
        "connector-ingress:source-1:event-1"
    );
}

#[test]
fn wrong_project_or_ingress_audience_is_denied() {
    let occurrence = verified_occurrence();
    let binding = snapshot("active");
    let wrong_project = ConnectorIngressIntent::from_verified_occurrence(
        &occurrence,
        &binding,
        "project-other",
        INGRESS_AUDIENCE,
        INGRESS_AUDIENCE,
        "read",
        "idem-1",
    )
    .unwrap_err();
    assert_eq!(wrong_project, "connector_ingress_project_denied");

    // A token minted for a different ingress resource must not be accepted.
    let wrong_audience = ConnectorIngressIntent::from_verified_occurrence(
        &occurrence,
        &binding,
        "project-1",
        INGRESS_AUDIENCE,
        "kiana-ingress:source-other",
        "read",
        "idem-1",
    )
    .unwrap_err();
    assert_eq!(wrong_audience, "connector_intent_ingress_audience_mismatch");
}

#[test]
fn reusing_the_ingress_token_for_the_connector_is_a_passthrough_and_is_denied() {
    let downstream = connector_credential_audience("connector-demo", "v1");
    let error = ConnectorIngressIntent::from_verified_occurrence(
        &verified_occurrence(),
        &snapshot("active"),
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
fn unbound_account_operation_and_revoked_binding_are_denied() {
    let occurrence = verified_occurrence();
    // Registered operation that the account has no scope for.
    let mut unbound = snapshot("active");
    unbound.binding.read_scopes.clear();
    let scope_error = ConnectorIngressIntent::from_verified_occurrence(
        &occurrence,
        &unbound,
        "project-1",
        INGRESS_AUDIENCE,
        INGRESS_AUDIENCE,
        "read",
        "idem-1",
    )
    .unwrap_err();
    assert_eq!(scope_error, "connector_account_scope_denied");

    // Operation not registered on the server-owned definition.
    let unknown_operation = ConnectorIngressIntent::from_verified_occurrence(
        &occurrence,
        &snapshot("active"),
        "project-1",
        INGRESS_AUDIENCE,
        INGRESS_AUDIENCE,
        "missing",
        "idem-1",
    )
    .unwrap_err();
    assert_eq!(unknown_operation, "connector_operation_unregistered");

    // A revoked binding cannot produce an intent even with a valid occurrence.
    let revoked = ConnectorIngressIntent::from_verified_occurrence(
        &occurrence,
        &snapshot("revoked"),
        "project-1",
        INGRESS_AUDIENCE,
        INGRESS_AUDIENCE,
        "read",
        "idem-1",
    )
    .unwrap_err();
    assert_eq!(revoked, "connector_binding_revoked");
}

#[test]
fn missing_idempotency_key_is_denied() {
    let error = ConnectorIngressIntent::from_verified_occurrence(
        &verified_occurrence(),
        &snapshot("active"),
        "project-1",
        INGRESS_AUDIENCE,
        INGRESS_AUDIENCE,
        "read",
        "   ",
    )
    .unwrap_err();
    assert_eq!(error, "connector_intent_idempotency_key_invalid");
}

#[test]
fn derived_intent_is_fail_closed_against_tampering_and_unknown_fields() {
    let original = derive(&snapshot("active"), "read");

    let mutations: [fn(&mut serde_json::Value); 5] = [
        |value: &mut serde_json::Value| {
            value["account_id"] = json!("account-other");
        },
        |value: &mut serde_json::Value| {
            value["purpose"] = json!("connector.reconcile");
        },
        |value: &mut serde_json::Value| {
            value["downstream_audience"] = json!("connector:other:v1");
        },
        |value: &mut serde_json::Value| {
            value["broker_calls"] = json!(1);
        },
        |value: &mut serde_json::Value| {
            value["ingress_audience"] = json!("connector:connector-demo:v1");
        },
    ];
    for mutate in mutations {
        let mut value = serde_json::to_value(&original).unwrap();
        mutate(&mut value);
        let tampered: ConnectorIngressIntent = serde_json::from_value(value).unwrap();
        assert_eq!(
            tampered.validate().unwrap_err(),
            "connector_ingress_intent_invalid"
        );
    }

    let mut value = serde_json::to_value(&original).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorIngressIntent>(value).is_err());
}
