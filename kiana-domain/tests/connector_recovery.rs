use kiana_domain::{ConnectorRecoveryFact, ConnectorRecoveryState, CONNECTOR_RECOVERY_SCHEMA};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn digest(fact: &ConnectorRecoveryFact) -> String {
    kiana_domain::json_digest(&json!({
        "schema": fact.schema,
        "connector_id": fact.connector_id,
        "binding_id": fact.binding_id,
        "account_id": fact.account_id,
        "source_cursor": fact.source_cursor,
        "projection_cursor": fact.projection_cursor,
        "projection_generation": fact.projection_generation,
        "source_generation": fact.source_generation,
        "authority_epoch": fact.authority_epoch,
        "data_epoch": fact.data_epoch,
        "lease_epoch": fact.lease_epoch,
        "current_lease_epoch": fact.current_lease_epoch,
        "worker_epoch": fact.worker_epoch,
        "current_worker_epoch": fact.current_worker_epoch,
        "pending_unknown_count": fact.pending_unknown_count,
        "pending_invocation_count": fact.pending_invocation_count,
        "pending_reconciliation_count": fact.pending_reconciliation_count,
        "stale_worker_fenced": fact.stale_worker_fenced,
        "old_lease_fenced": fact.old_lease_fenced,
        "default_paused": fact.default_paused,
        "source_digest": fact.source_digest,
        "projection_digest": fact.projection_digest,
        "re_admission_refs": fact.re_admission_refs,
        "state": fact.state,
    }))
}

fn fact(state: ConnectorRecoveryState) -> ConnectorRecoveryFact {
    let needs_recovery = matches!(state, ConnectorRecoveryState::NeedsRecovery);
    let mut fact = ConnectorRecoveryFact {
        schema: CONNECTOR_RECOVERY_SCHEMA.to_owned(),
        connector_id: "connector-1".to_owned(),
        binding_id: "binding-1".to_owned(),
        account_id: "account-1".to_owned(),
        source_cursor: 20,
        projection_cursor: 20,
        projection_generation: 3,
        source_generation: 3,
        authority_epoch: 5,
        data_epoch: 4,
        lease_epoch: 2,
        current_lease_epoch: 3,
        worker_epoch: 1,
        current_worker_epoch: 2,
        pending_unknown_count: u32::from(needs_recovery),
        pending_invocation_count: 0,
        pending_reconciliation_count: u32::from(needs_recovery),
        stale_worker_fenced: true,
        old_lease_fenced: true,
        default_paused: true,
        source_digest: D1.to_owned(),
        projection_digest: D2.to_owned(),
        re_admission_refs: if matches!(
            state,
            ConnectorRecoveryState::Reconciled | ConnectorRecoveryState::ReadyForAdmission
        ) {
            vec!["admission:new-1".to_owned()]
        } else {
            Vec::new()
        },
        state,
        recovery_digest: String::new(),
    };
    fact.recovery_digest = digest(&fact);
    fact
}

#[test]
fn recovery_is_paused_and_unknown_requires_reconciliation_and_fresh_admission() {
    assert!(fact(ConnectorRecoveryState::Paused).validate().is_ok());
    assert!(fact(ConnectorRecoveryState::NeedsRecovery)
        .validate()
        .is_ok());
    assert!(fact(ConnectorRecoveryState::Reconciled).validate().is_ok());
    assert!(fact(ConnectorRecoveryState::ReadyForAdmission)
        .validate()
        .is_ok());
}

#[test]
fn stale_worker_lease_and_projection_generation_gates_reject_premature_ready() {
    let mut stale = fact(ConnectorRecoveryState::ReadyForAdmission);
    stale.stale_worker_fenced = false;
    stale.recovery_digest = digest(&stale);
    assert_eq!(
        stale.validate(),
        Err("connector_recovery_stale_worker_unfenced")
    );

    let mut lagging = fact(ConnectorRecoveryState::ReadyForAdmission);
    lagging.projection_cursor = 19;
    lagging.recovery_digest = digest(&lagging);
    assert_eq!(
        lagging.validate(),
        Err("connector_recovery_ready_gate_invalid")
    );

    let mut value = serde_json::to_value(fact(ConnectorRecoveryState::Paused)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorRecoveryFact>(value).is_err());
}
