//! Local immutable artifact blobs rooted at an explicitly injected absolute directory.
//!
//! The directory is opened once and retained by descriptor, so later workspace changes or root
//! path replacement do not redirect existing store handles. This adapter records file and
//! directory sync acknowledgements; it does not claim power-loss-proof durability.

use crate::local_packages::{failed, sha256, LocalDir};
use async_trait::async_trait;
use kiana_domain::{ArtifactRef, ArtifactVersion, DataPropagationReceipt, MAX_ARTIFACT_BYTES};
use kiana_ports::{ArtifactContentPort, ArtifactStorePort, PortError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

const MANIFEST_SCHEMA: &str = "kiana.local-artifact-manifest.v1";
const STAGE_SCHEMA: &str = "kiana.local-artifact-stage.v1";
const COMMIT_SCHEMA: &str = "kiana.local-artifact-commit.v1";
const MAX_MANIFEST_BYTES: usize = 32 * 1024;
const MAX_MARKER_BYTES: usize = 4096;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactManifest {
    schema: String,
    version: ArtifactVersion,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactMarker {
    schema: String,
    manifest_sha256: String,
}

/// Descriptor-pinned local implementation of artifact staging, commit, verification and reads.
///
/// Callers choose the absolute storage root; this adapter does not infer a project or `.kiana`
/// path. It stores no authorization state and never performs content-sensitive authorization.
#[derive(Clone, Debug)]
pub struct LocalArtifactStore {
    root: Arc<LocalDir>,
}

impl LocalArtifactStore {
    /// Opens an explicit absolute root without following symlink path components.
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self, PortError> {
        let root = root.into();
        if !root.is_absolute() {
            return Err(failed("artifact_root_must_be_absolute"));
        }
        let opened = tokio::task::spawn_blocking(move || LocalDir::open(&root, true))
            .await
            .map_err(|error| failed(format!("artifact_root_open_join_failed:{error}")))??;
        Ok(Self {
            root: Arc::new(opened),
        })
    }

    fn validate_content(version: &ArtifactVersion, content: &[u8]) -> Result<(), PortError> {
        version
            .validate()
            .map_err(|error| failed(format!("artifact_version_invalid:{error}")))?;
        if content.len() > MAX_ARTIFACT_BYTES
            || version.size_bytes != content.len() as u64
            || kiana_domain::journal_sha256(content) != version.content_hash
        {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    fn key_directory(&self, reference: &ArtifactRef) -> Result<Option<LocalDir>, PortError> {
        let scope = sha256(reference.scope_digest.as_bytes());
        let Some(scope_dir) = self.root.subdir_optional(&scope)? else {
            return Ok(None);
        };
        let artifact = reference.artifact_id.to_string();
        scope_dir.subdir_optional(&artifact)
    }

    fn create_key_directory(&self, reference: &ArtifactRef) -> Result<LocalDir, PortError> {
        let scope = sha256(reference.scope_digest.as_bytes());
        let scope_dir = self.root.subdir(&scope, true)?;
        scope_dir.subdir(&reference.artifact_id.to_string(), true)
    }

    fn filenames(reference: &ArtifactRef) -> [String; 4] {
        let version = format!("v{}", reference.version);
        [
            format!("{version}.blob"),
            format!("{version}.manifest"),
            format!("{version}.stage"),
            format!("{version}.commit"),
        ]
    }

    fn encode<T: Serialize>(value: &T, reason: &str) -> Result<Vec<u8>, PortError> {
        serde_json::to_vec(value).map_err(|error| failed(format!("{reason}:{error}")))
    }

    fn decode<T: for<'de> Deserialize<'de>>(
        bytes: &[u8],
        reason: &str,
    ) -> Result<T, PortError> {
        serde_json::from_slice(bytes).map_err(|error| failed(format!("{reason}:{error}")))
    }

    fn read_optional(
        directory: &LocalDir,
        name: &str,
        limit: usize,
        missing: &str,
    ) -> Result<Vec<u8>, PortError> {
        directory
            .read_optional(name, limit)?
            .ok_or_else(|| PortError::Unavailable(missing.to_owned()))
    }

    fn read_manifest(
        directory: &LocalDir,
        reference: &ArtifactRef,
        manifest_name: &str,
    ) -> Result<(ArtifactManifest, Vec<u8>), PortError> {
        let bytes = Self::read_optional(
            directory,
            manifest_name,
            MAX_MANIFEST_BYTES,
            "artifact_manifest_missing",
        )?;
        let manifest: ArtifactManifest = Self::decode(&bytes, "artifact_manifest_invalid")?;
        if manifest.schema != MANIFEST_SCHEMA {
            return Err(failed("artifact_manifest_schema_invalid"));
        }
        manifest
            .version
            .validate()
            .map_err(|error| failed(format!("artifact_manifest_version_invalid:{error}")))?;
        if manifest.version.as_ref() != *reference {
            return Err(PortError::Conflict(
                "artifact_reference_manifest_mismatch".to_owned(),
            ));
        }
        Ok((manifest, bytes))
    }

    fn read_marker(
        directory: &LocalDir,
        name: &str,
        schema: &str,
        missing: &str,
    ) -> Result<ArtifactMarker, PortError> {
        let bytes = Self::read_optional(directory, name, MAX_MARKER_BYTES, missing)?;
        let marker: ArtifactMarker = Self::decode(&bytes, "artifact_marker_invalid")?;
        if marker.schema != schema || !kiana_domain::is_sha256_hex(&marker.manifest_sha256) {
            return Err(failed("artifact_marker_schema_invalid"));
        }
        Ok(marker)
    }

    fn read_committed_bytes(
        &self,
        reference: &ArtifactRef,
    ) -> Result<(ArtifactVersion, Vec<u8>), PortError> {
        reference
            .validate()
            .map_err(|error| failed(format!("artifact_reference_invalid:{error}")))?;
        let Some(directory) = self.key_directory(reference)? else {
            return Err(PortError::Unavailable(
                "artifact_version_not_committed".to_owned(),
            ));
        };
        let [blob_name, manifest_name, stage_name, commit_name] = Self::filenames(reference);
        let commit = Self::read_marker(
            &directory,
            &commit_name,
            COMMIT_SCHEMA,
            "artifact_version_not_committed",
        )?;
        let stage = Self::read_marker(
            &directory,
            &stage_name,
            STAGE_SCHEMA,
            "artifact_stage_manifest_missing",
        )?;
        let (manifest, manifest_bytes) =
            Self::read_manifest(&directory, reference, &manifest_name)?;
        let manifest_digest = sha256(&manifest_bytes);
        if stage.manifest_sha256 != manifest_digest || commit.manifest_sha256 != manifest_digest {
            return Err(PortError::Conflict(
                "artifact_manifest_digest_mismatch".to_owned(),
            ));
        }
        let bytes = Self::read_optional(
            &directory,
            &blob_name,
            MAX_ARTIFACT_BYTES,
            "artifact_blob_missing",
        )?;
        Self::validate_content(&manifest.version, &bytes)?;
        Ok((manifest.version, bytes))
    }

    fn stage_sync(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactRef, PortError> {
        let reference = version.as_ref();
        let directory = self.create_key_directory(&reference)?;
        let [blob_name, manifest_name, stage_name, _] = Self::filenames(&reference);
        let manifest = ArtifactManifest {
            schema: MANIFEST_SCHEMA.to_owned(),
            version,
        };
        let manifest_bytes = Self::encode(&manifest, "artifact_manifest_encode_failed")?;
        if manifest_bytes.len() > MAX_MANIFEST_BYTES {
            return Err(failed("artifact_manifest_size_exceeded"));
        }
        let manifest_digest = sha256(&manifest_bytes);
        directory.publish_immutable(
            &blob_name,
            &content,
            "artifact_version_conflict",
        )?;
        directory.publish_immutable(
            &manifest_name,
            &manifest_bytes,
            "artifact_version_conflict",
        )?;
        let marker = Self::encode(
            &ArtifactMarker {
                schema: STAGE_SCHEMA.to_owned(),
                manifest_sha256: manifest_digest,
            },
            "artifact_stage_marker_encode_failed",
        )?;
        directory.publish_immutable(
            &stage_name,
            &marker,
            "artifact_version_conflict",
        )?;
        Ok(reference)
    }

    fn commit_sync(
        &self,
        reference: ArtifactRef,
        expected_revision: Option<u64>,
    ) -> Result<(), PortError> {
        reference
            .validate()
            .map_err(|error| failed(format!("artifact_reference_invalid:{error}")))?;
        if expected_revision.is_some_and(|revision| revision != reference.version) {
            return Err(PortError::Conflict(
                "artifact_manifest_revision_conflict".to_owned(),
            ));
        }
        let Some(directory) = self.key_directory(&reference)? else {
            return Err(PortError::Unavailable(
                "artifact_version_not_staged".to_owned(),
            ));
        };
        let [blob_name, manifest_name, stage_name, commit_name] = Self::filenames(&reference);
        let stage = Self::read_marker(
            &directory,
            &stage_name,
            STAGE_SCHEMA,
            "artifact_version_not_staged",
        )?;
        let (manifest, manifest_bytes) =
            Self::read_manifest(&directory, &reference, &manifest_name)?;
        let manifest_digest = sha256(&manifest_bytes);
        if stage.manifest_sha256 != manifest_digest {
            return Err(PortError::Conflict(
                "artifact_manifest_digest_mismatch".to_owned(),
            ));
        }
        let content = Self::read_optional(
            &directory,
            &blob_name,
            MAX_ARTIFACT_BYTES,
            "artifact_blob_missing",
        )?;
        Self::validate_content(&manifest.version, &content)?;
        let marker = Self::encode(
            &ArtifactMarker {
                schema: COMMIT_SCHEMA.to_owned(),
                manifest_sha256: manifest_digest,
            },
            "artifact_commit_marker_encode_failed",
        )?;
        directory.publish_immutable(
            &commit_name,
            &marker,
            "artifact_commit_conflict",
        )
    }
}

#[async_trait]
impl ArtifactStorePort for LocalArtifactStore {
    async fn stage_artifact(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactRef, PortError> {
        Self::validate_content(&version, &content)?;
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self { root: Arc::new(root) }.stage_sync(version, content)
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_stage_join_failed:{error}"
            ))
        })?
    }

    async fn commit_artifact(
        &self,
        reference: ArtifactRef,
        expected_revision: Option<u64>,
    ) -> Result<(), PortError> {
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self { root: Arc::new(root) }.commit_sync(reference, expected_revision)
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_commit_join_failed:{error}"
            ))
        })?
    }

    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        let reference = reference.clone();
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self { root: Arc::new(root) }
                .read_committed_bytes(&reference)
                .map(|(_, content)| content)
        })
        .await
        .map_err(|error| failed(format!("artifact_read_join_failed:{error}")))?
    }

    async fn verify_artifact(&self, reference: &ArtifactRef) -> Result<(), PortError> {
        let reference = reference.clone();
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self { root: Arc::new(root) }
                .read_committed_bytes(&reference)
                .map(|_| ())
        })
        .await
        .map_err(|error| failed(format!("artifact_verify_join_failed:{error}")))?
    }

    async fn invalidate_artifact(
        &self,
        _reference: &ArtifactRef,
        _previous_epoch: u64,
        _data_epoch: u64,
        _tombstone_digest: &str,
        _observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        Err(PortError::Unavailable(
            "artifact_invalidation_unsupported".to_owned(),
        ))
    }
}

#[async_trait]
impl ArtifactContentPort for LocalArtifactStore {
    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        ArtifactStorePort::read_artifact(self, reference).await
    }
}
