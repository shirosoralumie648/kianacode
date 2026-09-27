use kiana_daemon::verify_restore;
use kiana_domain::{
    RestoreVerificationFact, RestoreVerificationState, SnapshotFileSeal, SnapshotManifest,
    SnapshotMode, RESTORE_VERIFICATION_SCHEMA,
};
use serde_json::json;

const PROJECTION: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fact(manifest: &SnapshotManifest) -> RestoreVerificationFact {
    let mut fact = RestoreVerificationFact {
        schema: RESTORE_VERIFICATION_SCHEMA.to_owned(),
        snapshot_manifest_digest: manifest.manifest_digest.clone(),
        owner_scope: manifest.owner_scope.clone(),
        store_id: manifest.store_id.clone(),
        previous_instance_id: Some(manifest.instance_id.clone()),
        restored_instance_id: "instance:new".to_owned(),
        source_cursor: manifest.source_cursor,
        projection_cursor: manifest.source_cursor,
        projection_digest: PROJECTION.to_owned(),
        data_epoch: manifest.data_epoch + 1,
        previous_data_epoch: Some(manifest.data_epoch),
        authority_epoch: 3,
        previous_authority_epoch: Some(2),
        artifact_refs: vec!["artifact:manifest".to_owned()],
        pending_approval_count: 0,
        active_lease_count: 0,
        trigger_count: 0,
        unknown_effect_count: 0,
        old_instance_fenced: true,
        old_authority_fenced: true,
        external_effects_reconciled: true,
        default_paused: true,
        reconciliation_refs: Vec::new(),
        state: RestoreVerificationState::Verified,
        reason: "ok".to_owned(),
        verification_digest: String::new(),
    };
    fact.verification_digest = kiana_domain::json_digest(&json!({
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
    }));
    fact
}

fn manifest() -> SnapshotManifest {
    SnapshotManifest::new(
        "snapshot:one",
        "project:/repo",
        "store:one",
        "instance:old",
        "/repo/.kiana",
        "/backup/repo",
        SnapshotMode::Full,
        12,
        2,
        None,
        true,
        true,
        vec![SnapshotFileSeal {
            relative_path: "events.jsonl".to_owned(),
            size_bytes: 1,
            content_hash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
        }],
    )
    .unwrap()
}

#[test]
fn restore_verifier_accepts_bound_verified_fact_and_rejects_scope_drift() {
    let manifest = manifest();
    let mut fact = fact(&manifest);
    verify_restore(&manifest, &fact).unwrap();
    fact.owner_scope = "project:/other".to_owned();
    assert_eq!(
        verify_restore(&manifest, &fact),
        Err("restore_verification_fact_invalid")
    );
}
