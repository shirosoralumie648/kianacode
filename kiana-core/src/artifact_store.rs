//! PD-14 unified content-addressed artifact store.
//!
//! Everything the ledger knows about an artifact lives in an event: identity, provenance, content
//! hash and scope. The bytes live here. This module is the storage side of that split — the one
//! implementation that gives both artifact port surfaces the same answers:
//!
//! ```text
//! <root>/blobs/<content-hash hex>                                   one file per unique byte content
//! <root>/scopes/<owner-scope hex>/artifacts/<artifact id>/v<n>.manifest
//! <root>/scopes/<owner-scope hex>/artifacts/<artifact id>/v<n>.commit
//! ```
//!
//! Four decisions shape it.
//!
//! **Bytes are addressed by content, manifests by identity.** A blob is named by the digest of its
//! own bytes, so two artifacts — in the same scope or in different scopes — that are staged with
//! identical bytes share one file. What they do *not* share is a manifest: the manifest is keyed by
//! `(owner scope, artifact, version)`, repeats the whole typed version, and is what a reader
//! rechecks. Deduplicating bytes can therefore never merge two artifacts into one readable object.
//!
//! **A handle is bound to one owner scope.** `open` takes the scope digest the store serves, and
//! every version or reference carrying another scope is refused with `artifact_scope_not_owned`
//! before any byte is written. Two scopes may share a root directory; they cannot read each other's
//! versions, even when the bytes are identical.
//!
//! **Publishing is atomic or it fails.** A blob or manifest is written to a
//! same-directory temporary file, flushed, and moved into place with a no-replace rename, then the
//! directory is synced. A target that already exists is only accepted when its bytes are identical;
//! anything else is a conflict, never an overwrite. If a sync step cannot be confirmed the call
//! reports failure instead of a durable write — see [`ArtifactSyncFault`], which exists so the
//! denial path is *provable* in a fixture rather than assumed.
//!
//! **Nothing is resolved through a path.** The root is opened once and retained by descriptor;
//! every later operation is `openat`/`renameat2`-relative to that descriptor with `O_NOFOLLOW`, and
//! blob and manifest files must be single-link regular files. A symlinked path component, a
//! hard-linked blob, a directory or fifo standing in for a file, and a root whose path is later
//! replaced are all refused rather than followed.
//!
//! # What this store is not
//!
//! It is not an authority source: it does not read an event, decide admission or interpret a
//! payload. It also does not claim power-loss-proof durability — the descriptor, rename and sync
//! acknowledgements recorded here are `proof_level=source` until a crash-recovery fixture proves
//! more. Retention, deletion fencing and physical erasure stay with their own steps, so
//! `invalidate_artifact` is deliberately **not** implemented: the legacy port reports it as
//! unsupported instead of pretending bytes were erased.

use async_trait::async_trait;
use kiana_domain::{ArtifactRef, ArtifactVersion, MAX_ARTIFACT_BYTES};
use kiana_ports::{
    ArtifactManifest, ArtifactStorePort, ContentAddressedArtifactStorePort, PortError,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Directory holding one file per unique byte content.
const BLOBS_DIR: &str = "blobs";
/// Directory holding one sub-directory per owner scope.
const SCOPES_DIR: &str = "scopes";
/// Directory holding one sub-directory per artifact inside a scope.
const ARTIFACTS_DIR: &str = "artifacts";
/// Schema tag of the commit marker that turns a staged manifest into a readable version.
const COMMIT_MARKER_SCHEMA: &str = "kiana.unified-artifact-commit.v1";
/// Default bound for a manifest, staged commit marker or re-read marker file.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}

/// Normalize a digest to bare lowercase hex. Both spellings the domain allows
/// (`<64 hex>` and `sha256:<64 hex>`) parse; anything else is `None` and the caller decides
/// whether that is an invalid input or a failed comparison.
fn normalize_digest(value: &str) -> Option<String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    (hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

/// Bounds a store enforces on itself. They are visible through the port so a caller can refuse
/// before staging instead of discovering the bound halfway through a write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnifiedArtifactLimits {
    pub max_artifact_bytes: usize,
    pub max_manifest_bytes: usize,
}

impl Default for UnifiedArtifactLimits {
    fn default() -> Self {
        Self {
            max_artifact_bytes: MAX_ARTIFACT_BYTES,
            max_manifest_bytes: MAX_MANIFEST_BYTES,
        }
    }
}

/// Injectable sync failure.
///
/// "fsync failure is denied" is a claim about *ordering*: bytes, renames and directory entries are
/// acknowledged only after the corresponding sync returned. Claiming that with a comment is what
/// this repository keeps punishing, so the failure is a value a fixture can set. The store then
/// takes the same code path a real `fsync` error would: report the failure, keep nothing readable,
/// and let a later call that succeeds publish the same immutable object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactSyncFault {
    /// No injected failure.
    None,
    /// The temporary file's sync fails, so nothing may be published.
    FileSync,
    /// The directory sync after a rename fails, so the result is reported as unknown.
    DirectorySync,
}

