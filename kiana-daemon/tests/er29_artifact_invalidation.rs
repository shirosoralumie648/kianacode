//! ER-29 artifact data-governance fence: the production local artifact store must deny reads of
//! a revoked version, persist the revocation across restarts and treat a replayed boundary as
//! idempotent while refusing a conflicting one.
//!
//! Deny-first: every refusal fixture runs before the success fixture.

use kiana_daemon::LocalArtifactStore;
use kiana_domain::{
    json_digest, ArtifactId, ArtifactProvenance, ArtifactRef, ArtifactVersion,
    DataPropagationState, DataPropagationTarget, RequestId,
};
use kiana_ports::{ArtifactContentPort, ArtifactStorePort, PortError};
use std::fs;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: std::env::temp_dir().join(format!("kiana-er29-artifact-{}", RequestId::new())),
        }
    }

    fn storage(&self) -> PathBuf {
        self.root.join("artifacts")
    }

    async fn open(&self) -> LocalArtifactStore {
        LocalArtifactStore::open(self.storage()).await.unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn version(content: &[u8]) -> ArtifactVersion {
    ArtifactVersion::new(
        ArtifactId::new(),
        1,
        "kiana.er29-fixture.v1",
        content,
        json_digest(&serde_json::json!({"project": "er29"})),
        ArtifactProvenance {
            producer_kind: "fixture".to_owned(),
            producer_id: "er29-local-store".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "fixture".to_owned(),
        },
        100,
    )
    .unwrap()
}

fn tombstone(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

async fn committed(store: &LocalArtifactStore, content: &[u8]) -> ArtifactRef {
    let version = version(content);
    let reference = ArtifactStorePort::stage_artifact(store, version, content.to_vec())
        .await
        .unwrap();
    ArtifactStorePort::commit_artifact(store, reference.clone(), Some(reference.version))
        .await
        .unwrap();
    reference
}

#[tokio::test]
async fn revoked_artifact_is_not_returned() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let reference = committed(&store, b"governed bytes").await;

    // Sanity: the bytes are readable before the revocation boundary.
    assert_eq!(
        ArtifactContentPort::read_artifact(&store, &reference)
            .await
            .unwrap(),
        b"governed bytes"
    );

    let receipt =
        ArtifactStorePort::invalidate_artifact(&store, &reference, 1, 2, &tombstone('a'), 500)
            .await
            .unwrap();
    assert_eq!(receipt.target, DataPropagationTarget::Artifact);
    assert_eq!(receipt.state, DataPropagationState::Invalidated);
    assert_eq!(receipt.project_ref, reference.scope_digest);
    assert!(receipt.receipt_digest.is_some());

    // Deny-first assertion: the production read path refuses revoked bytes rather than
    // reporting the version as merely uncommitted or returning the blob.
    match ArtifactContentPort::read_artifact(&store, &reference).await {
        Err(PortError::Conflict(reason)) => {
            assert_eq!(reason, "artifact_data_revoked_or_expired")
        }
        other => panic!("revoked artifact read was not denied: {other:?}"),
    }
    match ArtifactStorePort::verify_artifact(&store, &reference).await {
        Err(PortError::Conflict(reason)) => {
            assert_eq!(reason, "artifact_data_revoked_or_expired")
        }
        other => panic!("revoked artifact verify was not denied: {other:?}"),
    }
}

#[tokio::test]
async fn invalidation_replay_is_idempotent_and_conflicts_fail_closed() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let reference = committed(&store, b"replay bytes").await;
    let digest = tombstone('b');

    let first = ArtifactStorePort::invalidate_artifact(&store, &reference, 1, 2, &digest, 500)
        .await
        .unwrap();
    let replay = ArtifactStorePort::invalidate_artifact(&store, &reference, 1, 2, &digest, 777)
        .await
        .unwrap();
    assert_eq!(first.receipt_digest, replay.receipt_digest);

    // A different tombstone for the same version is a conflict, not a second revocation.
    match ArtifactStorePort::invalidate_artifact(&store, &reference, 1, 2, &tombstone('c'), 500)
        .await
    {
        Err(PortError::Conflict(reason)) => {
            assert_eq!(reason, "artifact_invalidation_conflict")
        }
        other => panic!("conflicting invalidation was not denied: {other:?}"),
    }
    match ArtifactStorePort::invalidate_artifact(&store, &reference, 1, 3, &digest, 500).await {
        Err(PortError::Conflict(reason)) => {
            assert_eq!(reason, "artifact_invalidation_conflict")
        }
        other => panic!("epoch drift invalidation was not denied: {other:?}"),
    }
}

