use kiana_capability_broker::{
    consume_connector_credential_invocation, ConnectorCredentialLeaseFence,
};
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

#[test]
fn fence_admits_one_lease_identity_and_refuses_every_clone() {
    let (binding, invocation) = fixture();
    let invocation_id = invocation.invocation_id;
    let lease_id = invocation.lease.lease_id;
    let mut first = invocation.clone();
    let mut second = invocation.clone();
    let mut fence = ConnectorCredentialLeaseFence::new();
    assert!(fence.is_empty());

    let evidence = fence
        .consume(
            &mut first,
            &binding,
            "read",
            invocation_id,
            "broker-key",
            2_001,
        )
        .expect("first admission");
    assert!(first.lease.consumed);
    assert!(fence.is_consumed(lease_id));
    assert_eq!(fence.len(), 1);

    // The clone still carries `consumed = false`; only the server-owned fence can refuse it.
    assert!(!second.lease.consumed);
    assert!(matches!(
        fence.consume(
            &mut second,
            &binding,
            "read",
            invocation_id,
            "broker-key",
            2_002,
        ),
        Err(kiana_ports::PortError::Failed(reason)) if reason == "credential_lease_replayed"
    ));
    assert!(
        !second.lease.consumed,
        "a replayed clone must be refused without consuming it"
    );
    assert_eq!(fence.len(), 1);

    // A later attempt through the already admitted value is refused too, so the fence does not
    // behave like a single-use check that forgets its record.
    assert!(matches!(
        fence.consume(
            &mut first,
            &binding,
            "read",
            invocation_id,
            "broker-key",
            2_003,
        ),
        Err(kiana_ports::PortError::Failed(reason)) if reason == "credential_lease_replayed"
    ));
    assert_eq!(evidence.binding_id, binding.binding.binding_id);
    assert_eq!(evidence.lease_digest, first.lease.lease_digest);
}

#[test]
fn fence_refuses_a_deserialized_replay_of_one_lease() {
    let (binding, mut invocation) = fixture();
    let invocation_id = invocation.invocation_id;
    let lease_id = invocation.lease.lease_id;
    let encoded = serde_json::to_string(&invocation).expect("envelope encoding");
    let mut fence = ConnectorCredentialLeaseFence::new();

    let mut restored: ConnectorCredentialInvocation =
        serde_json::from_str(&encoded).expect("envelope decoding");
    assert_eq!(restored.lease.lease_id, lease_id);
    fence
        .consume(
            &mut restored,
            &binding,
            "read",
            invocation_id,
            "broker-key",
            2_001,
        )
        .expect("first admission from the restored copy");
    let admitted_digest = restored.lease.lease_digest.clone();

    let mut replay: ConnectorCredentialInvocation =
        serde_json::from_str(&encoded).expect("envelope decoding");
    assert!(matches!(
        fence.consume(
            &mut replay,
            &binding,
            "read",
            invocation_id,
            "broker-key",
            2_002,
        ),
        Err(kiana_ports::PortError::Failed(reason)) if reason == "credential_lease_replayed"
    ));
    assert!(!replay.lease.consumed);

    // The fence records digests only: its debug projection must carry the admitted lease digest
    // and must never carry the SecretRef key or other credential material.
    let projection = format!("{fence:?}");
    assert!(projection.contains(&admitted_digest));
    assert!(!projection.contains("INT06_BROKER_SECRET"));
}

#[test]
fn fence_refuses_a_recomputed_digest_forgery_of_one_lease() {
    let (binding, mut invocation) = fixture();
    let invocation_id = invocation.invocation_id;
    let mut fence = ConnectorCredentialLeaseFence::new();
    fence
        .consume(
            &mut invocation,
            &binding,
            "read",
            invocation_id,
            "broker-key",
            2_001,
        )
        .expect("first admission");

    // A caller that owns only the mutable envelope can reset `consumed` and recompute the lease
    // digest so the value is internally consistent again.  The digest alone cannot refuse that;
    // the server-owned lease identity ledger can, before the adapter boundary is reached.
    let mut forged = invocation.clone();
    forged.lease.consumed = false;
    forged.lease.lease_digest = forged.lease.digest();
    assert_ne!(forged.lease.lease_digest, invocation.lease.lease_digest);
    assert!(matches!(
        fence.consume(
            &mut forged,
            &binding,
            "read",
            invocation_id,
            "broker-key",
            2_002,
        ),
        Err(kiana_ports::PortError::Failed(reason)) if reason == "credential_lease_replayed"
    ));
    assert!(!forged.lease.consumed);
}
