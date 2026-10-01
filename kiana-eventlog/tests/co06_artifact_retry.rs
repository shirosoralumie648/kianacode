use kiana_domain::{ArtifactId, ArtifactProvenance, ArtifactVersion};
use kiana_eventlog::MemoryArtifactStore;
use kiana_ports::{ArtifactStorePort, PortError};

const SCOPE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn version(id: ArtifactId, content: &[u8], recorded_at: u64) -> ArtifactVersion {
    ArtifactVersion::new(
        id,
        1,
        "kiana.company-artifact.v1",
        content,
        SCOPE,
        ArtifactProvenance {
            producer_kind: "company_command".to_owned(),
            producer_id: "register_artifact".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "actor".to_owned(),
        },
        recorded_at,
    )
    .expect("valid fixture version")
}

#[tokio::test]
async fn retry_returns_first_manifest_timestamp_and_commit_is_idempotent() {
    let store = MemoryArtifactStore::new();
    let id = ArtifactId::new();
    let first = store
        .stage_artifact_version(version(id, b"original", 10), b"original".to_vec())
        .await
        .expect("first stage");
    let retry = store
        .stage_artifact_version(version(id, b"original", 20), b"original".to_vec())
        .await
        .expect("same immutable retry");
    assert_eq!(retry, first);
    store
        .commit_artifact(first.as_ref(), Some(first.version))
        .await
        .expect("first commit");
    store
        .commit_artifact(retry.as_ref(), Some(retry.version))
        .await
        .expect("idempotent retry commit");
}

#[tokio::test]
async fn retry_with_hash_or_provenance_drift_is_rejected() {
    let store = MemoryArtifactStore::new();
    let id = ArtifactId::new();
    store
        .stage_artifact_version(version(id, b"original", 10), b"original".to_vec())
        .await
        .expect("first stage");
    assert_eq!(
        store
            .stage_artifact_version(version(id, b"changed", 20), b"changed".to_vec())
            .await
            .expect_err("hash drift must conflict"),
        PortError::Conflict("artifact_version_already_staged".to_owned())
    );
    let mut provenance_drift = version(id, b"original", 30);
    provenance_drift.provenance.producer_id = "forged".to_owned();
    assert_eq!(
        store
            .stage_artifact_version(provenance_drift, b"original".to_vec())
            .await
            .expect_err("provenance drift must conflict"),
        PortError::Conflict("artifact_version_already_staged".to_owned())
    );
}
