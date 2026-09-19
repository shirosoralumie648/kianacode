//! Workspace/artifact snapshot contracts for bounded, identity-checked reads.
//!
//! A snapshot is evidence about a read attempt, not an authority or an instruction source. A
//! caller must preserve the before/after identity and trust disposition; stable content from an
//! untrusted project is metadata-only and cannot be promoted to model instructions.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const WORKSPACE_FILE_SNAPSHOT_SCHEMA: &str = "kiana.workspace-file-snapshot.v1";
pub const WORKSPACE_SNAPSHOT_SCHEMA: &str = "kiana.workspace-snapshot.v1";
pub const WORKSPACE_SNAPSHOT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_WORKSPACE_SNAPSHOT_FILES: u64 = 4_096;
pub const MAX_WORKSPACE_SNAPSHOT_LIMITATIONS: usize = 16;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTrust {
    Trusted,
    Untrusted,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEntryKind {
    File,
    Symlink,
    Hardlink,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceReadDisposition {
    Indexed,
    MetadataOnly,
    Skipped,
    Fenced,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshotLimits {
    pub max_files: u64,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
    pub max_depth: u32,
    pub max_elapsed_ms: u64,
}

impl WorkspaceSnapshotLimits {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_files == 0
            || self.max_files > MAX_WORKSPACE_SNAPSHOT_FILES
            || self.max_total_bytes == 0
            || self.max_file_bytes == 0
            || self.max_file_bytes > self.max_total_bytes
            || self.max_depth == 0
            || self.max_elapsed_ms == 0
        {
            return Err("workspace_snapshot_limits_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileIdentity {
    #[serde(default)]
    pub device: Option<u64>,
    #[serde(default)]
    pub inode: Option<u64>,
    pub size_bytes: u64,
    #[serde(default)]
    pub modified_unix_ms: Option<u64>,
    #[serde(default)]
    pub content_digest: Option<String>,
    #[serde(default)]
    pub hard_link_count: Option<u64>,
}

impl WorkspaceFileIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if self.hard_link_count == Some(0) {
            return Err("workspace_file_identity_hard_link_count_invalid".to_owned());
        }
        if let Some(content_digest) = &self.content_digest {
            digest(content_digest, "workspace_file_identity_content_digest")?;
        }
        Ok(())
    }

    /// Compare filesystem identity only; the content digest is intentionally excluded.
    pub fn same_file(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.size_bytes == other.size_bytes
            && self.modified_unix_ms == other.modified_unix_ms
            && self.hard_link_count == other.hard_link_count
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileSnapshot {
    pub schema: String,
    pub path: String,
    pub kind: WorkspaceEntryKind,
    pub trust: WorkspaceTrust,
    pub identity_before: WorkspaceFileIdentity,
    #[serde(default)]
    pub identity_after: Option<WorkspaceFileIdentity>,
    pub bytes_read: u64,
    pub disposition: WorkspaceReadDisposition,
    pub instruction_safe: bool,
    #[serde(default)]
    pub reason: Option<String>,
    pub file_digest: String,
}

impl WorkspaceFileSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKSPACE_FILE_SNAPSHOT_SCHEMA
            || self.path.trim().is_empty()
            || self.path.len() > 4_096
            || self.path.starts_with('/')
            || self
                .path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err("workspace_file_snapshot_header_invalid".to_owned());
        }
        self.identity_before.validate()?;
        if let Some(identity_after) = &self.identity_after {
            identity_after.validate()?;
        }
        digest(&self.file_digest, "workspace_file_snapshot_digest")?;
        if let Some(reason) = &self.reason {
            required(reason, "workspace_file_snapshot_reason", 256)?;
        }
        let identity_stable = self
            .identity_after
            .as_ref()
            .is_some_and(|after| self.identity_before.same_file(after));
        let expected_safe = self.trust == WorkspaceTrust::Trusted
            && self.kind == WorkspaceEntryKind::File
            && self.disposition == WorkspaceReadDisposition::Indexed
            && identity_stable;
        if self.instruction_safe != expected_safe {
            return Err("workspace_file_snapshot_instruction_safety_mismatch".to_owned());
        }
        match self.disposition {
            WorkspaceReadDisposition::Indexed => {
                if !expected_safe
                    || self.identity_after.is_none()
                    || self
                        .identity_after
                        .as_ref()
                        .and_then(|value| value.content_digest.as_ref())
                        .is_none()
                {
                    return Err("workspace_file_snapshot_indexed_evidence_missing".to_owned());
                }
            }
            WorkspaceReadDisposition::Fenced => {
                if self.identity_after.is_none() || identity_stable {
                    return Err("workspace_file_snapshot_fence_invalid".to_owned());
                }
            }
            WorkspaceReadDisposition::MetadataOnly | WorkspaceReadDisposition::Skipped => {
                if self.reason.as_deref().is_none_or(str::is_empty) {
                    return Err("workspace_file_snapshot_reason_required".to_owned());
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub root: String,
    pub trust: WorkspaceTrust,
    pub limits: WorkspaceSnapshotLimits,
    pub files: Vec<WorkspaceFileSnapshot>,
    pub total_bytes_read: u64,
    pub dirty: bool,
    pub scan_limited: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub snapshot_digest: String,
}

impl WorkspaceSnapshot {
    pub fn new(
        root: impl Into<String>,
        trust: WorkspaceTrust,
        limits: WorkspaceSnapshotLimits,
        files: Vec<WorkspaceFileSnapshot>,
        scan_limited: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let total_bytes_read = files.iter().map(|file| file.bytes_read).sum();
        let dirty = files
            .iter()
            .any(|file| file.disposition == WorkspaceReadDisposition::Fenced);
        let mut snapshot = Self {
            schema: WORKSPACE_SNAPSHOT_SCHEMA.to_owned(),
            version: WORKSPACE_SNAPSHOT_VERSION,
            root: root.into(),
            trust,
            limits,
            files,
            total_bytes_read,
            dirty,
            scan_limited,
            limitations,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKSPACE_SNAPSHOT_SCHEMA
            || self.version != WORKSPACE_SNAPSHOT_VERSION
            || self.root.trim().is_empty()
            || self.root.len() > 4_096
            || self.files.len() as u64 > self.limits.max_files
            || self.total_bytes_read > self.limits.max_total_bytes
            || self.limitations.len() > MAX_WORKSPACE_SNAPSHOT_LIMITATIONS
        {
            return Err("workspace_snapshot_header_invalid".to_owned());
        }
        self.limits.validate()?;
        if self.scan_limited && self.limitations.is_empty() {
            return Err("workspace_snapshot_limitation_reason_required".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "workspace_snapshot_limitation", 256)?;
        }
        let mut paths = BTreeSet::new();
        for file in &self.files {
            file.validate()?;
            if !paths.insert(file.path.clone()) {
                return Err("workspace_snapshot_duplicate_path".to_owned());
            }
            if file.bytes_read > self.limits.max_file_bytes {
                return Err("workspace_snapshot_file_limit_exceeded".to_owned());
            }
        }
        let dirty = self
            .files
            .iter()
            .any(|file| file.disposition == WorkspaceReadDisposition::Fenced);
        if self.dirty != dirty {
            return Err("workspace_snapshot_dirty_mismatch".to_owned());
        }
        digest(&self.snapshot_digest, "workspace_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("workspace_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "root": self.root,
            "trust": self.trust,
            "limits": self.limits,
            "files": self.files,
            "total_bytes_read": self.total_bytes_read,
            "dirty": self.dirty,
            "scan_limited": self.scan_limited,
            "limitations": self.limitations,
        }))
    }
}
