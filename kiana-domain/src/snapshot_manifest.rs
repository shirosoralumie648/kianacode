//! Sealed backup manifest contracts.
//!
//! This module plans and validates a backup boundary. It never copies files, stops a writer or
//! activates a restore root; those effects remain adapter/ControlPlane responsibilities.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SNAPSHOT_MANIFEST_SCHEMA: &str = "kiana.snapshot-manifest.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    Full,
    Incremental,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotState {
    Building,
    Sealed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotFileSeal {
    pub relative_path: String,
    pub size_bytes: u64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotManifest {
    pub schema: String,
    pub snapshot_id: String,
    pub owner_scope: String,
    pub store_id: String,
    pub instance_id: String,
    pub active_root: String,
    pub backup_root: String,
    pub mode: SnapshotMode,
    pub state: SnapshotState,
    pub source_cursor: u64,
    pub data_epoch: u64,
    pub previous_snapshot_id: Option<String>,
    pub quiesced: bool,
    pub wal_sealed: bool,
    pub files: Vec<SnapshotFileSeal>,
    pub manifest_digest: String,
}

impl SnapshotManifest {
    pub fn new(
        snapshot_id: impl Into<String>,
        owner_scope: impl Into<String>,
        store_id: impl Into<String>,
        instance_id: impl Into<String>,
        active_root: impl Into<String>,
        backup_root: impl Into<String>,
        mode: SnapshotMode,
        source_cursor: u64,
        data_epoch: u64,
        previous_snapshot_id: Option<String>,
        quiesced: bool,
        wal_sealed: bool,
        mut files: Vec<SnapshotFileSeal>,
    ) -> Result<Self, String> {
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut manifest = Self {
            schema: SNAPSHOT_MANIFEST_SCHEMA.to_owned(),
            snapshot_id: snapshot_id.into(),
            owner_scope: owner_scope.into(),
            store_id: store_id.into(),
            instance_id: instance_id.into(),
            active_root: active_root.into(),
            backup_root: backup_root.into(),
            mode,
            state: SnapshotState::Sealed,
            source_cursor,
            data_epoch,
            previous_snapshot_id,
            quiesced,
            wal_sealed,
            files,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SNAPSHOT_MANIFEST_SCHEMA
            || self.state != SnapshotState::Sealed
            || !self.quiesced
            || !self.wal_sealed
            || self.source_cursor == 0
            || self.data_epoch == 0
            || self.files.is_empty()
        {
            return Err("snapshot_manifest_seal_invalid".to_owned());
        }
        for (value, field) in [
            (&self.snapshot_id, "snapshot_id"),
            (&self.owner_scope, "snapshot_owner_scope"),
            (&self.store_id, "snapshot_store_id"),
            (&self.instance_id, "snapshot_instance_id"),
            (&self.active_root, "snapshot_active_root"),
            (&self.backup_root, "snapshot_backup_root"),
        ] {
            if value.trim().is_empty() || value.len() > 4096 || value.contains('\0') {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.active_root == self.backup_root
            || self
                .backup_root
                .starts_with(&(self.active_root.clone() + "/"))
        {
            return Err("snapshot_backup_root_overlaps_active_root".to_owned());
        }
        if self.mode == SnapshotMode::Full && self.previous_snapshot_id.is_some() {
            return Err("snapshot_full_previous_forbidden".to_owned());
        }
        if self.mode == SnapshotMode::Incremental && self.previous_snapshot_id.is_none() {
            return Err("snapshot_incremental_previous_required".to_owned());
        }
        let mut previous = String::new();
        for file in &self.files {
            if file.relative_path.is_empty()
                || file.relative_path.starts_with('/')
                || file.relative_path.contains("..")
                || file.relative_path <= previous
            {
                return Err("snapshot_file_path_invalid".to_owned());
            }
            if file.content_hash.strip_prefix("sha256:").is_none_or(|hex| {
                hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) {
                return Err("snapshot_file_hash_invalid".to_owned());
            }
            previous = file.relative_path.clone();
        }
        if self
            .manifest_digest
            .strip_prefix("sha256:")
            .is_none_or(|hex| hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("snapshot_manifest_digest_invalid".to_owned());
        }
        if self.manifest_digest != self.digest() {
            return Err("snapshot_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "snapshot_id": self.snapshot_id,
            "owner_scope": self.owner_scope,
            "store_id": self.store_id,
            "instance_id": self.instance_id,
            "active_root": self.active_root,
            "backup_root": self.backup_root,
            "mode": self.mode,
            "state": self.state,
            "source_cursor": self.source_cursor,
            "data_epoch": self.data_epoch,
            "previous_snapshot_id": self.previous_snapshot_id,
            "quiesced": self.quiesced,
            "wal_sealed": self.wal_sealed,
            "files": self.files,
        }))
    }
}
