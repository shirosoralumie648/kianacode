use kiana_domain::{
    ConnectorDefinition, ConnectorEffect, ConnectorOperation, ConnectorRegistrySnapshot,
};
use std::collections::BTreeMap;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn definition(id: &str) -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: id.to_owned(),
        version: "1.0.0".to_owned(),
        provider_id: "fixture".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            "list".to_owned(),
            ConnectorOperation {
                effect: ConnectorEffect::ReadOnly,
                required_scope: "read".to_owned(),
                data_classes: Default::default(),
            },
        )]),
        rate_limit_per_minute: 10,
        idempotency_required: false,
        reconciliation_required: false,
        data_processing: "local_only".to_owned(),
    }
}

#[test]
fn registry_snapshot_is_immutable_and_cas_versioned() {
    let first = ConnectorRegistrySnapshot::new(
        1,
        BTreeMap::from([(String::from("fixture"), definition("fixture"))]),
        digest('a'),
    )
    .unwrap();
    let second = ConnectorRegistrySnapshot::new(
        2,
        BTreeMap::from([(String::from("fixture"), definition("fixture"))]),
        digest('b'),
    )
    .unwrap();
    assert_eq!(first.cas_replace(1, second).unwrap().registry_version, 2);
}

#[test]
fn stale_or_non_monotonic_registry_updates_fail_closed() {
    let first = ConnectorRegistrySnapshot::new(
        1,
        BTreeMap::from([(String::from("fixture"), definition("fixture"))]),
        digest('a'),
    )
    .unwrap();
    let second = ConnectorRegistrySnapshot::new(
        3,
        BTreeMap::from([(String::from("fixture"), definition("fixture"))]),
        digest('b'),
    )
    .unwrap();
    assert_eq!(
        first.cas_replace(1, second).unwrap_err(),
        "connector_registry_version_not_monotonic"
    );
    let third = ConnectorRegistrySnapshot::new(
        2,
        BTreeMap::from([(String::from("fixture"), definition("fixture"))]),
        digest('c'),
    )
    .unwrap();
    assert_eq!(
        first.cas_replace(0, third).unwrap_err(),
        "connector_registry_cas_conflict"
    );
}
