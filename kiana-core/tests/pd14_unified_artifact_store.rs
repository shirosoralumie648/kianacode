//! PD-14 unified content-addressed artifact store behaviour fixtures.
//!
//! The interface is a promise and the fixtures are the contract. Every rejection below is produced
//! by the store: the fixture builds a *valid* typed version and then perturbs exactly the field the
//! case is about, so a passing test cannot be explained by the fixture refusing its own input.
//!
//! Ordered deny-first: path and scope refusals, claim/digest/bound refusals, uncommitted reads,
//! file-identity and rename races, sync failures — and only then the positive cases (dedup, scope
//! isolation, reopen).

#![cfg(target_os = "linux")]

use kiana_core::{ArtifactSyncFault, UnifiedArtifactLimits, UnifiedArtifactStore};
use kiana_domain::{
    journal_sha256, ArtifactId, ArtifactProvenance, ArtifactRef, ArtifactVersion,
    MAX_ARTIFACT_BYTES,
};
use kiana_ports::{ArtifactStorePort, ContentAddressedArtifactStorePort, PortError};
use std::path::{Path, PathBuf};

const SCOPE_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCOPE_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FOREIGN_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const BLOB_BYTES: &[u8] = b"pd14 bytes that live behind a content hash";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("kiana-pd14-{label}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&root).expect("fixture root");
        Self { root }
    }

    fn artifact_root(&self) -> PathBuf {
        self.root.join("artifacts")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn provenance() -> ArtifactProvenance {
    ArtifactProvenance {
        producer_kind: "test".to_owned(),
        producer_id: "pd14".to_owned(),
        source_event_id: None,
        source_run_id: None,
        recorded_by: "ci".to_owned(),
    }
}

fn version_for(
    artifact_id: ArtifactId,
    scope: &str,
    content: &[u8],
    created_at_unix_ms: u64,
) -> ArtifactVersion {
    ArtifactVersion::new(
        artifact_id,
        1,
        "text/plain",
        content,
        scope,
        provenance(),
        created_at_unix_ms,
    )
    .expect("valid version")
}

fn version(scope: &str, content: &[u8]) -> ArtifactVersion {
    version_for(ArtifactId::new(), scope, content, 1)
}

fn stored_blob_path(root: &Path, content: &[u8]) -> PathBuf {
    root.join("blobs").join(journal_sha256(content))
}

fn stored_manifest_path(root: &Path, reference: &ArtifactRef) -> PathBuf {
    root.join("scopes")
        .join(SCOPE_A.trim_start_matches("sha256:"))
        .join("artifacts")
        .join(reference.artifact_id.to_string())
        .join(format!("v{}.manifest", reference.version))
}

fn stored_commit_path(root: &Path, reference: &ArtifactRef) -> PathBuf {
    root.join("scopes")
        .join(SCOPE_A.trim_start_matches("sha256:"))
        .join("artifacts")
        .join(reference.artifact_id.to_string())
        .join(format!("v{}.commit", reference.version))
}

/// Count files (not directories) under a tree. Used as independent evidence for dedup and for the
/// separation of scope directories, instead of trusting the store's own bookkeeping.
fn file_count(root: &Path) -> usize {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else {
                total += 1;
            }
        }
    }
    total
}

