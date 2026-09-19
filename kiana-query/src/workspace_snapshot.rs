//! Bounded workspace snapshot and identity-checked reads.
//!
//! This adapter only reads files. It never grants a capability or turns text into an authority
//! section. Untrusted/unknown roots produce metadata-only rows; trusted reads compare file
//! identity before and after the bounded read and fence a changed file.

use anyhow::{anyhow, Context, Result};
use kiana_domain::{
    json_digest, WorkspaceEntryKind, WorkspaceFileIdentity, WorkspaceFileSnapshot,
    WorkspaceReadDisposition, WorkspaceSnapshot, WorkspaceSnapshotLimits, WorkspaceTrust,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_MAX_FILES: u64 = 4_096;
const DEFAULT_MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const DEFAULT_MAX_FILE_BYTES: u64 = 128 * 1024;
const DEFAULT_MAX_DEPTH: u32 = 16;
const DEFAULT_MAX_ELAPSED_MS: u64 = 5_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshotOptions {
    pub trust: WorkspaceTrust,
    pub limits: WorkspaceSnapshotLimits,
}

impl Default for WorkspaceSnapshotOptions {
    fn default() -> Self {
        Self {
            trust: WorkspaceTrust::Unknown,
            limits: WorkspaceSnapshotLimits {
                max_files: DEFAULT_MAX_FILES,
                max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
                max_file_bytes: DEFAULT_MAX_FILE_BYTES,
                max_depth: DEFAULT_MAX_DEPTH,
                max_elapsed_ms: DEFAULT_MAX_ELAPSED_MS,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReadContent {
    pub path: String,
    pub text: String,
    pub content_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReadOutcome {
    pub snapshot: WorkspaceSnapshot,
    pub contents: Vec<WorkspaceReadContent>,
}

struct ScanState {
    root: PathBuf,
    options: WorkspaceSnapshotOptions,
    started: std::time::Instant,
    files: Vec<WorkspaceFileSnapshot>,
    contents: Vec<WorkspaceReadContent>,
    limitations: Vec<String>,
    scan_limited: bool,
}

impl ScanState {
    fn elapsed(&self) -> bool {
        self.started.elapsed() >= Duration::from_millis(self.options.limits.max_elapsed_ms)
    }

    fn limit(&mut self, reason: &str) {
        self.scan_limited = true;
        if !self.limitations.iter().any(|item| item == reason) {
            self.limitations.push(reason.to_owned());
        }
    }

    fn can_visit(&mut self) -> bool {
        if self.elapsed() {
            self.limit("max_elapsed_ms");
            return false;
        }
        if self.files.len() as u64 >= self.options.limits.max_files {
            self.limit("max_files");
            return false;
        }
        true
    }
}

/// Scan a canonical workspace root with bounded metadata/content reads.
pub fn read_workspace_snapshot(
    root: impl AsRef<Path>,
    options: WorkspaceSnapshotOptions,
) -> Result<WorkspaceReadOutcome> {
    options.limits.validate().map_err(|error| anyhow!(error))?;
    let requested_root = root.as_ref();
    let root_metadata = fs::symlink_metadata(requested_root).with_context(|| {
        format!(
            "workspace root metadata failed: {}",
            requested_root.display()
        )
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(anyhow!("workspace root must be a real directory"));
    }
    let root = fs::canonicalize(requested_root).with_context(|| {
        format!(
            "workspace root canonicalize failed: {}",
            requested_root.display()
        )
    })?;
    let mut state = ScanState {
        root: root.clone(),
        options,
        started: std::time::Instant::now(),
        files: Vec::new(),
        contents: Vec::new(),
        limitations: Vec::new(),
        scan_limited: false,
    };
    walk_directory(&mut state, &root, 0)?;
    let snapshot = WorkspaceSnapshot::new(
        root.to_string_lossy().replace('\\', "/"),
        state.options.trust,
        state.options.limits,
        state.files,
        state.scan_limited,
        state.limitations,
    )?;
    Ok(WorkspaceReadOutcome {
        snapshot,
        contents: state.contents,
    })
}

fn walk_directory(state: &mut ScanState, directory: &Path, depth: u32) -> Result<()> {
    if !state.can_visit() {
        return Ok(());
    }
    if depth > state.options.limits.max_depth {
        state.limit("max_depth");
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("workspace directory read failed: {}", directory.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| {
            format!(
                "workspace directory enumeration failed: {}",
                directory.display()
            )
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if !state.can_visit() {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | ".kiana" | "target" | "node_modules") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("workspace entry metadata failed: {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            record_metadata_only(
                state,
                &path,
                &metadata,
                WorkspaceEntryKind::Symlink,
                "symlink_not_read",
            )?;
        } else if metadata.is_dir() {
            if depth >= state.options.limits.max_depth {
                state.limit("max_depth");
            } else {
                walk_directory(state, &path, depth + 1)?;
            }
        } else if metadata.is_file() {
            read_file(state, &path, &metadata)?;
        }
    }
    Ok(())
}

fn read_file(state: &mut ScanState, path: &Path, metadata_before: &fs::Metadata) -> Result<()> {
    let relative = relative_path(&state.root, path)?;
    let identity_before = identity(metadata_before, None);
    let hard_linked = hard_link_count(metadata_before).is_some_and(|count| count > 1);
    if hard_linked {
        return record_metadata_only(
            state,
            path,
            metadata_before,
            WorkspaceEntryKind::Hardlink,
            "hardlink_not_read",
        );
    }
    if state.options.trust != WorkspaceTrust::Trusted {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before.clone(),
            Some(identity_before),
            0,
            WorkspaceReadDisposition::MetadataOnly,
            false,
            Some(match state.options.trust {
                WorkspaceTrust::Untrusted => "untrusted_metadata_only",
                WorkspaceTrust::Unknown => "unknown_trust_metadata_only",
                WorkspaceTrust::Trusted => unreachable!(),
            }),
            digest_bytes(path.to_string_lossy().as_bytes()),
            None,
        );
    }
    if metadata_before.len() > state.options.limits.max_file_bytes {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before.clone(),
            Some(identity_before),
            0,
            WorkspaceReadDisposition::Skipped,
            false,
            Some("max_file_bytes"),
            digest_bytes(path.to_string_lossy().as_bytes()),
            None,
        );
    }
    let mut file = fs::File::open(path)
        .with_context(|| format!("workspace file open failed: {}", path.display()))?;
    file.seek(SeekFrom::Start(0))?;
    let read_limit = state.options.limits.max_file_bytes.saturating_add(1) as usize;
    let mut bytes = Vec::with_capacity(read_limit.min(metadata_before.len() as usize + 1));
    file.take(read_limit as u64).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > state.options.limits.max_file_bytes {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before.clone(),
            Some(identity_before),
            0,
            WorkspaceReadDisposition::Skipped,
            false,
            Some("max_file_bytes_after_open"),
            digest_bytes(path.to_string_lossy().as_bytes()),
            None,
        );
    }
    let metadata_after = fs::symlink_metadata(path).with_context(|| {
        format!(
            "workspace file post-read metadata failed: {}",
            path.display()
        )
    })?;
    let identity_after = identity(&metadata_after, None);
    let content_digest = digest_bytes(&bytes);
    if !identity_before.same_file(&identity_after) {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before,
            Some(identity_after),
            0,
            WorkspaceReadDisposition::Fenced,
            false,
            Some("workspace_change_between_scan_and_read"),
            content_digest,
            None,
        );
    }
    let Ok(text) = String::from_utf8(bytes.clone()) else {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before,
            Some(identity_after),
            bytes.len() as u64,
            WorkspaceReadDisposition::Skipped,
            false,
            Some("non_utf8_content"),
            content_digest,
            None,
        );
    };
    let mut identity_after = identity_after;
    identity_after.content_digest = Some(content_digest.clone());
    record_file(
        state,
        relative.clone(),
        WorkspaceEntryKind::File,
        identity_before,
        Some(identity_after),
        text.len() as u64,
        WorkspaceReadDisposition::Indexed,
        true,
        None,
        content_digest.clone(),
        Some(WorkspaceReadContent {
            path: relative,
            text,
            content_digest,
        }),
    )
}

fn record_metadata_only(
    state: &mut ScanState,
    path: &Path,
    metadata: &fs::Metadata,
    kind: WorkspaceEntryKind,
    reason: &'static str,
) -> Result<()> {
    record_file(
        state,
        relative_path(&state.root, path)?,
        kind,
        identity(metadata, None),
        None,
        0,
        WorkspaceReadDisposition::MetadataOnly,
        false,
        Some(reason),
        digest_bytes(path.to_string_lossy().as_bytes()),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn record_file(
    state: &mut ScanState,
    path: String,
    kind: WorkspaceEntryKind,
    identity_before: WorkspaceFileIdentity,
    identity_after: Option<WorkspaceFileIdentity>,
    bytes_read: u64,
    disposition: WorkspaceReadDisposition,
    instruction_safe: bool,
    reason: Option<&str>,
    file_digest: String,
    content: Option<WorkspaceReadContent>,
) -> Result<()> {
    let file = WorkspaceFileSnapshot {
        schema: kiana_domain::WORKSPACE_FILE_SNAPSHOT_SCHEMA.to_owned(),
        path,
        kind,
        trust: state.options.trust,
        identity_before,
        identity_after,
        bytes_read,
        disposition,
        instruction_safe,
        reason: reason.map(str::to_owned),
        file_digest,
    };
    file.validate().map_err(|error| anyhow!(error))?;
    if let Some(content) = content {
        if state.total_content_bytes() + content.text.len() as u64
            > state.options.limits.max_total_bytes
        {
            state.limit("max_total_bytes");
            return Ok(());
        }
        state.contents.push(content);
    }
    state.files.push(file);
    Ok(())
}

impl ScanState {
    fn total_content_bytes(&self) -> u64 {
        self.contents
            .iter()
            .map(|content| content.text.len() as u64)
            .sum()
    }
}

fn relative_path(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| anyhow!("workspace path escaped canonical root"))?
        .to_string_lossy()
        .replace('\\', "/");
    if relative.is_empty() {
        return Err(anyhow!("workspace relative path empty"));
    }
    Ok(relative)
}

fn identity(metadata: &fs::Metadata, content_digest: Option<String>) -> WorkspaceFileIdentity {
    WorkspaceFileIdentity {
        device: device(metadata),
        inode: inode(metadata),
        size_bytes: metadata.len(),
        modified_unix_ms: metadata.modified().ok().and_then(unix_ms),
        content_digest,
        hard_link_count: hard_link_count(metadata),
    }
}

fn unix_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(unix)]
fn device(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.dev())
}

#[cfg(not(unix))]
fn device(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn inode(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.ino())
}

#[cfg(not(unix))]
fn inode(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn hard_link_count(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.nlink())
}

#[cfg(not(unix))]
fn hard_link_count(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

/// Stable digest for a snapshot result, useful to bind it into SourceSnapshot provenance.
pub fn snapshot_digest(snapshot: &WorkspaceSnapshot) -> String {
    json_digest(&serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null))
}
