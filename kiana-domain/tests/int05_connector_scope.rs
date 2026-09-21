use kiana_domain::{ConnectorBindingStatus, ConnectorScopeBinding};
use std::collections::BTreeSet;

fn binding(read: &[&str], write: &[&str], expiry: u64, epoch: u64) -> ConnectorScopeBinding {
    let mut value = ConnectorScopeBinding {
        schema: kiana_domain::CONNECTOR_SCOPE_SCHEMA.to_owned(),
        binding_id: "binding-1".to_owned(),
        owner_id: "owner-1".to_owned(),
        project_root: "/repo".to_owned(),
        read_scopes: read.iter().map(|value| (*value).to_owned()).collect(),
        write_scopes: write.iter().map(|value| (*value).to_owned()).collect(),
        data_classes: BTreeSet::from(["internal".to_owned()]),
        data_epoch: epoch,
        expires_at_unix_ms: expiry,
        status: ConnectorBindingStatus::Active,
        revision: 1,
        scope_digest: String::new(),
    };
    value.scope_digest = value.digest();
    value
}

#[test]
fn binding_scope_intersection_can_only_narrow() {
    let parent = binding(&["read", "write"], &["write"], 100, 4);
    let child = binding(&["read"], &[], 90, 4);
    assert_eq!(parent.intersect(&child).unwrap().expires_at_unix_ms, 90);
}

#[test]
fn binding_scope_owner_epoch_or_widening_conflicts_fail_closed() {
    let parent = binding(&["read"], &[], 100, 4);
    let mut widened = binding(&["read", "write"], &[], 100, 4);
    widened.scope_digest = widened.digest();
    assert_eq!(
        parent.intersect(&widened).unwrap_err(),
        "connector_scope_widening_denied"
    );
    let wrong_epoch = binding(&["read"], &[], 90, 5);
    assert_eq!(
        parent.intersect(&wrong_epoch).unwrap_err(),
        "connector_scope_identity_or_epoch_mismatch"
    );
}