fn directory_count(root: &Path) -> usize {
    std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

fn conflict(reason: &str) -> PortError {
    PortError::Conflict(reason.to_owned())
}

fn denied(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}

fn unavailable(reason: &str) -> PortError {
    PortError::Unavailable(reason.to_owned())
}

async fn committed(store: &UnifiedArtifactStore, content: &[u8]) -> ArtifactRef {
    let manifest = store
        .stage_content(version(SCOPE_A, content), content.to_vec())
        .await
        .expect("stage");
    store
        .commit_content(&manifest.version.as_ref())
        .await
        .expect("commit");
    manifest.version.as_ref()
}

#[tokio::test]
async fn open_refuses_relative_roots_traversal_symlinks_scope_and_limits() {
    let fixture = Fixture::new("open");
    assert_eq!(
        UnifiedArtifactStore::open("relative/artifacts", SCOPE_A)
            .await
            .unwrap_err(),
        denied("artifact_root_must_be_absolute")
    );
    assert_eq!(
        UnifiedArtifactStore::open(fixture.artifact_root(), "not-a-digest")
            .await
            .unwrap_err(),
        denied("artifact_owner_scope_invalid")
    );
    assert_eq!(
        UnifiedArtifactStore::open_with(
            fixture.artifact_root(),
            SCOPE_A,
            UnifiedArtifactLimits {
                max_artifact_bytes: 0,
                max_manifest_bytes: 1024,
            },
            ArtifactSyncFault::None,
        )
        .await
        .unwrap_err(),
        denied("artifact_store_limits_invalid")
    );

    // A symlinked component on the way to the root is refused while the descriptor is opened, not
    // after a later operation has already written through it.
    let target = fixture.root.join("real");
    std::fs::create_dir_all(&target).expect("real directory");
    let link = fixture.root.join("linked");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let error = UnifiedArtifactStore::open(link.join("artifacts"), SCOPE_A)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, PortError::Failed(reason) if reason.starts_with("artifact_directory_open_failed")),
        "unexpected symlink refusal: {error:?}"
    );

    // `..` is refused rather than resolved, so a store cannot be opened through traversal.
    assert_eq!(
        UnifiedArtifactStore::open(
            fixture.root.join("nested").join("..").join("artifacts"),
            SCOPE_A,
        )
        .await
        .unwrap_err(),
        denied("artifact_root_component_invalid")
    );
}

#[tokio::test]
async fn stage_refuses_size_digest_bound_scope_and_version_drift_before_writing() {
    let fixture = Fixture::new("stage");
    let store = UnifiedArtifactStore::open(fixture.artifact_root(), SCOPE_A)
        .await
        .expect("store");
    let content = BLOB_BYTES.to_vec();

    let mut wrong_size = version(SCOPE_A, &content);
    wrong_size.size_bytes += 1;
    assert_eq!(
        store
            .stage_content(wrong_size, content.clone())
            .await
            .unwrap_err(),
        conflict("artifact_content_size_mismatch")
    );

    let mut wrong_digest = version(SCOPE_A, &content);
    wrong_digest.content_hash = FOREIGN_DIGEST.to_owned();
    assert_eq!(
        store
            .stage_content(wrong_digest, content.clone())
            .await
            .unwrap_err(),
        conflict("artifact_content_hash_mismatch")
    );

    let mut not_immutable = version(SCOPE_A, &content);
    not_immutable.immutable = false;
    assert!(
        matches!(
            store
                .stage_content(not_immutable, content.clone())
                .await
                .unwrap_err(),
            PortError::Failed(reason) if reason.starts_with("artifact_version_invalid")
        ),
        "an invalid typed version must be refused before any write"
    );

    // The bound is the store's own, not the domain maximum: a small bound refuses small content,
    // and nothing is written.
    let bounded_root = fixture.root.join("bounded");
    let bounded = UnifiedArtifactStore::open_with(
        &bounded_root,
        SCOPE_A,
        UnifiedArtifactLimits {
            max_artifact_bytes: 8,
            max_manifest_bytes: 1024,
        },
        ArtifactSyncFault::None,
    )
    .await
    .expect("bounded store");
    assert_eq!(bounded.max_artifact_bytes(), 8);
    let too_large = b"nine byte".to_vec();
    assert_eq!(
        bounded
            .stage_content(version(SCOPE_A, &too_large), too_large.clone())
            .await
            .unwrap_err(),
        conflict("artifact_content_too_large")
    );
    assert!(!bounded
        .blob_present(&journal_sha256(&too_large))
        .await
        .expect("blob probe"));
    assert_eq!(file_count(&bounded_root.join("blobs")), 0);

    // A version that belongs to another scope is refused as not-owned, and no blob appears.
    let foreign = version(SCOPE_B, &content);
    assert_eq!(
        store
            .stage_content(foreign.clone(), content.clone())
            .await
            .unwrap_err(),
        conflict("artifact_scope_not_owned")
    );
    assert!(!store
        .blob_present(&foreign.content_hash)
        .await
        .expect("blob probe"));
}

