use kiana_domain::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn binding() -> ConnectorBindingSnapshot {
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

fn fact(status: ConnectorHealthStatus) -> ConnectorHealthFact {
    ConnectorHealthFact::new(
        "connector-demo",
        "binding-demo",
        status,
        "read_only",
        1_700_000_000_000,
        Some(json_digest(&json!({"fixture": "a"}))),
        None,
        1,
        None,
        CONNECTOR_FIXTURE_SOURCE,
        vec!["local_fixture_only".to_owned()],
    )
    .expect("valid health fact")
}

#[test]
fn health_status_classification_is_stable_and_redacted() {
    assert_eq!(
        classify_connector_health_error("invalid_token"),
        ConnectorHealthStatus::CredentialInvalid
    );
    assert_eq!(
        classify_connector_health_error("missing_scope"),
        ConnectorHealthStatus::ScopeInsufficient
    );
    assert_eq!(
        classify_connector_health_error("connection_refused"),
        ConnectorHealthStatus::EndpointUnreachable
    );
    assert_eq!(
        classify_connector_health_error("transport_unsupported"),
        ConnectorHealthStatus::Unsupported
    );
    assert_eq!(
        classify_connector_health_error("provider_specific_code"),
        ConnectorHealthStatus::ProviderError
    );

    for status in [
        ConnectorHealthStatus::ConnectivityOnly,
        ConnectorHealthStatus::CredentialInvalid,
        ConnectorHealthStatus::ScopeInsufficient,
        ConnectorHealthStatus::EndpointUnreachable,
        ConnectorHealthStatus::ProviderError,
        ConnectorHealthStatus::Unsupported,
    ] {
        assert!(fact(status).validate().is_ok());
    }
    assert!(ConnectorHealthFact::new(
        "connector-demo",
        "binding-demo",
        ConnectorHealthStatus::Verified,
        "read_only",
        1_700_000_000_000,
        None,
        None,
        1,
        None,
        CONNECTOR_FIXTURE_SOURCE,
        Vec::new(),
    )
    .is_err());
}

#[test]
fn connector_registry_accepts_only_binding_scoped_health_facts() {
    let state = binding();
    let binding_event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "connector.binding",
        json!({"state": state}),
    )
    .unwrap()
    .with_stream_metadata(CONNECTOR_STREAM, "/repo", 1);
    let health_event = RuntimeEvent::new(
        RequestId::new(),
        1,
        CONNECTOR_HEALTH_EVENT_KIND,
        json!({"health": fact(ConnectorHealthStatus::ConnectivityOnly)}),
    )
    .unwrap()
    .with_stream_metadata(CONNECTOR_STREAM, "/repo", 2);
    let events = vec![binding_event, health_event];
    let (version, bindings) = connector_bindings(&events).expect("valid health event");
    assert_eq!(version, 2);
    assert_eq!(bindings["binding-demo"].revision, 1);

    let foreign = ConnectorHealthFact::new(
        "connector-demo",
        "binding-other",
        ConnectorHealthStatus::ConnectivityOnly,
        "read_only",
        1_700_000_000_000,
        None,
        None,
        1,
        None,
        CONNECTOR_FIXTURE_SOURCE,
        Vec::new(),
    )
    .unwrap();
    let foreign_event = RuntimeEvent::new(
        RequestId::new(),
        1,
        CONNECTOR_HEALTH_EVENT_KIND,
        json!({"health": foreign}),
    )
    .unwrap()
    .with_stream_metadata(CONNECTOR_STREAM, "/repo", 2);
    assert_eq!(
        connector_bindings(&[events[0].clone(), foreign_event]).unwrap_err(),
        "connector_registry_event_invalid"
    );
}