#[tokio::test]
async fn invalidation_boundary_is_validated() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let reference = committed(&store, b"boundary bytes").await;

    for (previous, epoch, digest) in [
        (0, 2, tombstone('d')),
        (2, 2, tombstone('d')),
        (1, 1, tombstone('d')),
        (1, 2, "md5:not-a-digest".to_owned()),
    ] {
        match ArtifactStorePort::invalidate_artifact(
            &store, &reference, previous, epoch, &digest, 500,
        )
        .await
        {
            Err(PortError::Failed(reason)) => {
                assert_eq!(reason, "artifact_invalidation_boundary_invalid")
            }
            other => panic!("invalid boundary was not denied: {other:?}"),
        }
    }
    match ArtifactStorePort::invalidate_artifact(&store, &reference, 1, 2, &tombstone('d'), 0).await
    {
        Err(PortError::Failed(reason)) => {
            assert_eq!(reason, "artifact_invalidation_boundary_invalid")
        }
        other => panic!("zero observed time was not denied: {other:?}"),
    }

    // The failed attempts left no journal entry: the artifact remains readable.
    ArtifactContentPort::read_artifact(&store, &reference)
        .await
        .unwrap();
}

#[tokio::test]
async fn invalidation_requires_a_committed_version() {
    let fixture = Fixture::new();
    let store = fixture.open().await;

    let staged_version = version(b"staged only");
    let staged = ArtifactStorePort::stage_artifact(&store, staged_version, b"staged only".to_vec())
        .await
        .unwrap();
    match ArtifactStorePort::invalidate_artifact(&store, &staged, 1, 2, &tombstone('e'), 500).await
    {
        Err(PortError::Unavailable(reason)) => {
            assert_eq!(reason, "artifact_version_not_committed")
        }
        other => panic!("staged version invalidation was not denied: {other:?}"),
    }

    let unknown = version(b"never stored").as_ref();
    match ArtifactStorePort::invalidate_artifact(&store, &unknown, 1, 2, &tombstone('e'), 500).await
    {
        Err(PortError::Unavailable(reason)) => {
            assert_eq!(reason, "artifact_version_not_committed")
        }
        other => panic!("unknown version invalidation was not denied: {other:?}"),
    }
}

#[tokio::test]
async fn revocation_survives_reopen_and_blocks_late_commit() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let reference = committed(&store, b"durable revocation").await;
    ArtifactStorePort::invalidate_artifact(&store, &reference, 1, 2, &tombstone('f'), 500)
        .await
        .unwrap();
    drop(store);

    // A fresh handle over the same root must observe the persisted boundary.
    let reopened = fixture.open().await;
    match ArtifactContentPort::read_artifact(&reopened, &reference).await {
        Err(PortError::Conflict(reason)) => {
            assert_eq!(reason, "artifact_data_revoked_or_expired")
        }
        other => panic!("revocation did not survive reopen: {other:?}"),
    }

    // The commit marker was left on disk for audit, but a late commit cannot make the version
    // readable again.
    match ArtifactStorePort::commit_artifact(&reopened, reference.clone(), Some(1)).await {
        Err(PortError::Conflict(reason)) => {
            assert_eq!(reason, "artifact_data_revoked_or_expired")
        }
        other => panic!("late commit of a revoked version was not denied: {other:?}"),
    }
}