#[tokio::test]
async fn staged_content_is_unreadable_until_a_matching_commit() {
    let fixture = Fixture::new("commit");
    let store = UnifiedArtifactStore::open(fixture.artifact_root(), SCOPE_A)
        .await
        .expect("store");
    let content = BLOB_BYTES.to_vec();
    let manifest = store
        .stage_content(version(SCOPE_A, &content), content.clone())
        .await
        .expect("stage");
    let reference = manifest.version.as_ref();

    assert_eq!(
        store.read_manifest(&reference).await.unwrap_err(),
        unavailable("artifact_version_not_committed")
    );
    assert_eq!(
        store.read_content(&reference).await.unwrap_err(),
        unavailable("artifact_version_not_committed")
    );
    assert_eq!(
        store.verify_content(&reference).await.unwrap_err(),
        unavailable("artifact_version_not_committed")
    );

    let mut foreign_scope = reference.clone();
    foreign_scope.scope_digest = SCOPE_B.to_owned();
    assert_eq!(
        store.commit_content(&foreign_scope).await.unwrap_err(),
        conflict("artifact_scope_not_owned")
    );

    let mut drifted_reference = reference.clone();
    drifted_reference.content_hash = FOREIGN_DIGEST.to_owned();
    assert_eq!(
        store.commit_content(&drifted_reference).await.unwrap_err(),
        conflict("artifact_reference_manifest_mismatch")
    );

    // A later version of the same artifact has no manifest of its own yet.
    let mut next_version = reference.clone();
    next_version.version += 1;
    assert_eq!(
        store.read_manifest(&next_version).await.unwrap_err(),
        unavailable("artifact_manifest_missing")
    );

    let committed = store
        .commit_content(&reference)
        .await
        .expect("commit is allowed after staging");
    assert_eq!(committed, manifest);
    assert!(committed.blob_names_version_content());
    // Committing again is idempotent: the same marker, the same manifest.
    assert_eq!(
        store.commit_content(&reference).await.expect("idempotent"),
        manifest
    );
    assert_eq!(
        store.read_manifest(&reference).await.expect("read"),
        manifest
    );
    assert_eq!(store.read_content(&reference).await.expect("read"), content);
    assert_eq!(
        store.verify_content(&reference).await.expect("verify"),
        manifest
    );
    assert!(stored_commit_path(fixture.artifact_root().as_path(), &reference).is_file());
}

#[tokio::test]
async fn identical_bytes_are_deduplicated_but_scopes_never_merge() {
    let fixture = Fixture::new("dedup");
    let root = fixture.artifact_root();
    let content = BLOB_BYTES.to_vec();
    let digest = journal_sha256(&content);

    let store_a = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("store a");
    let first = committed(&store_a, &content).await;
    let second = committed(&store_a, &content).await;
    assert_ne!(first.artifact_id, second.artifact_id);
    assert_eq!(store_a.read_content(&first).await.expect("read"), content);
    assert_eq!(store_a.read_content(&second).await.expect("read"), content);
    assert!(store_a.blob_present(&digest).await.expect("probe"));
    assert_eq!(file_count(&root.join("blobs")), 1);
    assert_eq!(file_count(&root.join("scopes")), 4);

    // A second scope over the same root reuses the same blob bytes and keeps its own manifest.
    let store_b = UnifiedArtifactStore::open(&root, SCOPE_B)
        .await
        .expect("store b");
    let scoped = committed(&store_b, &content).await;
    assert_eq!(store_b.read_content(&scoped).await.expect("read"), content);
    assert_eq!(file_count(&root.join("blobs")), 1);
    assert_eq!(file_count(&root.join("scopes")), 6);
    assert_eq!(directory_count(&root.join("scopes")), 2);

    // Neither scope can resolve the other's reference, even though the bytes are identical.
    assert_eq!(
        store_b.read_manifest(&first).await.unwrap_err(),
        conflict("artifact_scope_not_owned")
    );
    assert_eq!(
        store_a.read_content(&scoped).await.unwrap_err(),
        conflict("artifact_scope_not_owned")
    );
    assert_eq!(
        store_b.verify_content(&second).await.unwrap_err(),
        conflict("artifact_scope_not_owned")
    );
}

