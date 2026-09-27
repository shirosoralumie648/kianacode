use kiana_domain::{
    RestoreVerificationFact, RestoreVerificationState, RESTORE_VERIFICATION_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn digest(fact: &RestoreVerificationFact) -> String {
    kiana_domain::json_digest(&json!({
        "schema": fact.schema,
        "snapshot_manifest_digest": fact.snapshot_manifest_digest,
        "owner_scope": fact.owner_scope,
        "store_id": fact.store_id,
        "previous_instance_id": fact.previous_instance_id,
        "restored_instance_id": fact.restored_instance_id,
        "source_cursor": fact.source_cursor,
        "projection_cursor": fact.projection_cursor,
        "projection_digest": fact.projection_digest,
        "data_epoch": fact.data_epoch,
        "previous_data_epoch": fact.previous_data_epoch,
        "authority_epoch": fact.authority_epoch,
        "previous_authority_epoch": fact.previous_authority_epoch,
        "artifact_refs": fact.artifact_refs,
        "pending_approval_count": fact.pending_approval_count,
        "active_lease_count": fact.active_lease_count,
        "trigger_count": fact.trigger_count,
        "unknown_effect_count": fact.unknown_effect_count,
        "old_instance_fenced": fact.old_instance_fenced,
        "old_authority_fenced": fact.old_authority_fenced,
        "external_effects_reconciled": fact.external_effects_reconciled,
        "default_paused": fact.default_paused,
        "reconciliation_refs": fact.reconciliation_refs,
        "state": fact.state,
        "reason": fact.reason,
    }))
}

fn fact(state: RestoreVerificationState) -> RestoreVerificationFact {
    let mut fact = RestoreVerificationFact {
        schema: RESTORE_VERIFICATION_SCHEMA.to_owned(),
        snapshot_manifest_digest: D1.to_owned(),
        owner_scope: "project:/repo".to_owned(),
        store_id: "store:one".to_owned(),
        previous_instance_id: Some("instance:old".to_owned()),
        restored_instance_id: "instance:new".to_owned(),
        source_cursor: 12,
        projection_cursor: 12,
        projection_digest: D2.to_owned(),
        data_epoch: 2,
        previous_data_epoch: Some(1),
        authority_epoch: 4,
        previous_authority_epoch: Some(3),
        artifact_refs: vec!["artifact:manifest".to_owned(), "artifact:acl".to_owned()],
        pending_approval_count: u32::from(matches!(
            state,
            RestoreVerificationState::NeedsReconciliation
        )),
        active_lease_count: 0,
        trigger_count: 0,
        unknown_effect_count: u32::from(matches!(
            state,
            RestoreVerificationState::NeedsReconciliation
        )),
        old_instance_fenced: true,
        old_authority_fenced: true,
        external_effects_reconciled: matches!(state, RestoreVerificationState::Verified),
        default_paused: true,
        reconciliation_refs: if matches!(state, RestoreVerificationState::NeedsReconciliation) {
            vec!["reconcile:restore-1".to_owned()]
        } else {
            Vec::new()
        },
        state,
        reason: if matches!(state, RestoreVerificationState::Verified) {
            "ok".to_owned()
        } else {
            "pending_reconciliation".to_owned()
        },
        verification_digest: String::new(),
    };
    fact.verification_digest = digest(&fact);
    fact
}

#[test]
fn verified_restore_requires_new_epochs_fences_and_quiescent_references() {
    assert!(fact(RestoreVerificationState::Verified).validate().is_ok());
    assert!(fact(RestoreVerificationState::NeedsReconciliation)
        .validate()
        .is_ok());

    let mut rollback = fact(RestoreVerificationState::Verified);
    rollback.authority_epoch = 3;
    rollback.verification_digest = digest(&rollback);
    assert_eq!(
        rollback.validate(),
        Err("restore_verification_fact_invalid")
    );

    let mut unverified = fact(RestoreVerificationState::Verified);
    unverified.unknown_effect_count = 1;
    unverified.verification_digest = digest(&unverified);
    assert_eq!(unverified.validate(), Err("restore_verification_not_ready"));
}

#[test]
fn old_instance_fence_and_shape_tampering_fail_closed() {
    let mut unfenced = fact(RestoreVerificationState::Verified);
    unfenced.old_instance_fenced = false;
    unfenced.verification_digest = digest(&unfenced);
    assert_eq!(unfenced.validate(), Err("restore_old_instance_not_fenced"));

    let mut value = serde_json::to_value(fact(RestoreVerificationState::Verified)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<RestoreVerificationFact>(value).is_err());
}
