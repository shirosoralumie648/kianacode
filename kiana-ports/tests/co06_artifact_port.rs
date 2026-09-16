use kiana_domain::{ArtifactProvenance, ArtifactVersion, ProjectId};
use kiana_ports::{ArtifactContentPort, InMemoryArtifactContentStore, PortError};

#[tokio::test]
async fn artifact_read_port_requires_persisted_hash_matching_blob() {
    let scope = kiana_domain::json_digest(&serde_json::json!({"project": ProjectId::new()}));
    let version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-artifact.v1",
        b"immutable",
        scope,
        ArtifactProvenance {
            producer_kind: "fixture".to_owned(),
            producer_id: "co06".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "fixture".to_owned(),
        },
        100,
    )
    .unwrap();
    let reference = version.as_ref();
    let store = InMemoryArtifactContentStore::new();
    assert!(matches!(
        store.read_artifact(&reference).await,
        Err(PortError::Unavailable(reason)) if reason == "artifact_blob_missing"
    ));
    store.put(&version, b"immutable".to_vec()).await.unwrap();
    assert_eq!(store.read_artifact(&reference).await.unwrap(), b"immutable");

    let mut forged = reference.clone();
    forged.content_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    assert!(matches!(
        store.read_artifact(&forged).await,
        Err(PortError::Conflict(reason)) if reason == "artifact_content_hash_mismatch"
    ));

    let mut foreign_scope = reference;
    foreign_scope.scope_digest = kiana_domain::json_digest(&serde_json::json!({"foreign": true}));
    assert!(matches!(
        store.read_artifact(&foreign_scope).await,
        Err(PortError::Conflict(reason)) if reason == "artifact_scope_mismatch"
    ));
}