#[tokio::test]
async fn symlinked_hardlinked_and_non_regular_files_are_refused() {
    let fixture = Fixture::new("identity");
    let root = fixture.artifact_root();
    let content = BLOB_BYTES.to_vec();

    // Symlinked blob.
    let store = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("store");
    let reference = committed(&store, &content).await;
    let blob = stored_blob_path(&root, &content);
    let elsewhere = fixture.root.join("elsewhere.blob");
    std::fs::write(&elsewhere, &content).expect("write decoy");
    std::fs::remove_file(&blob).expect("remove blob");
    std::os::unix::fs::symlink(&elsewhere, &blob).expect("symlink blob");
    assert_eq!(
        store.read_content(&reference).await.unwrap_err(),
        denied("artifact_file_symlinked")
    );
    assert_eq!(
        store.verify_content(&reference).await.unwrap_err(),
        denied("artifact_file_symlinked")
    );

    // Hard-linked blob: the inode has a second name, so it is no longer the object we published.
    std::fs::remove_file(&blob).expect("remove symlink");
    std::fs::write(&blob, &content).expect("restore blob");
    std::fs::hard_link(&blob, fixture.root.join("blob.alias")).expect("hard link");
    assert_eq!(
        store.read_content(&reference).await.unwrap_err(),
        denied("artifact_file_hardlinked")
    );

    // Non-regular file standing in for the blob.
    std::fs::remove_file(&blob).expect("remove hard link");
    std::fs::remove_file(fixture.root.join("blob.alias")).expect("remove alias");
    std::fs::create_dir(&blob).expect("directory at blob path");
    assert_eq!(
        store.read_content(&reference).await.unwrap_err(),
        denied("artifact_file_not_regular")
    );

    // The manifest is held to the same standard: a symlinked manifest is refused.
    std::fs::remove_dir(&blob).expect("remove directory");
    std::fs::write(&blob, &content).expect("restore blob");
    let manifest_path = stored_manifest_path(&root, &reference);
    let manifest_copy = fixture.root.join("manifest.copy");
    std::fs::copy(&manifest_path, &manifest_copy).expect("copy manifest");
    std::fs::remove_file(&manifest_path).expect("remove manifest");
    std::os::unix::fs::symlink(&manifest_copy, &manifest_path).expect("symlink manifest");
    assert_eq!(
        store.read_manifest(&reference).await.unwrap_err(),
        denied("artifact_file_symlinked")
    );
}

#[tokio::test]
async fn a_replaced_root_path_cannot_redirect_an_open_store() {
    let fixture = Fixture::new("root-swap");
    let root = fixture.artifact_root();
    let content = BLOB_BYTES.to_vec();
    let store = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("store");
    let reference = committed(&store, &content).await;

    // Move the real root away and put an empty directory at the old path. The handle was pinned at
    // open time, so reads still resolve against the original directory.
    let moved = fixture.root.join("moved-artifacts");
    std::fs::rename(&root, &moved).expect("move root");
    std::fs::create_dir_all(&root).expect("fresh root");
    assert_eq!(store.read_content(&reference).await.expect("read"), content);

    // And a store opened at the new, empty path knows nothing about the old version.
    let fresh = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("fresh store");
    assert_eq!(
        fresh.read_manifest(&reference).await.unwrap_err(),
        unavailable("artifact_version_not_committed")
    );
}

