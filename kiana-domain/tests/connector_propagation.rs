use kiana_domain::{
    ConnectorPropagationFact, ConnectorPropagationState, CONNECTOR_PROPAGATION_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn digest(fact: &ConnectorPropagationFact) -> String {
    kiana_domain::json_digest(&json!({
        "schema": fact.schema,
        "connector_id": fact.connector_id,
        "binding_id": fact.binding_id,
        "account_id": fact.account_id,
        "source_project": fact.source_project,
        "target_project": fact.target_project,
        "object_ref": fact.object_ref,
        "data_class": fact.data_class,
        "purpose": fact.purpose,
        "scope_digest": fact.scope_digest,
        "sharing_grant_digest": fact.sharing_grant_digest,
        "retention_policy_digest": fact.retention_policy_digest,
        "source_cursor": fact.source_cursor,
        "data_epoch": fact.data_epoch,
        "state": fact.state,
        "tombstone_epoch": fact.tombstone_epoch,
        "memory_invalidated": fact.memory_invalidated,
        "index_invalidated": fact.index_invalidated,
        "cache_invalidated": fact.cache_invalidated,
    }))
}

fn fact(state: ConnectorPropagationState) -> ConnectorPropagationFact {
    let invalidated = !matches!(state, ConnectorPropagationState::Active);
    let mut fact = ConnectorPropagationFact {
        schema: CONNECTOR_PROPAGATION_SCHEMA.to_owned(),
        connector_id: "connector-1".to_owned(),
        binding_id: "binding-1".to_owned(),
        account_id: "account-1".to_owned(),
        source_project: "project-a".to_owned(),
        target_project: "project-a".to_owned(),
        object_ref: "artifact:one".to_owned(),
        data_class: "internal".to_owned(),
        purpose: "connector.read".to_owned(),
        scope_digest: D1.to_owned(),
        sharing_grant_digest: None,
        retention_policy_digest: D2.to_owned(),
        source_cursor: 12,
        data_epoch: 4,
        state,
        tombstone_epoch: invalidated.then_some(4),
        memory_invalidated: invalidated,
        index_invalidated: invalidated,
        cache_invalidated: invalidated,
        propagation_digest: String::new(),
    };
    fact.propagation_digest = digest(&fact);
    fact
}

#[test]
fn active_and_revoked_states_bind_scope_purpose_retention_and_epoch() {
    assert!(fact(ConnectorPropagationState::Active).validate().is_ok());
    assert!(fact(ConnectorPropagationState::Revoked).validate().is_ok());
    assert!(fact(ConnectorPropagationState::Expired).validate().is_ok());
    assert!(fact(ConnectorPropagationState::Quarantined)
        .validate()
        .is_ok());
}

#[test]
fn cross_project_without_grant_and_uninvalidated_views_fail_closed() {
    let mut cross = fact(ConnectorPropagationState::Active);
    cross.target_project = "project-b".to_owned();
    cross.propagation_digest = digest(&cross);
    assert_eq!(
        cross.validate(),
        Err("connector_propagation_sharing_grant_required")
    );

    let mut revoked = fact(ConnectorPropagationState::Revoked);
    revoked.cache_invalidated = false;
    revoked.propagation_digest = digest(&revoked);
    assert_eq!(
        revoked.validate(),
        Err("connector_propagation_derived_view_not_invalidated")
    );

    let mut value = serde_json::to_value(fact(ConnectorPropagationState::Active)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorPropagationFact>(value).is_err());
}
