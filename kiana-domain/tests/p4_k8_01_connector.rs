use kiana_domain::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

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
    };
    ConnectorBindingSnapshot {
        definition,
        binding,
        project_root: "/tmp/p4-k8-01".to_owned(),
        revision: 1,
        status: status.to_owned(),
    }
}

#[test]
fn connector_contracts_are_scoped_and_fail_closed() {
    let active = snapshot("active");
    active.validate().unwrap();
    assert_eq!(
        active.operation("read").unwrap().risk(),
        RiskLevel::ReadOnly
    );
    assert_eq!(
        active.operation("write").unwrap().risk(),
        RiskLevel::ExternalSideEffect
    );
    assert_eq!(
        active.operation("missing").unwrap_err(),
        "connector_operation_unregistered"
    );

    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Tool,
        CONNECTOR_INVOKE_OPERATION,
        json!({
            "operation": "read",
            "binding_snapshot": active
        }),
    );
    assert_eq!(
        connector_invocation_risk(&request).unwrap(),
        RiskLevel::ReadOnly
    );

    let revoked = snapshot("revoked");
    assert_eq!(
        revoked.operation("read").unwrap_err(),
        "connector_binding_revoked"
    );

    let mut unsupported = snapshot("active");
    unsupported.definition.adapter = "http".to_owned();
    assert_eq!(
        unsupported.validate().unwrap_err(),
        "connector_transport_not_supported"
    );

    let mut unknown = serde_json::to_value(active).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorBindingSnapshot>(unknown).is_err());
}