#[tokio::test]
async fn pre_existing_blob_drift_and_conflicting_writers_fail_closed() {
    let fixture = Fixture::new("race");
    let root = fixture.artifact_root();
    std::fs::create_dir_all(root.join("blobs")).expect("blobs dir");
    let content = BLOB_BYTES.to_vec();

    // A blob already sitting at the content-addressed name with different bytes: reuse is refused
    // because the file does not hash to its own name.
    let blob = stored_blob_path(&root, &content);
    std::fs::write(&blob, b"not the bytes named by this path").expect("plant blob");
    let store = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("store");
    assert_eq!(
        store
            .stage_content(version(SCOPE_A, &content), content.clone())
            .await
            .unwrap_err(),
        conflict("artifact_content_hash_mismatch")
    );

    // Two writers race for the same (artifact, version) with different content. Exactly one wins;
    // the loser cannot replace the winning manifest.
    let contested_id = ArtifactId::new();
    let winner_bytes = b"winner bytes".to_vec();
    let loser_bytes = b"loser bytes".to_vec();
    let winner_version = version_for(contested_id, SCOPE_A, &winner_bytes, 7);
    let loser_version = version_for(contested_id, SCOPE_A, &loser_bytes, 8);
    let winner_ref = winner_version.as_ref();
    let loser_ref = loser_version.as_ref();
    let (first, second) = tokio::join!(
        store.stage_content(winner_version, winner_bytes.clone()),
        store.stage_content(loser_version, loser_bytes.clone()),
    );
    let successes = usize::from(first.is_ok()) + usize::from(second.is_ok());
    assert_eq!(
        successes, 1,
        "exactly one writer may publish: first={first:?} second={second:?}"
    );
    let staged = first.or(second).expect("one staged manifest");
    assert_eq!(
        staged.version.as_ref().artifact_id,
        contested_id,
        "the staged manifest must describe the contested version"
    );
    // The winning manifest names the bytes its own writer proposed. Keep that mapping so the read
    // back is compared against real bytes, and so the loser can be identified from the winner.
    let (staged_bytes, losing) = if staged.version.as_ref() == winner_ref {
        (winner_bytes, &loser_ref)
    } else {
        (loser_bytes, &winner_ref)
    };
    store
        .commit_content(&staged.version.as_ref())
        .await
        .expect("commit winner");
    assert_eq!(
        store
            .read_content(&staged.version.as_ref())
            .await
            .expect("read winner"),
        staged_bytes,
        "the readable bytes are the ones the winning manifest names"
    );
    // The writer that lost may not read its own proposed reference afterwards.
    assert_eq!(
        store.read_content(losing).await.unwrap_err(),
        conflict("artifact_reference_manifest_mismatch")
    );

    // Sequentially, a second proposal for an already staged version is a conflict and the stored
    // bytes stay the first writer's.
    let settled_root = fixture.root.join("settled");
    let settled = UnifiedArtifactStore::open(&settled_root, SCOPE_A)
        .await
        .expect("settled store");
    let first_bytes = b"first bytes".to_vec();
    let second_bytes = b"second bytes".to_vec();
    let settled_id = ArtifactId::new();
    let first_manifest = settled
        .stage_content(
            version_for(settled_id, SCOPE_A, &first_bytes, 1),
            first_bytes.clone(),
        )
        .await
        .expect("stage first");
    assert_eq!(
        settled
            .stage_content(
                version_for(settled_id, SCOPE_A, &second_bytes, 2),
                second_bytes.clone(),
            )
            .await
            .unwrap_err(),
        conflict("artifact_version_conflict")
    );
    assert_eq!(
        settled
            .read_content(&first_manifest.version.as_ref())
            .await
            .unwrap_err(),
        unavailable("artifact_version_not_committed")
    );
    settled
        .commit_content(&first_manifest.version.as_ref())
        .await
        .expect("commit first");
    assert_eq!(
        settled
            .read_content(&first_manifest.version.as_ref())
            .await
            .expect("read first"),
        first_bytes
    );
}

