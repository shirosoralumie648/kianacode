//! Backup manifest, chunk and integrity contract.
//!
//! A backup manifest describes what a quiesced snapshot contains and proves it can be replayed.
//! It carries identity and digests only: it never opens a store, reads a byte, restores a root or
//! schedules a backup. Manifests are verifiable, replayable and safe to export redacted, so an
//! operator can hand one to another instance without disclosing a path or a secret.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, DeploymentRevision, EventCursor, InstanceId,
    SchemaVersion, SecretScanChannel, StorageRootId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BACKUP_MANIFEST_SCHEMA: &str = "kiana.backup-manifest.v1";
pub const BACKUP_CHUNK_SCHEMA: &str = "kiana.backup-chunk.v1";
pub const BACKUP_SET_REF_SCHEMA: &str = "kiana.backup-set-ref.v1";
pub const BACKUP_INTEGRITY_SCHEMA: &str = "kiana.backup-integrity.v1";
pub const BACKUP_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_BACKUP_CHUNKS: usize = 4096;
pub const MAX_BACKUP_ID_TEXT: usize = 256;

/// How a chunk's integrity was established. `Unknown` is never acceptable: an unverified backup
/// must be reported as unverifiable, not treated as intact.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupIntegrity {
    /// Whole-chunk SHA-256 over the chunk bytes, bound to the manifest digest.
    ContentHashVerified,
    /// Chunk bytes were read and hashed, but the enclosing store was quiesced elsewhere.
    ContentHashOnly,
    /// The tail of the chunk could not be read to completion (a torn write).
    TornTail,
    /// Integrity was not established; the manifest is not usable for restore.
    Unknown,
}

impl BackupIntegrity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContentHashVerified => "content_hash_verified",
            Self::ContentHashOnly => "content_hash_only",
            Self::TornTail => "torn_tail",
            Self::Unknown => "unknown",
        }
    }

    /// Only a verified or content-hashed chunk can support a restore. Torn and unknown chunks
    /// make the whole manifest unusable rather than partially restorable.
    pub const fn is_restorable(self) -> bool {
        matches!(self, Self::ContentHashVerified | Self::ContentHashOnly)
    }
}

/// What a chunk holds. Content is described by kind and reference only, never by absolute path.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupChunkKind {
    /// EventLog records, including the write-ahead log tail.
    EventLog,
    /// ArtifactStore entries.
    Artifact,
    /// A projector or index checkpoint.
    Projection,
    /// The migration registry, so a restore knows which format the data is in.
    MigrationRegistry,
    /// A bounded config snapshot.
    Config,
}

impl BackupChunkKind {
    pub const ALL: [Self; 5] = [
        Self::EventLog,
        Self::Artifact,
        Self::Projection,
        Self::MigrationRegistry,
        Self::Config,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EventLog => "event_log",
            Self::Artifact => "artifact",
            Self::Projection => "projection",
            Self::MigrationRegistry => "migration_registry",
            Self::Config => "config",
        }
    }
}

/// One addressed piece of a backup. The reference is a logical, redacted identifier; a manifest
/// that carries an absolute path or a secret is rejected outright.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupChunk {
    pub schema: String,
    pub version: SchemaVersion,
    pub chunk_id: String,
    pub kind: BackupChunkKind,
    /// Logical, redacted reference such as `eventlog/0000012345`. Never a filesystem path.
    pub reference: String,
    pub byte_length: u64,
    pub chunk_digest: String,
    pub integrity: BackupIntegrity,
    pub chunk_digest_binding: String,
}

impl BackupChunk {
    pub fn new(
        chunk_id: impl Into<String>,
        kind: BackupChunkKind,
        reference: impl Into<String>,
        byte_length: u64,
        chunk_digest: impl Into<String>,
        integrity: BackupIntegrity,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: BACKUP_CHUNK_SCHEMA.to_owned(),
            version: BACKUP_VERSION,
            chunk_id: chunk_id.into(),
            kind,
            reference: reference.into(),
            byte_length,
            chunk_digest: chunk_digest.into(),
            integrity,
            chunk_digest_binding: String::new(),
        };
        value.chunk_digest_binding = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BACKUP_CHUNK_SCHEMA
            || self.version != BACKUP_VERSION
            || self.byte_length == 0
        {
            return Err("backup_chunk_header_invalid".to_owned());
        }
        safe_text(&self.chunk_id, "backup_chunk_id")?;
        safe_reference(&self.reference)?;
        valid_digest(&self.chunk_digest, "backup_chunk_digest")?;
        valid_digest(&self.chunk_digest_binding, "backup_chunk_binding")?;
        if self.chunk_digest_binding != self.digest() {
            return Err("backup_chunk_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "chunk_id": self.chunk_id,
            "kind": self.kind,
            "reference": self.reference,
            "byte_length": self.byte_length,
            "chunk_digest": self.chunk_digest,
            "integrity": self.integrity,
        }))
    }
}

/// The verifiable description of one quiesced snapshot.
///
/// A manifest is restorable only when it carries a source cursor, a matching chunk set, a known
/// integrity for every chunk, and a configuration reference that pins the data/authority epochs it
/// was taken under.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub schema: String,
    pub version: SchemaVersion,
    pub backup_id: String,
    pub instance_id: InstanceId,
    pub storage_root: StorageRootId,
    pub revision_id: String,
    pub source_cursor: EventCursor,
    pub projection_generation: u64,
    pub data_epoch: u64,
    pub authority_epoch: u64,
    pub chunks: Vec<BackupChunk>,
    /// Digest of the config snapshot that was in force when the backup was taken.
    pub config_digest: String,
    pub manifest_digest: String,
}

