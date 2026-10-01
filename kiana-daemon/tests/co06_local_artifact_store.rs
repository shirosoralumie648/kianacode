#![cfg(unix)]

use kiana_daemon::LocalArtifactStore;
use kiana_domain::{ArtifactId, ArtifactProvenance, ArtifactRef, ArtifactVersion, RequestId};
use kiana_ports::{ArtifactContentPort, ArtifactStorePort, PortError};
use std::fs;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: std::env::temp_dir().join(format!("kiana-co06-{}", RequestId::new())),
        }
    }

    async fn open(&self) -> LocalArtifactStore {
        LocalArtifactStore::open(self.root.join("artifacts"))
            .await
            .unwrap()
    }

    fn object_file(&self, extension: &str) -> PathBuf {
        let scope = fs::read_dir(self.root.join("artifacts"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .next()
            .unwrap();
        let artifact = fs::read_dir(scope)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .next()
            .unwrap();
        fs::read_dir(artifact)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|value| value == extension))
            .unwrap()
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
        "kiana.co06-fixture.v1",
        content,
        kiana_domain::json_digest(&serde_json::json!({"project": "co06"})),
        ArtifactProvenance {
            producer_kind: "fixture".to_owned(),
            producer_id: "co06-local-store".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "fixture".to_owned(),
        },
        100,
    )
    .unwrap()
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
async fn local_artifact_store_requires_absolute_root_and_rejects_symlink_components() {
    use std::os::unix::fs::symlink;

    assert_eq!(
        LocalArtifactStore::open("relative-artifacts")
            .await
            .unwrap_err(),
        PortError::Failed("artifact_root_must_be_absolute".to_owned())
    );
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("outside")).unwrap();
    symlink(fixture.root.join("outside"), fixture.root.join("link")).unwrap();
    assert!(LocalArtifactStore::open(fixture.root.join("link/artifacts"))
        .await
        .is_err());
    assert_eq!(fs::read_dir(fixture.root.join("outside")).unwrap().count(), 0);
    assert!(LocalArtifactStore::open(fixture.root.join("outside/../escape"))
        .await
        .is_err());
    assert!(!fixture.root.join("escape").exists());
}

#[tokio::test]
async fn local_artifact_stage_rejects_invalid_hash_size_schema_and_provenance_before_writing() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let good = version(b"immutable");
    let mut bad_size = good.clone();
    bad_size.size_bytes += 1;
    let mut bad_hash = good.clone();
    bad_hash.content_hash = "0".repeat(64);
    let mut bad_schema = good.clone();
    bad_schema.schema = "kiana.artifact-version.v99".to_owned();
    let mut bad_provenance = good.clone();
    bad_provenance.provenance.recorded_by.clear();
    for rejected in [bad_size, bad_hash, bad_schema, bad_provenance] {
        assert!(ArtifactStorePort::stage_artifact(&store, rejected, b"immutable".to_vec())
            .await
            .is_err());
    }
    assert!(ArtifactStorePort::stage_artifact(&store, good, b"different".to_vec())
        .await
        .is_err());
    assert_eq!(fs::read_dir(fixture.root.join("artifacts")).unwrap().count(), 0);
}

#[tokio::test]
async fn local_artifact_read_requires_commit_and_commit_requires_exact_reference_and_revision() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let version = version(b"immutable");
    let reference = version.as_ref();
    assert_eq!(
        ArtifactContentPort::read_artifact(&store, &reference).await,
        Err(PortError::Unavailable("artifact_version_not_committed".to_owned()))
    );
    assert_eq!(
        ArtifactStorePort::commit_artifact(&store, reference.clone(), Some(1)).await,
        Err(PortError::Unavailable("artifact_version_not_staged".to_owned()))
    );
    ArtifactStorePort::stage_artifact(&store, version, b"immutable".to_vec())
        .await
        .unwrap();
    assert_eq!(
        ArtifactStorePort::read_artifact(&store, &reference).await,
        Err(PortError::Unavailable("artifact_version_not_committed".to_owned()))
    );
    assert_eq!(
        ArtifactStorePort::commit_artifact(&store, reference.clone(), Some(0)).await,
        Err(PortError::Conflict("artifact_manifest_revision_conflict".to_owned()))
    );
    let mut forged = reference.clone();
    forged.provenance.producer_id = "foreign-producer".to_owned();
    assert_eq!(
        ArtifactStorePort::commit_artifact(&store, forged, Some(1)).await,
        Err(PortError::Conflict("artifact_reference_manifest_mismatch".to_owned()))
    );
    assert_eq!(
        ArtifactContentPort::read_artifact(&store, &reference).await,
        Err(PortError::Unavailable("artifact_version_not_committed".to_owned()))
    );
}