#[tokio::test]
async fn file_sync_failure_denies_the_stage_and_publishes_nothing() {
    let fixture = Fixture::new("file-sync");
    let root = fixture.artifact_root();
    let content = BLOB_BYTES.to_vec();
    let faulted = UnifiedArtifactStore::open_with(
        &root,
        SCOPE_A,
        UnifiedArtifactLimits::default(),
        ArtifactSyncFault::FileSync,
    )
    .await
    .expect("faulted store");

    let proposed = version(SCOPE_A, &content);
    let reference = proposed.as_ref();
    assert_eq!(
        faulted
            .stage_content(proposed, content.clone())
            .await
            .unwrap_err(),
        denied("artifact_sync_failed")
    );
    assert_eq!(
        faulted.commit_content(&reference).await.unwrap_err(),
        unavailable("artifact_version_not_staged")
    );
    assert_eq!(
        faulted.read_manifest(&reference).await.unwrap_err(),
        unavailable("artifact_version_not_committed")
    );

    // The same immutable object can still be published once the sync succeeds.
    let healthy = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("healthy store");
    let manifest = healthy
        .stage_content(reference_proposal(&reference, &content), content.clone())
        .await
        .expect("stage after recovery");
    healthy
        .commit_content(&manifest.version.as_ref())
        .await
        .expect("commit after recovery");
    assert_eq!(
        healthy
            .read_content(&manifest.version.as_ref())
            .await
            .expect("read"),
        content
    );
}

#[tokio::test]
async fn directory_sync_failure_is_reported_as_unknown_and_the_retry_is_idempotent() {
    let fixture = Fixture::new("dir-sync");
    let root = fixture.artifact_root();
    let content = BLOB_BYTES.to_vec();
    let faulted = UnifiedArtifactStore::open_with(
        &root,
        SCOPE_A,
        UnifiedArtifactLimits::default(),
        ArtifactSyncFault::DirectorySync,
    )
    .await
    .expect("faulted store");

    let proposed = version(SCOPE_A, &content);
    let reference = proposed.as_ref();
    let error = faulted
        .stage_content(proposed, content.clone())
        .await
        .unwrap_err();
    assert!(
        matches!(&error, PortError::Failed(reason) if reason.starts_with("result_unknown:artifact_directory_sync_failed")),
        "an unconfirmed directory sync must be reported as unknown: {error:?}"
    );
    // The manifest was never published, so the version is still not readable.
    assert_eq!(
        faulted.read_manifest(&reference).await.unwrap_err(),
        unavailable("artifact_version_not_committed")
    );

    let healthy = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("healthy store");
    let manifest = healthy
        .stage_content(reference_proposal(&reference, &content), content.clone())
        .await
        .expect("stage retry");
    let committed = healthy
        .commit_content(&manifest.version.as_ref())
        .await
        .expect("commit retry");
    assert_eq!(committed, manifest, "a retry must keep the first manifest");

    // A commit whose directory sync cannot be confirmed is reported as unknown rather than as a
    // durable success; the retry returns the identical manifest, which is what makes it idempotent.
    let second_bytes = b"second artifact bytes".to_vec();
    let second = healthy
        .stage_content(version(SCOPE_A, &second_bytes), second_bytes.clone())
        .await
        .expect("stage second");
    let second_reference = second.version.as_ref();
    let faulted_commit = faulted.commit_content(&second_reference).await.unwrap_err();
    assert!(
        matches!(&faulted_commit, PortError::Failed(reason) if reason.starts_with("result_unknown:artifact_directory_sync_failed")),
        "unconfirmed commit sync must not look durable: {faulted_commit:?}"
    );
    assert_eq!(
        healthy
            .commit_content(&second_reference)
            .await
            .expect("commit retry"),
        second
    );
    assert_eq!(
        healthy.read_content(&second_reference).await.expect("read"),
        second_bytes
    );
}

/// Rebuild the same typed version the fixture proposed, so a retry carries identical bytes without
/// reusing a value that the faulted call may have consumed.
fn reference_proposal(reference: &ArtifactRef, content: &[u8]) -> ArtifactVersion {
    let mut version = version_for(reference.artifact_id, &reference.scope_digest, content, 3);
    version.version = reference.version;
    version.artifact_schema = reference.artifact_schema.clone();
    version.provenance = reference.provenance.clone();
    assert_eq!(
        version.as_ref(),
        *reference,
        "the retry must rebuild exactly the typed version the fixture proposed"
    );
    version
}

