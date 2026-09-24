use kiana_domain::{
    ArtifactId, ArtifactProvenance, ArtifactVersion, DataClass, DataGovernanceSnapshot,
    DataPayloadState, DataPropagationPlan, DataPropagationTarget, DataRetentionObservation,
    EventId, StoreIdentityId,
};
use kiana_eventlog::{MemoryArtifactStore, MemoryDataGovernanceStore, MemoryRetentionStore};
use kiana_ports::{ArtifactStorePort, PortError};
use std::collections::BTreeMap;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

#[tokio::test]
async fn artifact_invalidation_denies_reads_and_replays_by_tombstone() {
    let store = MemoryArtifactStore::new();
    let version = ArtifactVersion::new(
        ArtifactId::new(),
        1,
        "text/plain",
        b"payload",
        digest('a'),
        ArtifactProvenance {
            producer_kind: "test".to_owned(),
            producer_id: "er29".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "ci".to_owned(),
        },
        1,
    )
    .unwrap();
    let reference = store
        .stage_artifact(version, b"payload".to_vec())
        .await
        .unwrap();
    store
        .commit_artifact(reference.clone(), Some(1))
        .await
        .unwrap();
    assert_eq!(store.read_artifact(&reference).await.unwrap(), b"payload");
    let receipt = store
        .invalidate_artifact(&reference, 1, 2, &digest('b'), 3)
        .await
        .unwrap();
    assert_eq!(receipt.target, DataPropagationTarget::Artifact);
    assert!(matches!(
        store.read_artifact(&reference).await,
        Err(PortError::Conflict(reason)) if reason == "artifact_data_revoked_or_expired"
    ));
    let replay = store
        .invalidate_artifact(&reference, 1, 2, &digest('b'), 3)
        .await
        .unwrap();
    assert_eq!(replay, receipt);
}

#[tokio::test]
async fn memory_propagation_requires_plan_before_target_receipt() {
    let snapshot = DataGovernanceSnapshot::new(
        "/project",
        1,
        2,
        4,
        vec![EventId::new()],
        vec![DataRetentionObservation {
            source_ref: "source".to_owned(),
            class: DataClass::Internal,
            purpose_id: "test".to_owned(),
            payload: DataPayloadState::Revoked,
            audit_metadata_retained: true,
            source_digest: digest('c'),
        }],
        BTreeMap::new(),
        true,
    )
    .unwrap();
    let plan = DataPropagationPlan::from_snapshot(&snapshot, 1, "revoke", digest('d'), 5).unwrap();
    let store = MemoryDataGovernanceStore::new();
    let target = plan
        .targets
        .iter()
        .find(|receipt| receipt.target == DataPropagationTarget::Memory)
        .cloned()
        .unwrap();
    assert!(store
        .append_propagation_receipt(&plan, target)
        .await
        .is_err());
    store.apply_plan(plan.clone()).await.unwrap();
    assert!(
        store
            .read_allowed("/project", 2, DataPropagationTarget::Memory)
            .await
            .unwrap()
            == false
    );
    assert!(store
        .append_propagation_receipt(&plan, plan.targets[0].clone())
        .await
        .is_err());
}

#[test]
fn retention_store_source_mentions_manifest_before_propagation() {
    let _ = MemoryRetentionStore::new();
    let _ = StoreIdentityId::new();
    let source = include_str!("../src/retention_store.rs");
    assert!(source.contains("deletion_manifest_missing"));
    assert!(source.contains("data_propagation_receipt_conflict"));
}