impl Default for ArtifactSyncFault {
    fn default() -> Self {
        Self::None
    }
}

/// Marker written after a manifest is complete. It pins the digest of the manifest *bytes* that
/// were staged, so a manifest that is edited or truncated after staging stops matching the commit.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CommitMarker {
    schema: String,
    manifest_sha256: String,
}

/// Unified content-addressed artifact store over an explicitly injected absolute root.
///
/// The root is opened once, by descriptor, walking every component with `O_NOFOLLOW`; later
/// workspace changes cannot redirect this handle, and no `.kiana` or project path is inferred. The
/// store holds no authorization state and performs no content-sensitive interpretation: it stores
/// the exact opaque bytes it was handed and re-verifies them against the typed version on every
/// read.
#[derive(Clone, Debug)]
pub struct UnifiedArtifactStore {
    root: Arc<platform::PinnedDir>,
    owner_scope_hex: Arc<str>,
    limits: UnifiedArtifactLimits,
    sync_fault: ArtifactSyncFault,
}

impl UnifiedArtifactStore {
    /// Open (creating it when missing) an absolute artifact root for one owner scope.
    pub async fn open(
        root: impl Into<PathBuf>,
        owner_scope_digest: impl Into<String>,
    ) -> Result<Self, PortError> {
        Self::open_with(
            root,
            owner_scope_digest,
            UnifiedArtifactLimits::default(),
            ArtifactSyncFault::None,
        )
        .await
    }

    /// Open a store with explicit bounds and an optional injected sync failure.
    pub async fn open_with(
        root: impl Into<PathBuf>,
        owner_scope_digest: impl Into<String>,
        limits: UnifiedArtifactLimits,
        sync_fault: ArtifactSyncFault,
    ) -> Result<Self, PortError> {
        let root = root.into();
        let owner_scope_digest = owner_scope_digest.into();
        let Some(owner_scope_hex) = normalize_digest(&owner_scope_digest) else {
            return Err(failed("artifact_owner_scope_invalid"));
        };
        if limits.max_artifact_bytes == 0 || limits.max_manifest_bytes == 0 {
            return Err(failed("artifact_store_limits_invalid"));
        }
        let opened = tokio::task::spawn_blocking(move || platform::PinnedDir::open(&root, true))
            .await
            .map_err(|error| failed(format!("artifact_store_open_join_failed:{error}")))??;
        Ok(Self {
            root: Arc::new(opened),
            owner_scope_hex: Arc::from(owner_scope_hex.as_str()),
            limits,
            sync_fault,
        })
    }

    /// Run one blocking filesystem operation on a task that is allowed to block.
    async fn blocking<T, F>(&self, operation: F) -> Result<T, PortError>
    where
        T: Send + 'static,
        F: FnOnce(Self) -> Result<T, PortError> + Send + 'static,
    {
        let store = self.clone();
        tokio::task::spawn_blocking(move || operation(store))
            .await
            .map_err(|error| failed(format!("result_unknown:artifact_store_join_failed:{error}")))?
    }

    /// Whether a scope digest names the owner scope of this handle. Both accepted spellings
    /// compare equal, so a caller writing `sha256:` and a caller writing bare hex see one scope.
    fn owns_scope(&self, scope_digest: &str) -> bool {
        normalize_digest(scope_digest).as_deref() == Some(self.owner_scope_hex.as_ref())
    }