impl BackupManifest {
    pub fn new(
        backup_id: impl Into<String>,
        instance_id: InstanceId,
        storage_root: StorageRootId,
        revision_id: impl Into<String>,
        source_cursor: EventCursor,
        projection_generation: u64,
        data_epoch: u64,
        authority_epoch: u64,
        chunks: Vec<BackupChunk>,
        config_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: BACKUP_MANIFEST_SCHEMA.to_owned(),
            version: BACKUP_VERSION,
            backup_id: backup_id.into(),
            instance_id,
            storage_root,
            revision_id: revision_id.into(),
            source_cursor,
            projection_generation,
            data_epoch,
            authority_epoch,
            chunks,
            config_digest: config_digest.into(),
            manifest_digest: String::new(),
        };
        value.manifest_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BACKUP_MANIFEST_SCHEMA
            || self.version != BACKUP_VERSION
            || self.source_cursor == 0
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.authority_epoch == 0
            || self.instance_id.as_uuid().is_nil()
            || self.storage_root.as_uuid().is_nil()
        {
            return Err("backup_manifest_header_invalid".to_owned());
        }
        safe_text(&self.backup_id, "backup_manifest_backup_id")?;
        safe_text(&self.revision_id, "backup_manifest_revision_id")?;
        valid_digest(&self.config_digest, "backup_manifest_config_digest")?;
        self.validate_chunks()?;
        valid_digest(&self.manifest_digest, "backup_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("backup_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Chunks must be non-empty, bounded, unique, ordered, and cover every kind that the source
    /// stores. A manifest missing the EventLog or migration registry cannot be restored.
    pub fn validate_chunks(&self) -> Result<(), String> {
        if self.chunks.is_empty() || self.chunks.len() > MAX_BACKUP_CHUNKS {
            return Err("backup_manifest_chunk_count_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        let mut references = BTreeSet::new();
        let mut kinds = BTreeSet::new();
        let mut previous_id = String::new();
        for chunk in &self.chunks {
            chunk.validate()?;
            if !ids.insert(chunk.chunk_id.clone())
                || !references.insert((chunk.kind, chunk.reference.clone()))
            {
                return Err("backup_manifest_chunk_duplicate".to_owned());
            }
            if !previous_id.is_empty() && chunk.chunk_id < previous_id {
                return Err("backup_manifest_chunk_order_invalid".to_owned());
            }
            previous_id.clone_from(&chunk.chunk_id);
            kinds.insert(chunk.kind);
        }
        for required in [
            BackupChunkKind::EventLog,
            BackupChunkKind::MigrationRegistry,
        ] {
            if !kinds.contains(&required) {
                return Err(format!("backup_manifest_missing_{}", required.as_str()));
            }
        }
        Ok(())
    }

    /// A manifest is restorable only if every chunk is intact. Torn or unknown integrity makes the
    /// whole backup unusable; it never degrades to a partial restore.
    pub fn restorable(&self) -> Result<(), String> {
        self.validate()?;
        if let Some(chunk) = self
            .chunks
            .iter()
            .find(|chunk| !chunk.integrity.is_restorable())
        {
            return Err(format!(
                "backup_manifest_integrity_{}",
                chunk.integrity.as_str()
            ));
        }
        Ok(())
    }

    /// Bind a manifest to the deployment revision it was taken under. A backup taken under a
    /// different instance, storage root, data epoch or authority epoch cannot be activated into
    /// this one.
    pub fn validate_against_revision(&self, revision: &DeploymentRevision) -> Result<(), String> {
        self.validate()?;
        if self.instance_id != revision.instance_id
            || self.storage_root != revision.storage_root
            || self.revision_id != revision.revision_id
            || self.data_epoch != revision.data_epoch
            || self.authority_epoch != revision.authority_epoch
        {
            return Err("backup_manifest_revision_binding_invalid".to_owned());
        }
        Ok(())
    }

    /// A redacted export that is safe to hand to another operator or attach to an incident. It
    /// drops chunk references and keeps only counts, digests and epochs, so no path or content
    /// leaves the instance.
    pub fn export_redacted(&self) -> Result<serde_json::Value, String> {
        self.validate()?;
        let mut by_kind: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
        for chunk in &self.chunks {
            *by_kind.entry(chunk.kind.as_str()).or_default() += 1;
        }
        Ok(serde_json::json!({
            "schema": BACKUP_SET_REF_SCHEMA,
            "version": self.version,
            "backup_id": self.backup_id,
            "source_cursor": self.source_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "chunk_count": self.chunks.len(),
            "chunk_kinds": by_kind,
            "config_digest": self.config_digest,
            "manifest_digest": self.manifest_digest,
        }))
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "backup_id": self.backup_id,
            "instance_id": self.instance_id,
            "storage_root": self.storage_root,
            "revision_id": self.revision_id,
            "source_cursor": self.source_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "chunks": self.chunks,
            "config_digest": self.config_digest,
        }))
    }
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_BACKUP_ID_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

/// A chunk reference is a logical relative name. Absolute paths, traversal, drive letters and
/// scheme prefixes are rejected so a manifest can never carry a filesystem location.
fn safe_reference(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_BACKUP_ID_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("://")
        || value.contains('\\')
        || value.starts_with('/')
        || value.contains(':')
    {
        return Err("backup_chunk_reference_invalid".to_owned());
    }
    for token in value.split('/') {
        if token.is_empty()
            || token == "."
            || token.starts_with('/')
            || token.chars().any(char::is_whitespace)
        {
            return Err("backup_chunk_reference_invalid".to_owned());
        }
    }
    safe_text(value, "backup_chunk_reference")
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