#[tokio::test]
async fn local_artifact_store_rejects_scope_hash_schema_identity_and_provenance_drift() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let reference = committed(&store, b"immutable").await;
    let mut foreign_scope = reference.clone();
    foreign_scope.scope_digest = kiana_domain::json_digest(&serde_json::json!({"foreign": true}));
    let mut changed_hash = reference.clone();
    changed_hash.content_hash = "0".repeat(64);
    let mut changed_schema = reference.clone();
    changed_schema.artifact_schema = "kiana.other-artifact.v1".to_owned();
    let mut changed_identity = reference.clone();
    changed_identity.artifact_id = ArtifactId::new();
    let mut changed_version = reference.clone();
    changed_version.version += 1;
    let mut changed_provenance = reference.clone();
    changed_provenance.provenance.source_run_id = Some(kiana_domain::RunId::new());
    let mut invalid_ref_schema = reference.clone();
    invalid_ref_schema.schema = "kiana.artifact-ref.v99".to_owned();
    for forged in [
        foreign_scope,
        changed_hash,
        changed_schema,
        changed_identity,
        changed_version,
        changed_provenance,
        invalid_ref_schema,
    ] {
        assert!(ArtifactContentPort::read_artifact(&store, &forged).await.is_err());
        assert!(ArtifactStorePort::verify_artifact(&store, &forged).await.is_err());
        assert!(ArtifactStorePort::commit_artifact(&store, forged, None).await.is_err());
    }
    assert_eq!(ArtifactContentPort::read_artifact(&store, &reference).await.unwrap(), b"immutable");
}

#[tokio::test]
async fn local_artifact_duplicate_version_cannot_replace_content_or_manifest() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let original = version(b"original");
    let reference = ArtifactStorePort::stage_artifact(&store, original.clone(), b"original".to_vec())
        .await
        .unwrap();
    ArtifactStorePort::commit_artifact(&store, reference.clone(), Some(1)).await.unwrap();
    let mut replacement = original.clone();
    replacement.content_hash = kiana_domain::journal_sha256(b"replaced");
    let mut changed_manifest = original.clone();
    changed_manifest.created_at_unix_ms += 1;
    for (version, content) in [
        (replacement, b"replaced".to_vec()),
        (changed_manifest, b"original".to_vec()),
    ] {
        assert_eq!(
            ArtifactStorePort::stage_artifact(&store, version, content).await,
            Err(PortError::Conflict("artifact_version_conflict".to_owned()))
        );
    }
    assert_eq!(
        ArtifactStorePort::stage_artifact(&store, original, b"original".to_vec()).await.unwrap(),
        reference
    );
    ArtifactStorePort::commit_artifact(&store, reference.clone(), Some(1)).await.unwrap();
    assert_eq!(ArtifactStorePort::read_artifact(&store, &reference).await.unwrap(), b"original");
}

#[tokio::test]
async fn local_artifact_commit_rejects_missing_or_changed_staged_blob_without_publishing_marker() {
    for remove_blob in [true, false] {
        let fixture = Fixture::new();
        let store = fixture.open().await;
        let version = version(b"immutable");
        let reference = ArtifactStorePort::stage_artifact(&store, version, b"immutable".to_vec())
            .await
            .unwrap();
        let blob = fixture.object_file("blob");
        if remove_blob {
            fs::remove_file(blob).unwrap();
        } else {
            fs::write(blob, b"corrupted").unwrap();
        }
        assert!(ArtifactStorePort::commit_artifact(&store, reference.clone(), None).await.is_err());
        assert_eq!(
            ArtifactContentPort::read_artifact(&store, &reference).await,
            Err(PortError::Unavailable("artifact_version_not_committed".to_owned()))
        );
        assert!(!fs::read_dir(fixture.root.join("artifacts"))
            .unwrap()
            .any(|entry| entry.unwrap().path().extension().is_some_and(|value| value == "commit")));
    }
}

#[tokio::test]
async fn local_artifact_reads_recheck_missing_files_and_blob_corruption_after_commit() {
    for extension in ["blob", "manifest", "commit"] {
        let fixture = Fixture::new();
        let store = fixture.open().await;
        let reference = committed(&store, b"immutable").await;
        fs::remove_file(fixture.object_file(extension)).unwrap();
        assert!(ArtifactContentPort::read_artifact(&store, &reference).await.is_err());
        assert!(ArtifactStorePort::verify_artifact(&store, &reference).await.is_err());
    }
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let reference = committed(&store, b"immutable").await;
    fs::write(fixture.object_file("blob"), b"corrupted").unwrap();
    assert_eq!(
        ArtifactContentPort::read_artifact(&store, &reference).await,
        Err(PortError::Conflict("artifact_content_hash_mismatch".to_owned()))
    );
}

