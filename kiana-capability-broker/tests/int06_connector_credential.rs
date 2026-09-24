use kiana_capability_broker::consume_connector_credential_invocation;
use kiana_domain::*;
use std::collections::{BTreeMap, BTreeSet};

fn fixture() -> (ConnectorBindingSnapshot, ConnectorCredentialInvocation) {
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
        credential_ref: Some(
            SecretRef::new(
                "env",
                "INT06_BROKER_SECRET",
                CONNECTOR_CREDENTIAL_PURPOSE,
                "connector:connector-demo:v1",
                2,
            )
            .unwrap(),
        ),
    };
    let binding = ConnectorBindingSnapshot {
        definition,
        binding: account,
        project_root: "/repo".to_owned(),
        revision: 3,
        status: "active".to_owned(),
    };
    let request_id = RequestId::new();
    let invocation = ConnectorCredentialInvocation::issue(
        &binding,
        "read",
        request_id,
        "broker-key",
        binding.binding.credential_ref.as_ref().unwrap(),
        2_000,
        100,
    )
    .unwrap();
    (binding, invocation)
}

#[test]
fn broker_consumes_connector_lease_at_exact_binding_boundary() {
    let (binding, mut invocation) = fixture();
    let invocation_id = invocation.invocation_id;
    let evidence = consume_connector_credential_invocation(
        &mut invocation,
        &binding,
        "read",
        invocation_id,
        "broker-key",
        2_001,
    )
    .unwrap();
    assert_eq!(evidence.binding_id, binding.binding.binding_id);
    assert_eq!(evidence.account_id, binding.binding.account_id);
    assert!(matches!(
        consume_connector_credential_invocation(
            &mut invocation,
            &binding,
            "read",
            invocation_id,
            "broker-key",
            2_002,
        ),
        Err(kiana_ports::PortError::Failed(reason)) if reason == "credential_lease_replayed"
    ));
}

#[test]
fn broker_rejects_stale_binding_before_consumption() {
    let (binding, mut invocation) = fixture();
    let invocation_id = invocation.invocation_id;
    let mut stale = binding.clone();
    stale.revision += 1;
    assert!(matches!(
        consume_connector_credential_invocation(
            &mut invocation,
            &stale,
            "read",
            invocation_id,
            "broker-key",
            2_001,
        ),
        Err(kiana_ports::PortError::Failed(reason)) if reason == "connector_credential_invocation_binding_mismatch"
    ));
    assert!(!invocation.lease.consumed);
}
