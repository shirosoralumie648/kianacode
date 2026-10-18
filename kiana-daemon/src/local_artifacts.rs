//! Local immutable artifact blobs rooted at an explicitly injected absolute directory.
//!
//! The directory is opened once and retained by descriptor, so later workspace changes or root
//! path replacement do not redirect existing store handles. This adapter records file and
//! directory sync acknowledgements; it does not claim power-loss-proof durability.

use crate::local_packages::{failed, sha256, LocalDir};
use async_trait::async_trait;
use kiana_domain::{
    ArtifactId, ArtifactPublicationIntent, ArtifactPublicationState, ArtifactRef, ArtifactVersion,
    DataPropagationReceipt, DataPropagationState, DataPropagationTarget, EventId,
    MAX_ARTIFACT_BYTES,
};
use kiana_ports::{ArtifactContentPort, ArtifactStorePort, PortError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

const MANIFEST_SCHEMA: &str = "kiana.local-artifact-manifest.v1";
const STAGE_SCHEMA: &str = "kiana.local-artifact-stage.v1";
const COMMIT_SCHEMA: &str = "kiana.local-artifact-commit.v1";
/// Append-only publication journal. It is a recovery sidecar, not a fact source: the EventLog
/// remains the only authority for whether a Company command committed.
const PUBLICATION_JOURNAL: &str = "publications.log";
/// Append-only invalidation journal. Like the publication journal it is recovery state, not a
/// fact source: the EventLog remains the only authority for a governance decision. It is
/// monotonic, so a revoked version stays revoked across restarts even before any projection is
/// rebuilt.
const INVALIDATION_JOURNAL: &str = "invalidations.log";
const INVALIDATION_JOURNAL_SCHEMA: &str = "kiana.local-artifact-invalidation.v1";
const MAX_MANIFEST_BYTES: usize = 32 * 1024;
const MAX_MARKER_BYTES: usize = 4096;
const MAX_JOURNAL_BYTES: usize = 1024 * 1024;
const MAX_INVALIDATION_JOURNAL_BYTES: usize = 1024 * 1024;

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

/// One durable revocation boundary for a committed artifact version.
///
/// The record deliberately carries the tombstone digest and data epoch, so a replayed
/// invalidation with the same boundary is idempotent while a different tombstone for the same
/// version is a conflict rather than a silent second revocation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactInvalidation {
    schema: String,
    artifact_id: ArtifactId,
    version: u64,
    scope_digest: String,
    data_epoch: u64,
    tombstone_digest: String,
    observed_at_ms: u64,
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

    /// Latest publication-journal record for a logical Company command, if any. Read-only; the
    /// journal never authorizes a fact or a blob mutation.
    pub async fn publication_intent(
        &self,
        command_id: &str,
    ) -> Result<Option<ArtifactPublicationIntent>, PortError> {
        let command_id = command_id.to_owned();
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .read_journal()
            .map(|intents| {
                intents
                    .into_iter()
                    .find(|intent| intent.command_id == command_id)
            })
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_publication_journal_join_failed:{error}"
            ))
        })?
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

    fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8], reason: &str) -> Result<T, PortError> {
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
        // Revocation is checked before existence: a revoked version must never be reported as
        // merely "not committed", and its bytes must not leave the store.
        self.check_not_revoked(reference)?;
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

    /// Read the append-only publication journal and reduce it to the latest record per command id.
    /// A journal that outgrows the bounded read window is reported as full instead of being read
    /// partially, so a restart cannot act on a truncated record set.
    fn read_journal(&self) -> Result<Vec<ArtifactPublicationIntent>, PortError> {
        let bytes = match self
            .root
            .read_optional(PUBLICATION_JOURNAL, MAX_JOURNAL_BYTES)
        {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(Vec::new()),
            Err(PortError::Failed(reason)) if reason == "package_size_exceeded" => {
                return Err(PortError::Conflict(
                    "artifact_publication_journal_full".to_owned(),
                ))
            }
            Err(error) => return Err(error),
        };
        let mut latest: BTreeMap<String, ArtifactPublicationIntent> = BTreeMap::new();
        for line in bytes.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let intent: ArtifactPublicationIntent =
                Self::decode(line, "artifact_publication_journal_invalid")?;
            intent
                .validate()
                .map_err(|error| failed(format!("artifact_publication_journal_invalid:{error}")))?;
            latest.insert(intent.command_id.clone(), intent);
        }
        Ok(latest.into_values().collect())
    }

    fn append_publication_record(
        &self,
        intent: &ArtifactPublicationIntent,
    ) -> Result<(), PortError> {
        intent
            .validate()
            .map_err(|error| failed(format!("artifact_publication_invalid:{error}")))?;
        let mut line = Self::encode(intent, "artifact_publication_encode_failed")?;
        line.push(b'\n');
        let current = match self
            .root
            .read_optional(PUBLICATION_JOURNAL, MAX_JOURNAL_BYTES)
        {
            Ok(Some(bytes)) => bytes.len(),
            Ok(None) => 0,
            Err(PortError::Failed(reason)) if reason == "package_size_exceeded" => {
                MAX_JOURNAL_BYTES + 1
            }
            Err(error) => return Err(error),
        };
        if current + line.len() > MAX_JOURNAL_BYTES {
            return Err(PortError::Conflict(
                "artifact_publication_journal_full".to_owned(),
            ));
        }
        self.root.append(PUBLICATION_JOURNAL, &line)
    }

    fn record_publication_intent_sync(
        &self,
        intent: &ArtifactPublicationIntent,
    ) -> Result<(), PortError> {
        if !intent.is_pending() {
            return Err(PortError::Conflict(
                "artifact_publication_intent_not_pending".to_owned(),
            ));
        }
        self.append_publication_record(intent)
    }

    fn resolve_publication_intent_sync(
        &self,
        command_id: &str,
        state: ArtifactPublicationState,
        event_id: Option<EventId>,
        resolved_at_unix_ms: u64,
    ) -> Result<ArtifactPublicationIntent, PortError> {
        if state == ArtifactPublicationState::Pending {
            return Err(PortError::Conflict(
                "artifact_publication_state_invalid".to_owned(),
            ));
        }
        let latest = self
            .read_journal()?
            .into_iter()
            .find(|intent| intent.command_id == command_id)
            .ok_or_else(|| {
                PortError::Unavailable("artifact_publication_intent_missing".to_owned())
            })?;
        if !latest.is_pending() {
            if latest.state != state || latest.resolved_event_id != event_id {
                return Err(PortError::Conflict(
                    "artifact_publication_already_resolved".to_owned(),
                ));
            }
            return Ok(latest);
        }
        let resolved = latest
            .resolved(state, event_id, resolved_at_unix_ms)
            .map_err(failed)?;
        self.append_publication_record(&resolved)?;
        Ok(resolved)
    }

    fn pending_publication_intents_sync(
        &self,
    ) -> Result<Vec<ArtifactPublicationIntent>, PortError> {
        let mut pending: Vec<ArtifactPublicationIntent> = self
            .read_journal()?
            .into_iter()
            .filter(ArtifactPublicationIntent::is_pending)
            .collect();
        pending.sort_by(|left, right| left.command_id.cmp(&right.command_id));
        Ok(pending)
    }

    fn publication_intents_sync(&self) -> Result<Vec<ArtifactPublicationIntent>, PortError> {
        let mut intents = self.read_journal()?;
        intents.sort_by(|left, right| left.command_id.cmp(&right.command_id));
        Ok(intents)
    }

    /// Read the append-only invalidation journal and reduce it to one boundary per artifact
    /// version. Conflicting boundaries for the same version fail closed instead of letting the
    /// last line silently win, so a concurrent or replayed writer cannot weaken a revocation.
    fn read_invalidations(
        &self,
    ) -> Result<BTreeMap<(ArtifactId, u64), ArtifactInvalidation>, PortError> {
        let bytes = match self
            .root
            .read_optional(INVALIDATION_JOURNAL, MAX_INVALIDATION_JOURNAL_BYTES)
        {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(BTreeMap::new()),
            Err(PortError::Failed(reason)) if reason == "package_size_exceeded" => {
                return Err(PortError::Conflict(
                    "artifact_invalidation_journal_full".to_owned(),
                ))
            }
            Err(error) => return Err(error),
        };
        let mut reduced: BTreeMap<(ArtifactId, u64), ArtifactInvalidation> = BTreeMap::new();
        for line in bytes.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let record: ArtifactInvalidation =
                Self::decode(line, "artifact_invalidation_journal_invalid")?;
            Self::validate_invalidation_record(&record)?;
            match reduced.get(&(record.artifact_id, record.version)) {
                Some(existing)
                    if existing.data_epoch == record.data_epoch
                        && existing.tombstone_digest == record.tombstone_digest
                        && existing.scope_digest == record.scope_digest => {}
                Some(_) => {
                    return Err(PortError::Conflict(
                        "artifact_invalidation_journal_conflict".to_owned(),
                    ))
                }
                None => {
                    reduced.insert((record.artifact_id, record.version), record);
                }
            }
        }
        Ok(reduced)
    }

    fn validate_invalidation_record(record: &ArtifactInvalidation) -> Result<(), PortError> {
        if record.schema != INVALIDATION_JOURNAL_SCHEMA
            || record.version == 0
            || record.data_epoch == 0
            || record.observed_at_ms == 0
            || !Self::valid_prefixed_digest(&record.scope_digest)
            || !Self::valid_tombstone_digest(&record.tombstone_digest)
        {
            return Err(PortError::Failed(
                "artifact_invalidation_journal_invalid".to_owned(),
            ));
        }
        Ok(())
    }

    /// A tombstone is an explicit `sha256:` digest so a caller cannot pass a bare or
    /// foreign-shaped string and still produce a receipt that validates.
    fn valid_tombstone_digest(value: &str) -> bool {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return false;
        };
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    /// Scope digests follow the repository convention of either a bare or prefixed SHA-256
    /// text; only the 64 hex characters are load-bearing.
    fn valid_prefixed_digest(value: &str) -> bool {
        let hex = value.strip_prefix("sha256:").unwrap_or(value);
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    /// Deny a read once the journal records a revocation boundary for this version.
    fn check_not_revoked(&self, reference: &ArtifactRef) -> Result<(), PortError> {
        if self
            .read_invalidations()?
            .contains_key(&(reference.artifact_id, reference.version))
        {
            return Err(PortError::Conflict(
                "artifact_data_revoked_or_expired".to_owned(),
            ));
        }
        Ok(())
    }

    fn append_invalidation_record(&self, record: &ArtifactInvalidation) -> Result<(), PortError> {
        Self::validate_invalidation_record(record)?;
        let mut line = Self::encode(record, "artifact_invalidation_encode_failed")?;
        line.push(b'\n');
        let current = match self
            .root
            .read_optional(INVALIDATION_JOURNAL, MAX_INVALIDATION_JOURNAL_BYTES)
        {
            Ok(Some(bytes)) => bytes.len(),
            Ok(None) => 0,
            Err(PortError::Failed(reason)) if reason == "package_size_exceeded" => {
                MAX_INVALIDATION_JOURNAL_BYTES + 1
            }
            Err(error) => return Err(error),
        };
        if current + line.len() > MAX_INVALIDATION_JOURNAL_BYTES {
            return Err(PortError::Conflict(
                "artifact_invalidation_journal_full".to_owned(),
            ));
        }
        self.root.append(INVALIDATION_JOURNAL, &line)
    }

    fn invalidation_receipt(
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        use serde_json::json;
        let mut receipt = DataPropagationReceipt {
            schema: kiana_domain::DATA_PROPAGATION_RECEIPT_SCHEMA.to_owned(),
            version: kiana_domain::DATA_PROPAGATION_VERSION,
            target: DataPropagationTarget::Artifact,
            state: DataPropagationState::Invalidated,
            project_ref: reference.scope_digest.clone(),
            previous_epoch,
            data_epoch,
            source_cursor: reference.version,
            tombstone_digest: tombstone_digest.to_owned(),
            receipt_digest: None,
            observed_at_ms,
        };
        receipt.receipt_digest = Some(kiana_domain::json_digest(&json!({
            "artifact_id": reference.artifact_id,
            "version": reference.version,
            "scope_digest": reference.scope_digest,
            "data_epoch": data_epoch,
            "tombstone_digest": tombstone_digest,
        })));
        receipt
            .validate()
            .map_err(|error| failed(format!("artifact_invalidation_receipt:{error}")))?;
        Ok(receipt)
    }

    fn invalidate_sync(
        &self,
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        reference
            .validate()
            .map_err(|error| failed(format!("artifact_reference_invalid:{error}")))?;
        if previous_epoch == 0
            || data_epoch <= previous_epoch
            || observed_at_ms == 0
            || !Self::valid_tombstone_digest(tombstone_digest)
        {
            return Err(PortError::Failed(
                "artifact_invalidation_boundary_invalid".to_owned(),
            ));
        }

        // Idempotent replay is resolved from the journal before the filesystem: a revoked
        // reference no longer needs to be readable to confirm the same boundary twice.
        if let Some(existing) = self
            .read_invalidations()?
            .get(&(reference.artifact_id, reference.version))
        {
            if existing.data_epoch != data_epoch
                || existing.tombstone_digest != tombstone_digest
                || existing.scope_digest != reference.scope_digest
            {
                return Err(PortError::Conflict(
                    "artifact_invalidation_conflict".to_owned(),
                ));
            }
            return Self::invalidation_receipt(
                reference,
                previous_epoch,
                data_epoch,
                tombstone_digest,
                observed_at_ms,
            );
        }

        // Only a committed version can be revoked; revoking a missing or merely staged version
        // would fabricate reach over bytes that governance never admitted.
        let Some(directory) = self.key_directory(reference)? else {
            return Err(PortError::Unavailable(
                "artifact_version_not_committed".to_owned(),
            ));
        };
        let [_, _, _, commit_name] = Self::filenames(reference);
        Self::read_marker(
            &directory,
            &commit_name,
            COMMIT_SCHEMA,
            "artifact_version_not_committed",
        )?;

        self.append_invalidation_record(&ArtifactInvalidation {
            schema: INVALIDATION_JOURNAL_SCHEMA.to_owned(),
            artifact_id: reference.artifact_id,
            version: reference.version,
            scope_digest: reference.scope_digest.clone(),
            data_epoch,
            tombstone_digest: tombstone_digest.to_owned(),
            observed_at_ms,
        })?;
        Self::invalidation_receipt(
            reference,
            previous_epoch,
            data_epoch,
            tombstone_digest,
            observed_at_ms,
        )
    }

    /// Reclaim the bytes of a committed blob that this store's own journal proves fact-free.
    ///
    /// The caller's retention decision is never trusted: this method re-derives the precondition
    /// from the append-only journal and refuses any reference that any recorded intent still holds
    /// as pending or committed. The pinned descriptor and the recorded content hash are checked
    /// before unlinking, so a replaced, drifted or hard-linked file is refused rather than deleted.
    fn remove_orphan_blob_sync(&self, reference: &ArtifactRef) -> Result<bool, PortError> {
        reference
            .validate()
            .map_err(|error| failed(format!("artifact_reference_invalid:{error}")))?;
        // The journal is checked before the filesystem: a reference this store never recorded is
        // denied rather than reported as "nothing to reclaim", so a mis-targeted call cannot look
        // like a successful no-op.
        let journal = self.read_journal()?;
        let mut matching = journal
            .iter()
            .filter(|intent| intent.artifact == *reference)
            .peekable();
        if matching.peek().is_none() {
            return Err(PortError::Conflict(
                "artifact_orphan_retention_unrecorded".to_owned(),
            ));
        }
        if matching.any(|intent| intent.state != ArtifactPublicationState::Abandoned) {
            return Err(PortError::Conflict(
                "artifact_orphan_retention_denied".to_owned(),
            ));
        }
        let Some(directory) = self.key_directory(&reference)? else {
            return Ok(false);
        };
        let [blob_name, manifest_name, stage_name, commit_name] = Self::filenames(&reference);
        let blob_marker = blob_name.clone();
        let mut removed = false;
        // Remove the commit marker first: after it is gone no reader can treat this version as
        // committed, so an interrupted reclamation cannot leave a half-readable artifact.
        for name in [commit_name, stage_name, manifest_name, blob_name] {
            let Some(owned) = directory.open_regular_file(&name)? else {
                continue;
            };
            if name == blob_marker {
                let mut bytes = Vec::new();
                (&owned)
                    .take(MAX_ARTIFACT_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| failed(format!("artifact_orphan_read_failed:{error}")))?;
                if bytes.len() > MAX_ARTIFACT_BYTES
                    || kiana_domain::journal_sha256(&bytes) != reference.content_hash
                {
                    return Err(PortError::Conflict(
                        "artifact_orphan_content_drift".to_owned(),
                    ));
                }
            }
            directory.remove_owned_file(&name, &owned)?;
            removed = true;
        }
        Ok(removed)
    }

    fn stage_version_sync(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactVersion, PortError> {
        Self::validate_content(&version, &content)?;
        let reference = version.as_ref();
        let directory = self.create_key_directory(&reference)?;
        let [blob_name, manifest_name, stage_name, _] = Self::filenames(&reference);
        let (manifest, manifest_bytes) = match directory
            .read_optional(&manifest_name, MAX_MANIFEST_BYTES)?
        {
            Some(bytes) => {
                let manifest: ArtifactManifest = Self::decode(&bytes, "artifact_manifest_invalid")?;
                if manifest.schema != MANIFEST_SCHEMA {
                    return Err(failed("artifact_manifest_schema_invalid"));
                }
                manifest.version.validate().map_err(|error| {
                    failed(format!("artifact_manifest_version_invalid:{error}"))
                })?;
                if manifest.version.as_ref() != reference {
                    return Err(PortError::Conflict("artifact_version_conflict".to_owned()));
                }
                (manifest, bytes)
            }
            None => {
                let manifest = ArtifactManifest {
                    schema: MANIFEST_SCHEMA.to_owned(),
                    version,
                };
                let bytes = Self::encode(&manifest, "artifact_manifest_encode_failed")?;
                (manifest, bytes)
            }
        };
        if manifest_bytes.len() > MAX_MANIFEST_BYTES {
            return Err(failed("artifact_manifest_size_exceeded"));
        }
        let manifest_digest = sha256(&manifest_bytes);
        if let Some(existing) = directory.read_optional(&blob_name, MAX_ARTIFACT_BYTES)? {
            if existing != content {
                return Err(PortError::Conflict(
                    "artifact_content_hash_mismatch".to_owned(),
                ));
            }
        } else {
            directory.publish_immutable(&blob_name, &content, "artifact_version_conflict")?;
        }
        if directory
            .read_optional(&manifest_name, MAX_MANIFEST_BYTES)?
            .is_none()
        {
            directory.publish_immutable(
                &manifest_name,
                &manifest_bytes,
                "artifact_version_conflict",
            )?;
        }
        let marker = Self::encode(
            &ArtifactMarker {
                schema: STAGE_SCHEMA.to_owned(),
                manifest_sha256: manifest_digest,
            },
            "artifact_stage_marker_encode_failed",
        )?;
        directory.publish_immutable(&stage_name, &marker, "artifact_version_conflict")?;
        Ok(manifest.version)
    }

    fn stage_sync(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactRef, PortError> {
        let reference = version.as_ref();
        let directory = self.create_key_directory(&reference)?;
        let filenames = Self::filenames(&reference);
        let manifest_name = &filenames[1];
        if directory
            .read_optional(&manifest_name, MAX_MANIFEST_BYTES)?
            .is_some()
        {
            return Err(PortError::Conflict(
                "artifact_version_already_staged".to_owned(),
            ));
        }
        Self::stage_version_sync(self, version, content).map(|version| version.as_ref())
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
        // A version revoked while still staged must not become readable by a later commit.
        self.check_not_revoked(&reference)?;
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
        directory.publish_immutable(&commit_name, &marker, "artifact_commit_conflict")
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
            Self {
                root: Arc::new(root),
            }
            .stage_sync(version, content)
        })
        .await
        .map_err(|error| failed(format!("result_unknown:artifact_stage_join_failed:{error}")))?
    }

    async fn stage_artifact_version(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactVersion, PortError> {
        Self::validate_content(&version, &content)?;
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .stage_version_sync(version, content)
        })
        .await
        .map_err(|error| failed(format!("result_unknown:artifact_stage_join_failed:{error}")))?
    }

    async fn commit_artifact(
        &self,
        reference: ArtifactRef,
        expected_revision: Option<u64>,
    ) -> Result<(), PortError> {
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .commit_sync(reference, expected_revision)
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
            Self {
                root: Arc::new(root),
            }
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
            Self {
                root: Arc::new(root),
            }
            .read_committed_bytes(&reference)
            .map(|_| ())
        })
        .await
        .map_err(|error| failed(format!("artifact_verify_join_failed:{error}")))?
    }

    async fn invalidate_artifact(
        &self,
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        let reference = reference.clone();
        let tombstone_digest = tombstone_digest.to_owned();
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .invalidate_sync(
                &reference,
                previous_epoch,
                data_epoch,
                &tombstone_digest,
                observed_at_ms,
            )
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_invalidation_join_failed:{error}"
            ))
        })?
    }

    async fn record_publication_intent(
        &self,
        intent: &ArtifactPublicationIntent,
    ) -> Result<(), PortError> {
        let intent = intent.clone();
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .record_publication_intent_sync(&intent)
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_publication_journal_join_failed:{error}"
            ))
        })?
    }

    async fn resolve_publication_intent(
        &self,
        command_id: &str,
        state: ArtifactPublicationState,
        event_id: Option<EventId>,
        resolved_at_unix_ms: u64,
    ) -> Result<ArtifactPublicationIntent, PortError> {
        let command_id = command_id.to_owned();
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .resolve_publication_intent_sync(
                &command_id,
                state,
                event_id,
                resolved_at_unix_ms,
            )
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_publication_journal_join_failed:{error}"
            ))
        })?
    }

    async fn pending_publication_intents(
        &self,
    ) -> Result<Vec<ArtifactPublicationIntent>, PortError> {
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .pending_publication_intents_sync()
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_publication_journal_join_failed:{error}"
            ))
        })?
    }

    async fn publication_intents(&self) -> Result<Vec<ArtifactPublicationIntent>, PortError> {
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .publication_intents_sync()
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_publication_journal_join_failed:{error}"
            ))
        })?
    }

    async fn remove_orphan_blob(&self, reference: &ArtifactRef) -> Result<bool, PortError> {
        let reference = reference.clone();
        let root = self.root.try_clone()?;
        tokio::task::spawn_blocking(move || {
            Self {
                root: Arc::new(root),
            }
            .remove_orphan_blob_sync(&reference)
        })
        .await
        .map_err(|error| {
            failed(format!(
                "result_unknown:artifact_orphan_removal_join_failed:{error}"
            ))
        })?
    }
}

#[async_trait]
impl ArtifactContentPort for LocalArtifactStore {
    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        ArtifactStorePort::read_artifact(self, reference).await
    }
}