#[tokio::test]
async fn legacy_artifact_port_and_manifest_surface_agree() {
    let fixture = Fixture::new("ports");
    let root = fixture.artifact_root();
    let content = BLOB_BYTES.to_vec();
    let store = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("store");
    assert_eq!(
        store.owner_scope_digest(),
        format!("sha256:{}", SCOPE_A.trim_start_matches("sha256:"))
    );
    assert_eq!(store.max_artifact_bytes(), MAX_ARTIFACT_BYTES);

    let proposal = version(SCOPE_A, &content);
    let reference = ArtifactStorePort::stage_artifact(&store, proposal.clone(), content.clone())
        .await
        .expect("stage through the legacy port");
    assert_eq!(reference, proposal.as_ref());
    // The legacy staging contract has no way to return a previously held version, so a second stage
    // of the same version is a conflict there.
    assert_eq!(
        ArtifactStorePort::stage_artifact(&store, proposal.clone(), content.clone())
            .await
            .unwrap_err(),
        conflict("artifact_version_already_staged")
    );
    assert_eq!(
        ArtifactStorePort::commit_artifact(&store, reference.clone(), Some(reference.version + 1))
            .await
            .unwrap_err(),
        conflict("artifact_manifest_revision_conflict")
    );
    ArtifactStorePort::commit_artifact(&store, reference.clone(), Some(reference.version))
        .await
        .expect("commit");
    assert_eq!(
        ArtifactStorePort::read_artifact(&store, &reference)
            .await
            .expect("read"),
        content
    );
    ArtifactStorePort::verify_artifact(&store, &reference)
        .await
        .expect("verify");

    // The same bytes staged through the manifest surface resolve to one manifest and one blob.
    let manifest = store.read_manifest(&reference).await.expect("manifest");
    assert!(manifest.blob_names_version_content());
    assert_eq!(file_count(&root.join("blobs")), 1);
    let retry = store
        .stage_content(proposal.clone(), content.clone())
        .await
        .expect("idempotent retry");
    assert_eq!(retry, manifest, "a retry keeps the first manifest");

    assert_eq!(
        ArtifactStorePort::invalidate_artifact(&store, &reference, 1, 2, FOREIGN_DIGEST, 1)
            .await
            .unwrap_err(),
        unavailable("artifact_invalidation_unsupported")
    );
    assert_eq!(
        store.blob_present("not-a-digest").await.unwrap_err(),
        denied("artifact_content_hash_invalid")
    );
}

#[tokio::test]
async fn reopen_preserves_the_published_manifest() {
    let fixture = Fixture::new("reopen");
    let root = fixture.artifact_root();
    let content = BLOB_BYTES.to_vec();
    let published = {
        let store = UnifiedArtifactStore::open(&root, SCOPE_A)
            .await
            .expect("store");
        let manifest = store
            .stage_content(version(SCOPE_A, &content), content.clone())
            .await
            .expect("stage");
        store
            .commit_content(&manifest.version.as_ref())
            .await
            .expect("commit");
        manifest
    };

    let reopened = UnifiedArtifactStore::open(&root, SCOPE_A)
        .await
        .expect("reopen");
    let reference = published.version.as_ref();
    assert_eq!(
        reopened.read_manifest(&reference).await.expect("manifest"),
        published
    );
    assert_eq!(
        reopened.verify_content(&reference).await.expect("verify"),
        published
    );
    assert_eq!(
        reopened.read_content(&reference).await.expect("read"),
        content
    );
    assert!(reopened
        .blob_present(&journal_sha256(&content))
        .await
        .expect("probe"));
    // The manifest that staging would have proposed a second time must not be written over the
    // published one, so the digest a reader compares is stable across restarts. The retry proposes
    // a different creation timestamp and must still resolve to the first published manifest.
    let retried = reopened
        .stage_content(
            version_for(reference.artifact_id, SCOPE_A, &content, 64),
            content.clone(),
        )
        .await
        .expect("retry after reopen");
    assert_eq!(retried.manifest_digest, published.manifest_digest);
    assert_eq!(retried, published);
}