#[tokio::test]
async fn local_artifact_manifest_and_commit_corruption_never_become_readable() {
    for extension in ["manifest", "commit"] {
        for corruption in [b"{".as_slice(), b"{}".as_slice()] {
            let fixture = Fixture::new();
            let store = fixture.open().await;
            let reference = committed(&store, b"immutable").await;
            fs::write(fixture.object_file(extension), corruption).unwrap();
            assert!(ArtifactContentPort::read_artifact(&store, &reference).await.is_err());
            assert!(ArtifactStorePort::commit_artifact(&store, reference, None).await.is_err());
        }
    }
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let reference = committed(&store, b"immutable").await;
    let path = fixture.object_file("manifest");
    let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["version"]["created_at_unix_ms"] = serde_json::json!(101);
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(ArtifactContentPort::read_artifact(&store, &reference).await.is_err());
    assert!(ArtifactStorePort::commit_artifact(&store, reference, None).await.is_err());
}

#[tokio::test]
async fn local_artifact_files_reject_symlinks_directories_fifos_and_hardlinks() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    for extension in ["blob", "manifest", "commit"] {
        for kind in ["symlink", "directory", "fifo", "hardlink"] {
            let fixture = Fixture::new();
            let store = fixture.open().await;
            let reference = committed(&store, b"immutable").await;
            let target = fixture.object_file(extension);
            let outside = fixture.root.join("outside");
            fs::rename(&target, &outside).unwrap();
            match kind {
                "symlink" => symlink(&outside, &target).unwrap(),
                "directory" => fs::create_dir(&target).unwrap(),
                "fifo" => {
                    let name = CString::new(target.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                }
                "hardlink" => fs::hard_link(&outside, &target).unwrap(),
                _ => unreachable!(),
            }
            assert!(ArtifactContentPort::read_artifact(&store, &reference).await.is_err());
            assert!(ArtifactStorePort::commit_artifact(&store, reference, None).await.is_err());
            assert!(fs::symlink_metadata(&target).is_ok());
        }
    }
}

#[tokio::test]
async fn local_artifact_version_remains_reviewable_after_reopen_and_workspace_changes() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let workspace_file = fixture.root.join("workspace.txt");
    fs::write(&workspace_file, b"approved original").unwrap();
    let reference = committed(&store, &fs::read(&workspace_file).unwrap()).await;
    fs::write(&workspace_file, b"later workspace edit").unwrap();
    drop(store);
    let reopened = fixture.open().await;
    assert_eq!(
        ArtifactContentPort::read_artifact(&reopened, &reference).await.unwrap(),
        b"approved original"
    );
    assert_eq!(fs::read(workspace_file).unwrap(), b"later workspace edit");
    ArtifactStorePort::verify_artifact(&reopened, &reference).await.unwrap();
}

#[tokio::test]
async fn local_artifact_staging_can_be_reopened_but_stays_unreadable_until_commit() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let reference = ArtifactStorePort::stage_artifact(&store, version(b"immutable"), b"immutable".to_vec())
        .await
        .unwrap();
    drop(store);
    let reopened = fixture.open().await;
    assert_eq!(
        ArtifactContentPort::read_artifact(&reopened, &reference).await,
        Err(PortError::Unavailable("artifact_version_not_committed".to_owned()))
    );
    ArtifactStorePort::commit_artifact(&reopened, reference.clone(), Some(1)).await.unwrap();
    assert_eq!(ArtifactContentPort::read_artifact(&reopened, &reference).await.unwrap(), b"immutable");
}

#[tokio::test]
async fn local_artifact_store_keeps_the_opened_root_when_its_path_is_replaced() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let store = fixture.open().await;
    let moved = fixture.root.join("opened-artifacts");
    fs::rename(fixture.root.join("artifacts"), &moved).unwrap();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, fixture.root.join("artifacts")).unwrap();
    let reference = committed(&store, b"immutable").await;
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    assert!(LocalArtifactStore::open(fixture.root.join("artifacts")).await.is_err());
    let reopened = LocalArtifactStore::open(moved).await.unwrap();
    assert_eq!(ArtifactContentPort::read_artifact(&reopened, &reference).await.unwrap(), b"immutable");
}

#[tokio::test]
async fn local_artifact_conflicting_writers_cannot_replace_the_winning_version() {
    let fixture = Fixture::new();
    let first = fixture.open().await;
    let second = fixture.open().await;
    let first_version = version(b"first");
    let mut second_version = first_version.clone();
    second_version.content_hash = kiana_domain::journal_sha256(b"other");
    let (left, right) = tokio::join!(
        ArtifactStorePort::stage_artifact(&first, first_version, b"first".to_vec()),
        ArtifactStorePort::stage_artifact(&second, second_version, b"other".to_vec())
    );
    let (reference, expected) = match (left, right) {
        (Ok(reference), Err(_)) => (reference, b"first"),
        (Err(_), Ok(reference)) => (reference, b"other"),
        outcomes => panic!("expected one immutable winner: {outcomes:?}"),
    };
    ArtifactStorePort::commit_artifact(&first, reference.clone(), Some(1)).await.unwrap();
    assert_eq!(ArtifactContentPort::read_artifact(&second, &reference).await.unwrap(), expected);
}