    fn validate_stage(&self, version: &ArtifactVersion, content: &[u8]) -> Result<(), PortError> {
        version
            .validate()
            .map_err(|error| failed(format!("artifact_version_invalid:{error}")))?;
        if !self.owns_scope(&version.scope_digest) {
            return Err(PortError::Conflict("artifact_scope_not_owned".to_owned()));
        }
        if content.len() > self.limits.max_artifact_bytes {
            return Err(PortError::Conflict("artifact_content_too_large".to_owned()));
        }
        let Some(content_hex) = normalize_digest(&version.content_hash) else {
            return Err(failed("artifact_content_hash_invalid"));
        };
        // The claim and the bytes must agree. The digest is recomputed here instead of trusting the
        // caller's `content_hash`, and the size is checked separately so a truncated or padded blob
        // cannot pass by hashing the same prefix.
        if version.size_bytes != content.len() as u64 {
            return Err(PortError::Conflict(
                "artifact_content_size_mismatch".to_owned(),
            ));
        }
        if kiana_domain::journal_sha256(content) != content_hex {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_reference(&self, reference: &ArtifactRef) -> Result<(), PortError> {
        reference
            .validate()
            .map_err(|error| failed(format!("artifact_reference_invalid:{error}")))?;
        if !self.owns_scope(&reference.scope_digest) {
            return Err(PortError::Conflict("artifact_scope_not_owned".to_owned()));
        }
        Ok(())
    }

    fn scope_dir_name(&self) -> String {
        self.owner_scope_hex.to_string()
    }

    /// Open the manifest directory of one artifact, creating the scope layout when asked.
    fn version_dir(&self, reference: &ArtifactRef) -> Result<platform::PinnedDir, PortError> {
        let scopes = self.root.subdir(SCOPES_DIR, true)?;
        let scope = scopes.subdir(&self.scope_dir_name(), true)?;
        let artifacts = scope.subdir(ARTIFACTS_DIR, true)?;
        artifacts.subdir(&reference.artifact_id.to_string(), true)
    }

    /// Open the manifest directory of one artifact without creating anything.
    fn version_dir_optional(
        &self,
        reference: &ArtifactRef,
    ) -> Result<Option<platform::PinnedDir>, PortError> {
        let Some(scopes) = self.root.subdir_optional(SCOPES_DIR)? else {
            return Ok(None);
        };
        let Some(scope) = scopes.subdir_optional(&self.scope_dir_name())? else {
            return Ok(None);
        };
        let Some(artifacts) = scope.subdir_optional(ARTIFACTS_DIR)? else {
            return Ok(None);
        };
        artifacts.subdir_optional(&reference.artifact_id.to_string())
    }

    fn decode_manifest(bytes: &[u8]) -> Result<ArtifactManifest, PortError> {
        let manifest: ArtifactManifest = serde_json::from_slice(bytes)
            .map_err(|error| failed(format!("artifact_manifest_invalid:{error}")))?;
        manifest.validate().map_err(PortError::Failed)?;
        Ok(manifest)
    }

    /// Read the staged manifest of one artifact version and require it to describe exactly the
    /// reference the caller holds.
    fn read_staged(
        &self,
        directory: &platform::PinnedDir,
        reference: &ArtifactRef,
    ) -> Result<(ArtifactManifest, Vec<u8>), PortError> {
        let name = manifest_name(reference);
        let Some(bytes) = directory.read_regular(
            &name,
            self.limits.max_manifest_bytes,
            "artifact_manifest_size_exceeded",
        )?
        else {
            return Err(PortError::Unavailable(
                "artifact_manifest_missing".to_owned(),
            ));
        };
        let manifest = Self::decode_manifest(&bytes)?;
        if manifest.version.as_ref() != *reference {
            return Err(PortError::Conflict(
                "artifact_reference_manifest_mismatch".to_owned(),
            ));
        }
        Ok((manifest, bytes))
    }

    /// Read the blob a manifest names, re-deriving its digest instead of trusting the file name or
    /// the manifest field.
    fn read_blob(&self, manifest: &ArtifactManifest) -> Result<Vec<u8>, PortError> {
        let Some(blob_hex) = normalize_digest(&manifest.blob_digest) else {
            return Err(failed("artifact_manifest_blob_digest_invalid"));
        };
        let Some(blobs) = self.root.subdir_optional(BLOBS_DIR)? else {
            return Err(PortError::Unavailable("artifact_blob_missing".to_owned()));
        };
        let Some(content) = blobs.read_regular(
            &blob_hex,
            self.limits.max_artifact_bytes,
            "artifact_content_too_large",
        )?
        else {
            return Err(PortError::Unavailable("artifact_blob_missing".to_owned()));
        };
        if content.len() as u64 != manifest.version.size_bytes
            || kiana_domain::journal_sha256(&content) != blob_hex
        {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(content)
    }

    /// Publish one immutable file, or accept an existing identical one.
    ///
    /// The order is the guarantee: write the temporary file, sync it, move it into place without
    /// replacing anything, then sync the directory. Every step that cannot be confirmed returns an
    /// error; the conflict path re-reads the existing entry and accepts it only byte-for-byte.
    /// A temporary file left behind by a failed call is inert: readers open exact names, so a
    /// `.kiana-artifact-*` leftover can never be mistaken for a blob or a manifest.
    fn publish_immutable(
        &self,
        directory: &platform::PinnedDir,
        name: &str,
        bytes: &[u8],
        limit: usize,
        conflict: &str,
    ) -> Result<(), PortError> {
        use std::io::Write;

        let (temporary, mut file) = directory.create_temp()?;
        let staged = (|| -> Result<(), PortError> {
            file.write_all(bytes)
                .map_err(|error| failed(format!("artifact_write_failed:{error}")))?;
            if self.sync_fault == ArtifactSyncFault::FileSync {
                return Err(failed("artifact_sync_failed"));
            }
            file.sync_all()
                .map_err(|error| failed(format!("artifact_sync_failed:{error}")))?;
            match directory.rename_noreplace(&temporary, name)? {
                platform::RenameOutcome::Published => Ok(()),
                platform::RenameOutcome::AlreadyPresent => {
                    let existing = directory
                        .read_regular(name, limit, conflict)?
                        .ok_or_else(|| PortError::Conflict(conflict.to_owned()))?;
                    if existing != bytes {
                        return Err(PortError::Conflict(conflict.to_owned()));
                    }
                    Ok(())
                }
            }
        })();
        if let Err(error) = staged {
            // The temporary file is only still present when the rename never happened; removing it
            // is best effort because the caller needs the original rejection, not a cleanup report.
            let _ = directory.unlink(&temporary);
            return Err(error);
        }
        // After a successful rename the temporary name is gone, so this is a no-op; after the
        // idempotent conflict path it removes the loser's copy and the directory sync below covers
        // the removal.
        directory.unlink(&temporary)?;
        if self.sync_fault == ArtifactSyncFault::DirectorySync {
            return Err(failed("result_unknown:artifact_directory_sync_failed"));
        }
        directory.sync_directory()
    }

    /// Store bytes under their own digest, reusing an existing identical blob.
    fn publish_blob(
        &self,
        blobs: &platform::PinnedDir,
        content_hex: &str,
        content: &[u8],
    ) -> Result<(), PortError> {
        if let Some(existing) = blobs.read_regular(
            content_hex,
            self.limits.max_artifact_bytes,
            "artifact_content_too_large",
        )? {
            return Self::require_same_blob(&existing, content_hex, content);
        }
        self.publish_immutable(
            blobs,
            content_hex,
            content,
            self.limits.max_artifact_bytes,
            "artifact_content_hash_mismatch",
        )
    }

    /// An existing blob is reused only when it hashes to the name it is stored under *and* equals
    /// the offered bytes. A longer file with the same prefix, a shorter one, or a file whose digest
    /// drifted are all `artifact_content_hash_mismatch`.
    fn require_same_blob(
        existing: &[u8],
        content_hex: &str,
        content: &[u8],
    ) -> Result<(), PortError> {
        if existing != content || kiana_domain::journal_sha256(existing) != content_hex {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// Stage bytes and publish the manifest that describes them. Staging the same version twice
    /// returns the manifest published first, so a retry cannot rewrite history by proposing a later
    /// creation time; a different reference for the same `(artifact, version)` is a conflict.
    fn stage_sync(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactManifest, PortError> {
        self.validate_stage(&version, &content)?;
        let Some(content_hex) = normalize_digest(&version.content_hash) else {
            return Err(failed("artifact_content_hash_invalid"));
        };
        let blobs = self.root.subdir(BLOBS_DIR, true)?;
        self.publish_blob(&blobs, &content_hex, &content)?;

        let manifest = ArtifactManifest::new(version, format!("sha256:{content_hex}"))
            .map_err(|error| failed(format!("artifact_manifest_invalid:{error}")))?;
        let reference = manifest.version.as_ref();
        let payload = serde_json::to_vec(&manifest)
            .map_err(|error| failed(format!("artifact_manifest_encode_failed:{error}")))?;
        if payload.len() > self.limits.max_manifest_bytes {
            return Err(failed("artifact_manifest_size_exceeded"));
        }

        let directory = self.version_dir(&reference)?;
        let name = manifest_name(&reference);
        if let Some(existing_bytes) = directory.read_regular(
            &name,
            self.limits.max_manifest_bytes,
            "artifact_manifest_size_exceeded",
        )? {
            let existing = Self::decode_manifest(&existing_bytes)?;
            if existing.version.as_ref() != reference {
                return Err(PortError::Conflict("artifact_version_conflict".to_owned()));
            }
            return Ok(existing);
        }
        self.publish_immutable(
            &directory,
            &name,
            &payload,
            self.limits.max_manifest_bytes,
            "artifact_version_conflict",
        )?;
        // Return what is on disk rather than the proposal: a call that lost the publish race must
        // report the first writer's manifest, not the one it wanted to write.
        self.read_staged(&directory, &reference)
            .map(|(manifest, _)| manifest)
    }

    /// Legacy staging contract: a version that already has a manifest is a conflict, because
    /// `stage_artifact` has no way to return the version that was held first.
    fn stage_legacy_sync(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactManifest, PortError> {
        self.validate_stage(&version, &content)?;
        let reference = version.as_ref();
        if let Some(directory) = self.version_dir_optional(&reference)? {
            if directory
                .read_regular(
                    &manifest_name(&reference),
                    self.limits.max_manifest_bytes,
                    "artifact_manifest_size_exceeded",
                )?
                .is_some()
            {
                return Err(PortError::Conflict(
                    "artifact_version_already_staged".to_owned(),
                ));
            }
        }
        self.stage_sync(version, content)
    }

    fn commit_sync(&self, reference: &ArtifactRef) -> Result<ArtifactManifest, PortError> {
        self.validate_reference(reference)?;
        let Some(directory) = self.version_dir_optional(reference)? else {
            return Err(PortError::Unavailable(
                "artifact_version_not_staged".to_owned(),
            ));
        };
        let (manifest, manifest_bytes) = self.read_staged(&directory, reference)?;
        // The blob must still be present and correct before the version becomes readable, so a
        // commit can never publish a manifest whose bytes are missing or replaced.
        self.read_blob(&manifest)?;
        let marker = CommitMarker {
            schema: COMMIT_MARKER_SCHEMA.to_owned(),
            manifest_sha256: kiana_domain::journal_sha256(&manifest_bytes),
        };
        let payload = serde_json::to_vec(&marker)
            .map_err(|error| failed(format!("artifact_commit_marker_encode_failed:{error}")))?;
        if payload.len() > self.limits.max_manifest_bytes {
            return Err(failed("artifact_manifest_size_exceeded"));
        }
        self.publish_immutable(
            &directory,
            &commit_name(reference),
            &payload,
            self.limits.max_manifest_bytes,
            "artifact_manifest_digest_mismatch",
        )?;
        Ok(manifest)
    }

    fn read_committed(
        &self,
        reference: &ArtifactRef,
    ) -> Result<(ArtifactManifest, Vec<u8>), PortError> {
        self.validate_reference(reference)?;
        let Some(directory) = self.version_dir_optional(reference)? else {
            return Err(PortError::Unavailable(
                "artifact_version_not_committed".to_owned(),
            ));
        };
        let (manifest, manifest_bytes) = self.read_staged(&directory, reference)?;
        let Some(marker_bytes) = directory.read_regular(
            &commit_name(reference),
            self.limits.max_manifest_bytes,
            "artifact_manifest_size_exceeded",
        )?
        else {
            return Err(PortError::Unavailable(
                "artifact_version_not_committed".to_owned(),
            ));
        };
        let marker: CommitMarker = serde_json::from_slice(&marker_bytes)
            .map_err(|error| failed(format!("artifact_commit_marker_invalid:{error}")))?;
        // The marker pins the manifest bytes that were committed. A staged manifest edited after
        // the commit, a truncated one, or one whose seal no longer recomputes stops matching here.
        if marker.schema != COMMIT_MARKER_SCHEMA
            || normalize_digest(&marker.manifest_sha256).as_deref()
                != Some(kiana_domain::journal_sha256(&manifest_bytes).as_str())
        {
            return Err(PortError::Conflict(
                "artifact_manifest_digest_mismatch".to_owned(),
            ));
        }
        let content = self.read_blob(&manifest)?;
        Ok((manifest, content))
    }

    fn blob_present_sync(&self, content_hash: &str) -> Result<bool, PortError> {
        let Some(content_hex) = normalize_digest(content_hash) else {
            return Err(failed("artifact_content_hash_invalid"));
        };
        let Some(blobs) = self.root.subdir_optional(BLOBS_DIR)? else {
            return Ok(false);
        };
        Ok(blobs.open_regular(&content_hex)?.is_some())
    }
}

fn manifest_name(reference: &ArtifactRef) -> String {
    format!("v{}.manifest", reference.version)
}

fn commit_name(reference: &ArtifactRef) -> String {
    format!("v{}.commit", reference.version)
}

#[async_trait]
impl ContentAddressedArtifactStorePort for UnifiedArtifactStore {
    fn owner_scope_digest(&self) -> String {
        format!("sha256:{}", self.owner_scope_hex)
    }

    fn max_artifact_bytes(&self) -> usize {
        self.limits.max_artifact_bytes
    }

    async fn stage_content(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactManifest, PortError> {
        self.blocking(move |store| store.stage_sync(version, content))
            .await
    }

    async fn commit_content(&self, reference: &ArtifactRef) -> Result<ArtifactManifest, PortError> {
        let reference = reference.clone();
        self.blocking(move |store| store.commit_sync(&reference))
            .await
    }

    async fn read_manifest(&self, reference: &ArtifactRef) -> Result<ArtifactManifest, PortError> {
        let reference = reference.clone();
        self.blocking(move |store| store.read_committed(&reference))
            .await
            .map(|(manifest, _)| manifest)
    }

    async fn read_content(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        let reference = reference.clone();
        self.blocking(move |store| store.read_committed(&reference))
            .await
            .map(|(_, content)| content)
    }

    async fn verify_content(&self, reference: &ArtifactRef) -> Result<ArtifactManifest, PortError> {
        let reference = reference.clone();
        self.blocking(move |store| store.read_committed(&reference))
            .await
            .map(|(manifest, _)| manifest)
    }

    async fn blob_present(&self, content_hash: &str) -> Result<bool, PortError> {
        let content_hash = content_hash.to_owned();
        self.blocking(move |store| store.blob_present_sync(&content_hash))
            .await
    }
}

#[async_trait]
impl ArtifactStorePort for UnifiedArtifactStore {
    async fn stage_artifact(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactRef, PortError> {
        self.blocking(move |store| store.stage_legacy_sync(version, content))
            .await
            .map(|manifest| manifest.version.as_ref())
    }

    async fn stage_artifact_version(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactVersion, PortError> {
        self.blocking(move |store| store.stage_sync(version, content))
            .await
            .map(|manifest| manifest.version)
    }

    async fn commit_artifact(
        &self,
        reference: ArtifactRef,
        expected_revision: Option<u64>,
    ) -> Result<(), PortError> {
        self.blocking(move |store| {
            if expected_revision.is_some_and(|revision| revision != reference.version) {
                return Err(PortError::Conflict(
                    "artifact_manifest_revision_conflict".to_owned(),
                ));
            }
            store.commit_sync(&reference).map(|_| ())
        })
        .await
    }

    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        let reference = reference.clone();
        self.blocking(move |store| store.read_committed(&reference))
            .await
            .map(|(_, content)| content)
    }

    async fn verify_artifact(&self, reference: &ArtifactRef) -> Result<(), PortError> {
        let reference = reference.clone();
        self.blocking(move |store| store.read_committed(&reference))
            .await
            .map(|_| ())
    }
}

/// Descriptor-pinned directory operations.
///
/// Every mutation and every read goes through the descriptor opened at `open` time; nothing is
/// re-resolved from a path, and nothing follows a symlink. Non-Linux targets report the capability
/// as unsupported instead of falling back to a path-based implementation that would silently have
/// different atomicity.
#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use crate::artifacts::{
        linux_artifact_renameat2, linux_artifact_sync_directory, linux_artifact_unlinkat,
        linux_create_artifact_temp, LINUX_RENAME_NOREPLACE,
    };
    use std::ffi::{CString, OsStr};
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, RawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    pub(super) enum RenameOutcome {
        Published,
        AlreadyPresent,
    }

    #[derive(Debug)]
    pub(super) struct PinnedDir(std::fs::File);

    /// Validate one name the *store itself* constructs before it reaches `openat`.
    ///
    /// Those names are digests, UUIDs and `v<n>.<kind>`, so accepting only leading alphanumerics
    /// plus `-_.` keeps `.`, `..` and anything NUL-containing out of the way.
    fn component(name: &str) -> Result<CString, PortError> {
        let valid = !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if !valid {
            return Err(failed("artifact_path_component_invalid"));
        }
        CString::new(name).map_err(|_| failed("artifact_path_component_invalid"))
    }

    /// Validate one component of a caller-supplied root path. The caller chooses where its store
    /// lives, so this only refuses traversal (`.`, `..`, NUL) instead of imposing a charset.
    fn root_component(name: &OsStr) -> Result<CString, PortError> {
        if name.is_empty() || name == OsStr::new(".") || name == OsStr::new("..") {
            return Err(failed("artifact_root_component_invalid"));
        }
        CString::new(name.as_bytes()).map_err(|_| failed("artifact_root_component_invalid"))
    }

    /// Open a child directory relative to an open descriptor, optionally creating it and syncing
    /// the parent entry so a crash cannot leave a directory the next call believes it created.
    fn open_directory_at(
        parent: RawFd,
        name: &CString,
        create: bool,
    ) -> Result<std::fs::File, PortError> {
        let open = || unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        let mut fd = open();
        if fd < 0 && create && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
        {
            let created = unsafe { libc::mkdirat(parent, name.as_ptr(), 0o700) };
            if created != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
            {
                return Err(failed(format!(
                    "artifact_directory_create_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            linux_artifact_sync_directory(parent)
                .map_err(|error| failed(format!("artifact_directory_sync_failed:{error}")))?;
            fd = open();
        }
        if fd < 0 {
            return Err(failed(format!(
                "artifact_directory_open_failed:{}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }

    impl PinnedDir {
        /// Open an absolute root, walking every component with `O_NOFOLLOW` so no symlink on the
        /// way in can redirect the handle.
        pub(super) fn open(path: &Path, create: bool) -> Result<Self, PortError> {
            if !path.is_absolute() {
                return Err(failed("artifact_root_must_be_absolute"));
            }
            let root = CString::new("/").expect("constant");
            let fd = unsafe {
                libc::open(
                    root.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd < 0 {
                return Err(failed(format!(
                    "artifact_root_open_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            let mut current = Self(unsafe { std::fs::File::from_raw_fd(fd) });
            for path_component in path.components() {
                match path_component {
                    std::path::Component::RootDir => {}
                    std::path::Component::Normal(name) => {
                        let name = root_component(name)?;
                        current = Self(open_directory_at(current.0.as_raw_fd(), &name, create)?);
                    }
                    // `.` is normalized away by `components`; `..` and prefixes are refused rather
                    // than resolved, so a root cannot be opened through a parent traversal.
                    _ => return Err(failed("artifact_root_component_invalid")),
                }
            }
            Ok(current)
        }

        pub(super) fn subdir(&self, name: &str, create: bool) -> Result<Self, PortError> {
            let name = component(name)?;
            open_directory_at(self.0.as_raw_fd(), &name, create).map(Self)
        }

        pub(super) fn subdir_optional(&self, name: &str) -> Result<Option<Self>, PortError> {
            let name = component(name)?;
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                return Err(failed(format!("artifact_directory_open_failed:{error}")));
            }
            Ok(Some(Self(unsafe { std::fs::File::from_raw_fd(fd) })))
        }

        /// Open a stored file, refusing symlinks, non-regular files and hard-linked inodes.
        pub(super) fn open_regular(&self, name: &str) -> Result<Option<std::fs::File>, PortError> {
            let name = component(name)?;
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                if error.raw_os_error() == Some(libc::ELOOP) {
                    return Err(failed("artifact_file_symlinked"));
                }
                return Err(failed(format!("artifact_file_open_failed:{error}")));
            }
            let file = unsafe { std::fs::File::from_raw_fd(fd) };
            let metadata = file
                .metadata()
                .map_err(|error| failed(format!("artifact_file_metadata_failed:{error}")))?;
            if !metadata.file_type().is_file() {
                return Err(failed("artifact_file_not_regular"));
            }
            if metadata.nlink() != 1 {
                return Err(failed("artifact_file_hardlinked"));
            }
            Ok(Some(file))
        }

        /// Read a stored file with a bound. A larger file is a rejection, not a truncated read.
        pub(super) fn read_regular(
            &self,
            name: &str,
            limit: usize,
            too_large: &str,
        ) -> Result<Option<Vec<u8>>, PortError> {
            let Some(file) = self.open_regular(name)? else {
                return Ok(None);
            };
            let mut bytes = Vec::new();
            (&file)
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| failed(format!("artifact_read_failed:{error}")))?;
            if bytes.len() > limit {
                return Err(PortError::Conflict(too_large.to_owned()));
            }
            Ok(Some(bytes))
        }

        pub(super) fn create_temp(&self) -> Result<(CString, std::fs::File), PortError> {
            linux_create_artifact_temp(self.0.as_raw_fd())
                .map_err(|error| failed(format!("artifact_temp_create_failed:{error}")))
        }

        /// Rename the temporary file into place without replacing an existing entry.
        pub(super) fn rename_noreplace(
            &self,
            source: &CString,
            target: &str,
        ) -> Result<RenameOutcome, PortError> {
            let target = component(target)?;
            match linux_artifact_renameat2(
                self.0.as_raw_fd(),
                source,
                &target,
                LINUX_RENAME_NOREPLACE,
            ) {
                Ok(()) => Ok(RenameOutcome::Published),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    Ok(RenameOutcome::AlreadyPresent)
                }
                Err(error) => Err(failed(format!("artifact_publish_failed:{error}"))),
            }
        }

        /// Remove a temporary name. A name that is already gone is success: a successful rename
        /// consumes it.
        pub(super) fn unlink(&self, name: &CString) -> Result<(), PortError> {
            match linux_artifact_unlinkat(self.0.as_raw_fd(), name) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(failed(format!("artifact_temp_cleanup_failed:{error}"))),
            }
        }

        pub(super) fn sync_directory(&self) -> Result<(), PortError> {
            linux_artifact_sync_directory(self.0.as_raw_fd())
                .map_err(|error| failed(format!("artifact_directory_sync_failed:{error}")))
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    //! Non-Linux targets report the store as unsupported: the atomic no-replace publish and the
    //! descriptor-pinned reads this module promises are only implemented there today (PD-32 owns
    //! the platform matrix and the capability report).

    #![allow(dead_code)]

    use super::*;
    use std::ffi::CString;

    #[allow(dead_code)]
    pub(super) enum RenameOutcome {
        Published,
        AlreadyPresent,
    }

    #[derive(Debug)]
    pub(super) struct PinnedDir;

    impl PinnedDir {
        fn unsupported<T>() -> Result<T, PortError> {
            Err(PortError::Unavailable(
                "artifact_store_platform_unsupported".to_owned(),
            ))
        }

        pub(super) fn open(_: &Path, _: bool) -> Result<Self, PortError> {
            Self::unsupported()
        }

        pub(super) fn subdir(&self, _: &str, _: bool) -> Result<Self, PortError> {
            Self::unsupported()
        }

        pub(super) fn subdir_optional(&self, _: &str) -> Result<Option<Self>, PortError> {
            Self::unsupported()
        }

        pub(super) fn open_regular(&self, _: &str) -> Result<Option<std::fs::File>, PortError> {
            Self::unsupported()
        }

        pub(super) fn read_regular(
            &self,
            _: &str,
            _: usize,
            _: &str,
        ) -> Result<Option<Vec<u8>>, PortError> {
            Self::unsupported()
        }

        pub(super) fn create_temp(&self) -> Result<(CString, std::fs::File), PortError> {
            Self::unsupported()
        }

        pub(super) fn rename_noreplace(
            &self,
            _: &CString,
            _: &str,
        ) -> Result<RenameOutcome, PortError> {
            Self::unsupported()
        }

        pub(super) fn unlink(&self, _: &CString) -> Result<(), PortError> {
            Self::unsupported()
        }

        pub(super) fn sync_directory(&self) -> Result<(), PortError> {
            Self::unsupported()
        }
    }
}
