use kiana_domain::{ArtifactId, ArtifactProvenance, ArtifactVersion};
use kiana_eventlog::MemoryArtifactStore;
use kiana_ports::{ArtifactStorePort, PortError};

const SCOPE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn version(content: &[u8]) -> ArtifactVersion {
    ArtifactVersion::new(
        ArtifactId::new(),
        1,
        "text/plain",
        content,
        SCOPE,
        ArtifactProvenance {
            producer_kind: "test".to_owned(),
            producer_id: "pd14".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "ci".to_owned(),
        },
        1,
    )
    .unwrap()
}

#[tokio::test]
async fn stage_commit_verify_and_read_preserve_immutable_manifest() {
    let store = MemoryArtifactStore::new();
    let content = b"artifact bytes".to_vec();
    let version = version(&content);
    let reference = store
        .stage_artifact(version, content.clone())
        .await
        .unwrap();
    assert_eq!(
        store.read_artifact(&reference).await.unwrap_err(),
        PortError::Unavailable("artifact_version_not_committed".to_owned())
    );
    store
        .commit_artifact(reference.clone(), Some(reference.version))
        .await
        .unwrap();
    store.verify_artifact(&reference).await.unwrap();
    assert_eq!(store.read_artifact(&reference).await.unwrap(), content);
    assert_eq!(
        store
            .stage_artifact(
                ArtifactVersion::new(
                    reference.artifact_id,
                    reference.version,
                    "text/plain",
                    b"other",
                    SCOPE,
                    ArtifactProvenance {
                        producer_kind: "test".to_owned(),
                        producer_id: "pd14".to_owned(),
                        source_event_id: None,
                        source_run_id: None,
                        recorded_by: "ci".to_owned(),
                    },
                    2,
                )
                .unwrap(),
                b"other".to_vec(),
            )
            .await
            .unwrap_err(),
        PortError::Conflict("artifact_version_already_staged".to_owned())
    );
}

#[tokio::test]
async fn hash_scope_and_manifest_drift_fail_closed() {
    let store = MemoryArtifactStore::new();
    let content = b"bounded".to_vec();
    let version = version(&content);
    let reference = store.stage_artifact(version, content).await.unwrap();
    store
        .commit_artifact(reference.clone(), None)
        .await
        .unwrap();

    let mut wrong_scope = reference.clone();
    wrong_scope.scope_digest =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    assert!(matches!(
        store.read_artifact(&wrong_scope).await.unwrap_err(),
        PortError::Conflict(reason) if reason == "artifact_reference_manifest_mismatch"
    ));

    assert_eq!(
        store.commit_artifact(reference, Some(2)).await.unwrap_err(),
        PortError::Conflict("artifact_manifest_revision_conflict".to_owned())
    );
}
