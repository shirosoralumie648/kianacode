//! Codex-compatible apply_patch parser/applier owned by the daemon.
//!
//! Markers and hunk grammar are derived from OpenAI Codex (Apache-2.0)
//! `codex-rs/apply-patch`. Copied into Kiana; `reference/` is audit-only.
//!
//! Parse, path confinement, and hunk application are preflighted against an
//! in-memory overlay before any user-visible write. Single-file updates replace
//! via a same-directory temp file and rename.

use kiana_ports::PortError;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

const BEGIN_PATCH: &str = "*** Begin Patch";
const END_PATCH: &str = "*** End Patch";
const ADD_FILE: &str = "*** Add File: ";
const DELETE_FILE: &str = "*** Delete File: ";
const UPDATE_FILE: &str = "*** Update File: ";
const MOVE_TO: &str = "*** Move to: ";
const EOF_MARKER: &str = "*** End of File";
const TEMP_SIBLING_ATTEMPTS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Hunk {
    AddFile {
        path: PathBuf,
        contents: String,
    },
    DeleteFile {
        path: PathBuf,
    },
    UpdateFile {
        path: PathBuf,
        move_path: Option<PathBuf>,
        chunks: Vec<UpdateChunk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateChunk {
    change_context: Option<String>,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OverlayFile {
    Present(String),
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlannedOp {
    Add {
        target: PathBuf,
        contents: String,
    },
    Delete {
        target: PathBuf,
    },
    Update {
        target: PathBuf,
        contents: String,
    },
    Move {
        source: PathBuf,
        destination: PathBuf,
        contents: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedPatch {
    operations: Vec<PlannedOp>,
    preconditions: Vec<PathPrecondition>,
}

#[cfg(target_os = "linux")]
type CommitDirectories = Vec<(PathBuf, File)>;

#[cfg(not(target_os = "linux"))]
type CommitDirectories = ();

#[derive(Debug)]
struct ProjectPatchLock {
    _file: File,
}

impl ProjectPatchLock {
    fn acquire(project_root: &Path) -> Result<Self, PortError> {
        let lock_dir = std::env::var_os("KIANA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".kiana")))
            .unwrap_or_else(|| PathBuf::from(".kiana"))
            .join("locks");
        fs::create_dir_all(&lock_dir).map_err(|_| failed("apply_patch_lock_unavailable"))?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        project_root.display().to_string().hash(&mut hasher);
        let path = lock_dir.join(format!("patch-{:#016x}.lock", hasher.finish()));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| failed("apply_patch_lock_unavailable"))?;
        lock_project_patch(&file)?;
        Ok(Self { _file: file })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathPrecondition {
    path: PathBuf,
    snapshot: PathSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSnapshot {
    Missing,
    Present {
        fingerprint: MetadataFingerprint,
        contents: Option<Vec<u8>>,
        readonly: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataFingerprint {
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

pub fn apply_codex_patch(project_root: &Path, patch: &str) -> Result<Value, PortError> {
    let project_root = project_root
        .canonicalize()
        .map_err(|error| failed(format!("apply_patch_project_root_invalid:{error}")))?;
    if !project_root.is_dir() {
        return Err(failed("apply_patch_project_root_invalid:not_directory"));
    }
    let hunks = parse_patch(patch)?;
    if hunks.is_empty() {
        return Err(failed("apply_patch_empty"));
    }
    let _lock = ProjectPatchLock::acquire(&project_root)?;
    let planned = plan_hunks(&project_root, hunks)?;
    commit_planned(&project_root, &planned)
}

#[cfg(unix)]
fn lock_project_patch(file: &File) -> Result<(), PortError> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result == 0 {
        Ok(())
    } else {
        Err(failed("apply_patch_lock_unavailable"))
    }
}

#[cfg(not(unix))]
fn lock_project_patch(_file: &File) -> Result<(), PortError> {
    Err(failed("apply_patch_lock_unavailable"))
}

fn parse_patch(patch: &str) -> Result<Vec<Hunk>, PortError> {
    let normalized = patch.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    if lines.first().map(|line| line.trim()) != Some(BEGIN_PATCH) {
        return Err(failed("apply_patch_missing_begin"));
    }

    let mut hunks = Vec::new();
    let mut index = 1;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.is_empty() {
            index += 1;
            continue;
        }
        if trimmed == END_PATCH {
            break;
        }
        if let Some(path) = trimmed.strip_prefix(ADD_FILE) {
            let path = PathBuf::from(path.trim());
            index += 1;
            let mut contents = String::new();
            while index < lines.len() {
                let line = lines[index];
                let next = line.trim_start();
                if next.starts_with("*** ") {
                    break;
                }
                let body = line
                    .strip_prefix('+')
                    .ok_or_else(|| failed("apply_patch_add_line_missing_plus"))?;
                contents.push_str(body);
                contents.push('\n');
                index += 1;
            }
            hunks.push(Hunk::AddFile { path, contents });
            continue;
        }
        if let Some(path) = trimmed.strip_prefix(DELETE_FILE) {
            hunks.push(Hunk::DeleteFile {
                path: PathBuf::from(path.trim()),
            });
            index += 1;
            continue;
        }
        if let Some(path) = trimmed.strip_prefix(UPDATE_FILE) {
            let path = PathBuf::from(path.trim());
            index += 1;
            let mut move_path = None;
            if index < lines.len() {
                if let Some(moved) = lines[index].trim().strip_prefix(MOVE_TO) {
                    move_path = Some(PathBuf::from(moved.trim()));
                    index += 1;
                }
            }
            let mut chunks = Vec::new();
            while index < lines.len() {
                let marker = lines[index].trim();
                if marker.starts_with("*** ") && marker != EOF_MARKER {
                    break;
                }
                if marker == "@@" || marker.starts_with("@@ ") {
                    let change_context = marker
                        .strip_prefix("@@ ")
                        .map(str::to_owned)
                        .filter(|value| !value.is_empty());
                    index += 1;
                    let mut chunk = UpdateChunk {
                        change_context,
                        old_lines: Vec::new(),
                        new_lines: Vec::new(),
                    };
                    while index < lines.len() {
                        let line = lines[index];
                        let trimmed_line = line.trim();
                        if trimmed_line == "@@"
                            || trimmed_line.starts_with("@@ ")
                            || (trimmed_line.starts_with("*** ") && trimmed_line != EOF_MARKER)
                        {
                            break;
                        }
                        if trimmed_line == EOF_MARKER {
                            index += 1;
                            break;
                        }
                        if let Some(body) = line.strip_prefix('+') {
                            chunk.new_lines.push(body.to_owned());
                        } else if let Some(body) = line.strip_prefix('-') {
                            chunk.old_lines.push(body.to_owned());
                        } else if let Some(body) = line.strip_prefix(' ') {
                            chunk.old_lines.push(body.to_owned());
                            chunk.new_lines.push(body.to_owned());
                        } else if line.is_empty() {
                            chunk.old_lines.push(String::new());
                            chunk.new_lines.push(String::new());
                        } else {
                            return Err(failed("apply_patch_update_line_invalid"));
                        }
                        index += 1;
                    }
                    chunks.push(chunk);
                    continue;
                }
                if marker.is_empty() {
                    index += 1;
                    continue;
                }
                return Err(failed("apply_patch_update_hunk_invalid"));
            }
            hunks.push(Hunk::UpdateFile {
                path,
                move_path,
                chunks,
            });
            continue;
        }
        return Err(failed("apply_patch_hunk_invalid"));
    }
    Ok(hunks)
}

fn plan_hunks(project_root: &Path, hunks: Vec<Hunk>) -> Result<PlannedPatch, PortError> {
    let mut overlay = HashMap::new();
    let mut operations = Vec::new();
    for hunk in hunks {
        operations.push(plan_hunk(project_root, hunk, &mut overlay)?);
    }
    let preconditions = capture_preconditions(project_root, &operations)?;
    Ok(PlannedPatch {
        operations,
        preconditions,
    })
}

fn plan_hunk(
    project_root: &Path,
    hunk: Hunk,
    overlay: &mut HashMap<PathBuf, OverlayFile>,
) -> Result<PlannedOp, PortError> {
    match hunk {
        Hunk::AddFile { path, contents } => plan_add(project_root, path, contents, overlay),
        Hunk::DeleteFile { path } => plan_delete(project_root, path, overlay),
        Hunk::UpdateFile {
            path,
            move_path,
            chunks,
        } => plan_update(project_root, path, move_path, chunks, overlay),
    }
}

fn plan_add(
    root: &Path,
    path: PathBuf,
    contents: String,
    overlay: &mut HashMap<PathBuf, OverlayFile>,
) -> Result<PlannedOp, PortError> {
    let target = planned_new_path(root, &path, overlay)?;
    overlay.insert(target.clone(), OverlayFile::Present(contents.clone()));
    Ok(PlannedOp::Add { target, contents })
}

fn plan_delete(
    root: &Path,
    path: PathBuf,
    overlay: &mut HashMap<PathBuf, OverlayFile>,
) -> Result<PlannedOp, PortError> {
    let target = planned_existing_path(root, &path, overlay)?;
    overlay.insert(target.clone(), OverlayFile::Deleted);
    Ok(PlannedOp::Delete { target })
}

fn planned_existing_path(
    root: &Path,
    relative: &Path,
    overlay: &HashMap<PathBuf, OverlayFile>,
) -> Result<PathBuf, PortError> {
    let candidate = confined_candidate(root, relative)?;
    if let Some(file) = overlay_file(overlay, &candidate) {
        return match file {
            OverlayFile::Present(_) => Ok(candidate),
            OverlayFile::Deleted => Err(failed("apply_patch_path_not_file")),
        };
    }
    let target = confined_existing_file(root, relative)?;
    if let Some(file) = overlay_file(overlay, &target) {
        return match file {
            OverlayFile::Present(_) => Ok(target),
            OverlayFile::Deleted => Err(failed("apply_patch_path_not_file")),
        };
    }
    Ok(target)
}

fn plan_update(
    root: &Path,
    path: PathBuf,
    move_path: Option<PathBuf>,
    chunks: Vec<UpdateChunk>,
    overlay: &mut HashMap<PathBuf, OverlayFile>,
) -> Result<PlannedOp, PortError> {
    let (source, original) = planned_existing(root, &path, overlay)?;
    let contents = apply_chunks(&original, &chunks)?;
    match move_path {
        Some(moved) => plan_move(root, source, moved, contents, overlay),
        None => {
            overlay.insert(source.clone(), OverlayFile::Present(contents.clone()));
            Ok(PlannedOp::Update {
                target: source,
                contents,
            })
        }
    }
}

fn plan_move(
    root: &Path,
    source: PathBuf,
    moved: PathBuf,
    contents: String,
    overlay: &mut HashMap<PathBuf, OverlayFile>,
) -> Result<PlannedOp, PortError> {
    let destination = planned_new_path(root, &moved, overlay)?;
    overlay.insert(source.clone(), OverlayFile::Deleted);
    overlay.insert(destination.clone(), OverlayFile::Present(contents.clone()));
    Ok(PlannedOp::Move {
        source,
        destination,
        contents,
    })
}

fn planned_existing(
    root: &Path,
    relative: &Path,
    overlay: &HashMap<PathBuf, OverlayFile>,
) -> Result<(PathBuf, String), PortError> {
    let candidate = confined_candidate(root, relative)?;
    if let Some(file) = overlay_file(overlay, &candidate) {
        return overlay_present(candidate, file);
    }
    let target = confined_existing_file(root, relative)?;
    if let Some(file) = overlay_file(overlay, &target) {
        return overlay_present(target, file);
    }
    let contents = fs::read_to_string(&target).map_err(io_failed)?;
    Ok((target, contents))
}

fn planned_new_path(
    root: &Path,
    relative: &Path,
    overlay: &HashMap<PathBuf, OverlayFile>,
) -> Result<PathBuf, PortError> {
    let candidate = confined_candidate(root, relative)?;
    match overlay_file(overlay, &candidate) {
        Some(OverlayFile::Present(_)) => Err(failed("apply_patch_add_exists")),
        Some(OverlayFile::Deleted) => Ok(candidate),
        None => new_path_from_disk(root, relative),
    }
}

fn new_path_from_disk(root: &Path, relative: &Path) -> Result<PathBuf, PortError> {
    let target = confined_new_file(root, relative)?;
    if target.exists() {
        Err(failed("apply_patch_add_exists"))
    } else {
        Ok(target)
    }
}

fn overlay_file<'a>(
    overlay: &'a HashMap<PathBuf, OverlayFile>,
    path: &Path,
) -> Option<&'a OverlayFile> {
    overlay.get(path).or_else(|| {
        path.canonicalize()
            .ok()
            .and_then(|canonical| overlay.get(&canonical))
    })
}

fn overlay_present(path: PathBuf, file: &OverlayFile) -> Result<(PathBuf, String), PortError> {
    match file {
        OverlayFile::Present(contents) => Ok((path, contents.clone())),
        OverlayFile::Deleted => Err(failed("apply_patch_path_not_file")),
    }
}

#[cfg(target_os = "linux")]
fn open_commit_directories(
    preconditions: &[PathPrecondition],
) -> Result<CommitDirectories, PortError> {
    let mut directories = Vec::new();
    for precondition in preconditions {
        let PathSnapshot::Present { fingerprint, .. } = &precondition.snapshot else {
            continue;
        };
        if !fingerprint.is_dir {
            continue;
        }
        let name = std::ffi::CString::new(precondition.path.as_os_str().as_bytes())
            .map_err(|_| failed("apply_patch_path_invalid"))?;
        let fd = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io_failed(std::io::Error::last_os_error()));
        }
        directories.push((precondition.path.clone(), unsafe { File::from_raw_fd(fd) }));
    }
    Ok(directories)
}

#[cfg(not(target_os = "linux"))]
fn open_commit_directories(
    _preconditions: &[PathPrecondition],
) -> Result<CommitDirectories, PortError> {
    Ok(())
}

fn commit_planned(project_root: &Path, planned: &PlannedPatch) -> Result<Value, PortError> {
    let directories = open_commit_directories(&planned.preconditions)?;
    verify_preconditions(&planned.preconditions)?;
    let mut changed = Vec::new();
    for operation in &planned.operations {
        match commit_op(project_root, operation, &directories) {
            Ok(change) => changed.push(change),
            Err(error) => {
                return match rollback_preconditions(&planned.preconditions) {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(failed(format!(
                        "apply_patch_rollback_failed:{error};{rollback_error}"
                    ))),
                };
            }
        }
    }
    Ok(json!({ "changed": changed }))
}

fn capture_preconditions(
    project_root: &Path,
    operations: &[PlannedOp],
) -> Result<Vec<PathPrecondition>, PortError> {
    let mut paths = Vec::new();
    for operation in operations {
        let operation_paths = match operation {
            PlannedOp::Add { target, .. }
            | PlannedOp::Delete { target }
            | PlannedOp::Update { target, .. } => {
                vec![target]
            }
            PlannedOp::Move {
                source,
                destination,
                ..
            } => vec![source, destination],
        };
        for path in operation_paths {
            let mut current = Some(path.as_path());
            while let Some(candidate) = current {
                if !candidate.starts_with(project_root) {
                    break;
                }
                if !paths.iter().any(|existing: &PathBuf| existing == candidate) {
                    paths.push(candidate.to_path_buf());
                }
                if candidate == project_root {
                    break;
                }
                current = candidate.parent();
            }
        }
    }
    paths.sort_by_key(|path| path.components().count());
    paths
        .into_iter()
        .map(|path| {
            let snapshot = snapshot_path(&path)?;
            Ok(PathPrecondition { path, snapshot })
        })
        .collect()
}

fn verify_preconditions(preconditions: &[PathPrecondition]) -> Result<(), PortError> {
    for precondition in preconditions {
        let current = snapshot_path(&precondition.path)?;
        if !same_snapshot(&current, &precondition.snapshot) {
            return Err(failed(format!(
                "apply_patch_path_changed:{}",
                precondition.path.display()
            )));
        }
    }
    Ok(())
}

fn same_snapshot(current: &PathSnapshot, expected: &PathSnapshot) -> bool {
    match (current, expected) {
        (PathSnapshot::Missing, PathSnapshot::Missing) => true,
        (
            PathSnapshot::Present {
                fingerprint: current,
                ..
            },
            PathSnapshot::Present {
                fingerprint: expected,
                ..
            },
        ) => current == expected,
        _ => false,
    }
}

fn snapshot_path(path: &Path) -> Result<PathSnapshot, PortError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PathSnapshot::Missing);
        }
        Err(error) => return Err(io_failed(error)),
    };
    let contents = if metadata.is_file() {
        Some(fs::read(path).map_err(io_failed)?)
    } else {
        None
    };
    Ok(PathSnapshot::Present {
        fingerprint: metadata_fingerprint(&metadata),
        contents,
        readonly: metadata.permissions().readonly(),
    })
}

fn metadata_fingerprint(metadata: &fs::Metadata) -> MetadataFingerprint {
    MetadataFingerprint {
        is_file: metadata.is_file(),
        is_dir: metadata.is_dir(),
        is_symlink: metadata.file_type().is_symlink(),
        len: metadata.len(),
        modified: metadata.modified().ok(),
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
    }
}

fn rollback_preconditions(preconditions: &[PathPrecondition]) -> Result<(), PortError> {
    let mut ordered = preconditions.to_vec();
    ordered.sort_by_key(|precondition| std::cmp::Reverse(precondition.path.components().count()));
    for precondition in ordered {
        restore_snapshot(&precondition.path, &precondition.snapshot)?;
    }
    Ok(())
}

fn restore_snapshot(path: &Path, snapshot: &PathSnapshot) -> Result<(), PortError> {
    match snapshot {
        PathSnapshot::Missing => match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir(path).map_err(io_failed),
            Ok(_) => fs::remove_file(path).map_err(io_failed),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_failed(error)),
        },
        PathSnapshot::Present {
            fingerprint,
            contents,
            readonly,
        } => {
            if !fingerprint.is_file {
                return Ok(());
            }
            let contents = contents
                .as_deref()
                .ok_or_else(|| failed("apply_patch_rollback_contents_missing"))?;
            atomic_replace_bytes(path, contents)?;
            let mut permissions = fs::metadata(path).map_err(io_failed)?.permissions();
            permissions.set_readonly(*readonly);
            fs::set_permissions(path, permissions).map_err(io_failed)
        }
    }
}

fn commit_op(
    project_root: &Path,
    op: &PlannedOp,
    directories: &CommitDirectories,
) -> Result<Value, PortError> {
    match op {
        PlannedOp::Add { target, contents } => {
            commit_add(project_root, target, contents, directories)
        }
        PlannedOp::Delete { target } => commit_delete(project_root, target, directories),
        PlannedOp::Update { target, contents } => {
            commit_update(project_root, target, contents, directories)
        }
        PlannedOp::Move {
            source,
            destination,
            contents,
        } => commit_move(project_root, source, destination, contents, directories),
    }
}

fn commit_add(
    project_root: &Path,
    target: &Path,
    contents: &str,
    directories: &CommitDirectories,
) -> Result<Value, PortError> {
    #[cfg(target_os = "linux")]
    if let Some(parent) = target
        .parent()
        .and_then(|path| directories.iter().find(|(candidate, _)| candidate == path))
        .map(|(_, file)| file)
    {
        create_new_file_at(target, contents, parent)?;
    } else {
        ensure_parent(target)?;
        create_new_file(target, contents)?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = directories;
        ensure_parent(target)?;
        create_new_file(target, contents)?;
    }
    Ok(json!({ "op": "add", "path": display_relative(project_root, target) }))
}

fn commit_delete(
    project_root: &Path,
    target: &Path,
    directories: &CommitDirectories,
) -> Result<Value, PortError> {
    #[cfg(target_os = "linux")]
    if let Some(parent) = target
        .parent()
        .and_then(|path| directories.iter().find(|(candidate, _)| candidate == path))
        .map(|(_, file)| file)
    {
        remove_file_at(target, parent)?;
    } else {
        fs::remove_file(target).map_err(io_failed)?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = directories;
        fs::remove_file(target).map_err(io_failed)?;
    }
    Ok(json!({ "op": "delete", "path": display_relative(project_root, target) }))
}

fn commit_update(
    project_root: &Path,
    target: &Path,
    contents: &str,
    directories: &CommitDirectories,
) -> Result<Value, PortError> {
    #[cfg(target_os = "linux")]
    if let Some(parent) = target
        .parent()
        .and_then(|path| directories.iter().find(|(candidate, _)| candidate == path))
        .map(|(_, file)| file)
    {
        atomic_replace_bytes_at(target, contents.as_bytes(), parent)?;
    } else {
        atomic_replace_bytes(target, contents.as_bytes())?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = directories;
        atomic_replace(target, contents)?;
    }
    Ok(json!({ "op": "update", "path": display_relative(project_root, target) }))
}

fn commit_move(
    project_root: &Path,
    source: &Path,
    destination: &Path,
    contents: &str,
    directories: &CommitDirectories,
) -> Result<Value, PortError> {
    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = destination
            .parent()
            .and_then(|path| directories.iter().find(|(candidate, _)| candidate == path))
            .map(|(_, file)| file)
        {
            create_new_file_at(destination, contents, parent)?;
        } else {
            ensure_parent(destination)?;
            create_new_file(destination, contents)?;
        }
        if let Some(parent) = source
            .parent()
            .and_then(|path| directories.iter().find(|(candidate, _)| candidate == path))
            .map(|(_, file)| file)
        {
            remove_file_at(source, parent)?;
        } else {
            fs::remove_file(source).map_err(io_failed)?;
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = directories;
        ensure_parent(destination)?;
        create_new_file(destination, contents)?;
        fs::remove_file(source).map_err(io_failed)?;
    }
    Ok(json!({ "op": "update", "path": display_relative(project_root, destination) }))
}

fn ensure_parent(path: &Path) -> Result<(), PortError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_failed)?;
    }
    Ok(())
}

fn create_new_file(path: &Path, contents: &str) -> Result<(), PortError> {
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(failed("apply_patch_add_exists"));
        }
        Err(error) => return Err(io_failed(error)),
    };
    file.write_all(contents.as_bytes()).map_err(|error| {
        let _ = fs::remove_file(path);
        io_failed(error)
    })
}

#[cfg(target_os = "linux")]
fn create_new_file_at(path: &Path, contents: &str, parent_file: &File) -> Result<(), PortError> {
    let name = path
        .file_name()
        .ok_or_else(|| failed("apply_patch_path_required"))?;
    let name =
        std::ffi::CString::new(name.as_bytes()).map_err(|_| failed("apply_patch_path_invalid"))?;
    let fd = unsafe {
        libc::openat(
            parent_file.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o666,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::AlreadyExists {
            Err(failed("apply_patch_add_exists"))
        } else {
            Err(io_failed(error))
        };
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    if let Err(error) = file.write_all(contents.as_bytes()) {
        drop(file);
        unsafe {
            libc::unlinkat(parent_file.as_raw_fd(), name.as_ptr(), 0);
        }
        return Err(io_failed(error));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_file_at(path: &Path, parent_file: &File) -> Result<(), PortError> {
    let name = path
        .file_name()
        .ok_or_else(|| failed("apply_patch_path_required"))?;
    let name =
        std::ffi::CString::new(name.as_bytes()).map_err(|_| failed("apply_patch_path_invalid"))?;
    let result = unsafe { libc::unlinkat(parent_file.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io_failed(std::io::Error::last_os_error()))
    }
}

#[cfg(not(target_os = "linux"))]
fn atomic_replace(path: &Path, contents: &str) -> Result<(), PortError> {
    atomic_replace_bytes(path, contents.as_bytes())
}

#[cfg(target_os = "linux")]
fn atomic_replace_bytes(path: &Path, contents: &[u8]) -> Result<(), PortError> {
    let parent = path
        .parent()
        .ok_or_else(|| failed("apply_patch_path_required"))?;
    let parent_name = std::ffi::CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| failed("apply_patch_path_invalid"))?;
    let parent_fd = unsafe {
        libc::open(
            parent_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if parent_fd < 0 {
        return Err(io_failed(std::io::Error::last_os_error()));
    }
    let parent_file = unsafe { File::from_raw_fd(parent_fd) };
    atomic_replace_bytes_at(path, contents, &parent_file)
}

#[cfg(target_os = "linux")]
fn atomic_replace_bytes_at(
    path: &Path,
    contents: &[u8],
    parent_file: &File,
) -> Result<(), PortError> {
    let target_name = path
        .file_name()
        .ok_or_else(|| failed("apply_patch_path_required"))?;
    let target_name = std::ffi::CString::new(target_name.as_bytes())
        .map_err(|_| failed("apply_patch_path_invalid"))?;
    let stamp = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let permissions = {
        let mut metadata = unsafe { std::mem::zeroed::<libc::stat>() };
        let result = unsafe {
            libc::fstatat(
                parent_file.as_raw_fd(),
                target_name.as_ptr(),
                &mut metadata,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            Some(fs::Permissions::from_mode(metadata.st_mode as u32 & 0o7777))
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                None
            } else {
                return Err(io_failed(error));
            }
        }
    };

    for attempt in 0..TEMP_SIBLING_ATTEMPTS {
        let temporary = temp_sibling_path(path, stamp, attempt)?;
        let temporary_name = std::ffi::CString::new(
            temporary
                .file_name()
                .ok_or_else(|| failed("apply_patch_path_required"))?
                .as_bytes(),
        )
        .map_err(|_| failed("apply_patch_path_invalid"))?;
        let temporary_fd = unsafe {
            libc::openat(
                parent_file.as_raw_fd(),
                temporary_name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if temporary_fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(io_failed(error));
        }
        let mut file = unsafe { File::from_raw_fd(temporary_fd) };
        let write_result = (|| {
            file.write_all(contents).map_err(io_failed)?;
            if let Some(permissions) = permissions.clone() {
                file.set_permissions(permissions).map_err(io_failed)?;
            }
            Ok(())
        })();
        drop(file);
        if let Err(error) = write_result {
            unsafe {
                libc::unlinkat(parent_file.as_raw_fd(), temporary_name.as_ptr(), 0);
            }
            return Err(error);
        }
        let renamed = unsafe {
            libc::renameat(
                parent_file.as_raw_fd(),
                temporary_name.as_ptr(),
                parent_file.as_raw_fd(),
                target_name.as_ptr(),
            )
        };
        if renamed == 0 {
            return Ok(());
        }
        unsafe {
            libc::unlinkat(parent_file.as_raw_fd(), temporary_name.as_ptr(), 0);
        }
        return Err(io_failed(std::io::Error::last_os_error()));
    }
    Err(failed("apply_patch_temp_unavailable"))
}

#[cfg(not(target_os = "linux"))]
fn atomic_replace_bytes(path: &Path, contents: &[u8]) -> Result<(), PortError> {
    let (tmp, mut file) = create_temp_sibling(path)?;
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let write_result = (|| {
        file.write_all(contents).map_err(io_failed)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions).map_err(io_failed)?;
        }
        Ok(())
    })();
    drop(file);
    let result = write_result.and_then(|()| fs::rename(&tmp, path).map_err(io_failed));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(not(target_os = "linux"))]
fn create_temp_sibling(path: &Path) -> Result<(PathBuf, File), PortError> {
    let stamp = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    create_temp_sibling_with_stamp(path, stamp)
}

#[cfg(any(not(target_os = "linux"), test))]
fn create_temp_sibling_with_stamp(path: &Path, stamp: u128) -> Result<(PathBuf, File), PortError> {
    for attempt in 0..TEMP_SIBLING_ATTEMPTS {
        let tmp = temp_sibling_path(path, stamp, attempt)?;
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_failed(error)),
        }
    }
    Err(failed("apply_patch_temp_unavailable"))
}

fn temp_sibling_path(path: &Path, stamp: u128, attempt: usize) -> Result<PathBuf, PortError> {
    let name = path
        .file_name()
        .ok_or_else(|| failed("apply_patch_path_required"))?;
    Ok(path.with_file_name(format!(
        ".{}.kiana-patch-{}-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        stamp,
        attempt
    )))
}

fn apply_chunks(original: &str, chunks: &[UpdateChunk]) -> Result<String, PortError> {
    let trailing_newline = original.ends_with('\n');
    let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();

    for chunk in chunks {
        let start = locate_chunk(&lines, chunk)?;
        let end = start + chunk.old_lines.len();
        let mut next = Vec::new();
        next.extend_from_slice(&lines[..start]);
        next.extend(chunk.new_lines.iter().cloned());
        next.extend_from_slice(&lines[end..]);
        lines = next;
    }

    if lines.is_empty() {
        return Ok(String::new());
    }
    let mut rendered = lines.join("\n");
    if trailing_newline {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn locate_chunk(lines: &[String], chunk: &UpdateChunk) -> Result<usize, PortError> {
    let search_from = if let Some(context) = &chunk.change_context {
        lines
            .iter()
            .position(|line| line.contains(context))
            .ok_or_else(|| failed("apply_patch_context_missing"))?
    } else {
        0
    };
    if chunk.old_lines.is_empty() {
        return Ok(if chunk.change_context.is_some() {
            search_from + 1
        } else {
            0
        });
    }
    let mut found = None;
    let window = chunk.old_lines.len();
    if search_from + window > lines.len() {
        return Err(failed("apply_patch_hunk_mismatch"));
    }
    for start in search_from..=lines.len() - window {
        if lines[start..start + window] == chunk.old_lines {
            if found.is_some() {
                return Err(failed("apply_patch_hunk_ambiguous"));
            }
            found = Some(start);
        }
    }
    found.ok_or_else(|| failed("apply_patch_hunk_mismatch"))
}

fn confined_existing_file(root: &Path, relative: &Path) -> Result<PathBuf, PortError> {
    let candidate = confined_candidate(root, relative)?;
    reject_symlink_components(root, &candidate)?;
    let metadata = fs::symlink_metadata(&candidate).map_err(io_failed)?;
    if !metadata.is_file() {
        return Err(failed("apply_patch_path_not_file"));
    }
    reject_hardlink(&metadata)?;
    let resolved = candidate
        .canonicalize()
        .map_err(|error| failed(format!("apply_patch_path_invalid:{error}")))?;
    ensure_inside(root, &resolved)?;
    Ok(resolved)
}

fn confined_new_file(root: &Path, relative: &Path) -> Result<PathBuf, PortError> {
    let candidate = confined_candidate(root, relative)?;
    reject_symlink_components(root, &candidate)?;
    if candidate.exists() {
        let metadata = fs::symlink_metadata(&candidate).map_err(io_failed)?;
        if metadata.is_file() {
            reject_hardlink(&metadata)?;
        }
        let resolved = candidate
            .canonicalize()
            .map_err(|error| failed(format!("apply_patch_path_invalid:{error}")))?;
        ensure_inside(root, &resolved)?;
        return Ok(resolved);
    }
    if let Some(parent) = candidate.parent() {
        if parent.exists() {
            let resolved_parent = parent
                .canonicalize()
                .map_err(|error| failed(format!("apply_patch_path_invalid:{error}")))?;
            ensure_inside(root, &resolved_parent)?;
        } else {
            ensure_inside(root, parent)?;
        }
    }
    Ok(candidate)
}

fn reject_symlink_components(root: &Path, candidate: &Path) -> Result<(), PortError> {
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| failed("apply_patch_path_outside_project"))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(failed("apply_patch_path_symlink"));
            }
        }
    }
    Ok(())
}

fn reject_hardlink(metadata: &fs::Metadata) -> Result<(), PortError> {
    #[cfg(unix)]
    if metadata.nlink() > 1 {
        return Err(failed("apply_patch_path_hardlink"));
    }
    Ok(())
}

fn confined_candidate(root: &Path, relative: &Path) -> Result<PathBuf, PortError> {
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(failed("apply_patch_path_not_relative"));
    }
    if relative.as_os_str().is_empty() {
        return Err(failed("apply_patch_path_required"));
    }
    Ok(root.join(relative))
}

fn ensure_inside(root: &Path, candidate: &Path) -> Result<(), PortError> {
    if !candidate.starts_with(root) {
        return Err(failed("apply_patch_path_outside_project"));
    }
    Ok(())
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn failed(message: impl Into<String>) -> PortError {
    PortError::Failed(message.into())
}

fn io_failed(error: std::io::Error) -> PortError {
    PortError::Failed(format!("apply_patch_io:{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-apply-patch-{stamp}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[cfg(unix)]
    #[test]
    fn project_patch_lock_waits_for_the_current_commit() {
        let root = temp_root();
        let first = ProjectPatchLock::acquire(&root).unwrap();
        let waiting_root = root.clone();
        let waiting = std::thread::spawn(move || ProjectPatchLock::acquire(&waiting_root));
        std::thread::sleep(std::time::Duration::from_millis(25));
        assert!(!waiting.is_finished());
        drop(first);
        assert!(waiting.join().unwrap().is_ok());
    }

    #[test]
    fn commit_failure_rolls_back_earlier_operations() {
        let root = temp_root();
        fs::write(root.join("blocker"), "keep-me\n").unwrap();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Add File: first.txt\n+first\n*** Add File: blocker/second.txt\n+second\n*** End Patch\n",
        )
        .unwrap_err();
        assert!(matches!(
            error,
            PortError::Failed(message) if message.starts_with("apply_patch_io:")
        ));
        assert!(!root.join("first.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("blocker")).unwrap(),
            "keep-me\n"
        );
    }

    #[test]
    fn changed_file_after_preflight_is_rejected() {
        let root = temp_root();
        let target = root.join("file.txt");
        fs::write(&target, "before\n").unwrap();
        let hunks = parse_patch(
            "*** Begin Patch\n*** Update File: file.txt\n@@\n-before\n+after\n*** End Patch\n",
        )
        .unwrap();
        let planned = plan_hunks(&root, hunks).unwrap();
        fs::write(&target, "changed\n").unwrap();
        assert_eq!(
            verify_preconditions(&planned.preconditions).unwrap_err(),
            failed(format!("apply_patch_path_changed:{}", target.display()))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn committed_update_keeps_the_preflighted_parent_after_path_replacement() {
        let root = temp_root();
        let directory = root.join("nested");
        let target = directory.join("file.txt");
        let moved_directory = root.join("nested-original");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&target, "before\n").unwrap();
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&target, permissions).unwrap();
        let operations = vec![PlannedOp::Update {
            target: target.clone(),
            contents: "after\n".to_owned(),
        }];
        let preconditions = capture_preconditions(&root, &operations).unwrap();
        let directories = open_commit_directories(&preconditions).unwrap();
        verify_preconditions(&preconditions).unwrap();

        fs::rename(&directory, &moved_directory).unwrap();
        fs::create_dir(&directory).unwrap();
        commit_update(&root, &target, "after\n", &directories).unwrap();

        assert_eq!(
            fs::read_to_string(moved_directory.join("file.txt")).unwrap(),
            "after\n"
        );
        assert!(
            fs::metadata(moved_directory.join("file.txt"))
                .unwrap()
                .permissions()
                .readonly(),
            "descriptor-relative replacement must preserve the original mode"
        );
        assert!(
            !target.exists(),
            "replacement parent must not receive the write"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn committed_add_keeps_the_preflighted_parent_after_path_replacement() {
        let root = temp_root();
        let directory = root.join("nested");
        let target = directory.join("new.txt");
        let moved_directory = root.join("nested-original");
        fs::create_dir_all(&directory).unwrap();
        let operations = vec![PlannedOp::Add {
            target: target.clone(),
            contents: "added\n".to_owned(),
        }];
        let preconditions = capture_preconditions(&root, &operations).unwrap();
        let directories = open_commit_directories(&preconditions).unwrap();
        verify_preconditions(&preconditions).unwrap();

        fs::rename(&directory, &moved_directory).unwrap();
        fs::create_dir(&directory).unwrap();
        commit_add(&root, &target, "added\n", &directories).unwrap();

        assert_eq!(
            fs::read_to_string(moved_directory.join("new.txt")).unwrap(),
            "added\n"
        );
        assert!(
            !target.exists(),
            "replacement parent must not receive the descriptor-relative add"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn committed_delete_keeps_the_preflighted_parent_after_path_replacement() {
        let root = temp_root();
        let directory = root.join("nested");
        let target = directory.join("gone.txt");
        let moved_directory = root.join("nested-original");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&target, "delete-me\n").unwrap();
        let operations = vec![PlannedOp::Delete {
            target: target.clone(),
        }];
        let preconditions = capture_preconditions(&root, &operations).unwrap();
        let directories = open_commit_directories(&preconditions).unwrap();
        verify_preconditions(&preconditions).unwrap();

        fs::rename(&directory, &moved_directory).unwrap();
        fs::create_dir(&directory).unwrap();
        commit_delete(&root, &target, &directories).unwrap();

        assert!(
            !moved_directory.join("gone.txt").exists(),
            "descriptor-relative delete must affect the preflighted parent"
        );
        assert!(
            !target.exists(),
            "replacement parent must not retain a path-visible target"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn committed_move_keeps_the_preflighted_parent_after_path_replacement() {
        let root = temp_root();
        let directory = root.join("nested");
        let source = directory.join("before.txt");
        let destination = directory.join("after.txt");
        let moved_directory = root.join("nested-original");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&source, "before\n").unwrap();
        let operations = vec![PlannedOp::Move {
            source: source.clone(),
            destination: destination.clone(),
            contents: "after\n".to_owned(),
        }];
        let preconditions = capture_preconditions(&root, &operations).unwrap();
        let directories = open_commit_directories(&preconditions).unwrap();
        verify_preconditions(&preconditions).unwrap();

        fs::rename(&directory, &moved_directory).unwrap();
        fs::create_dir(&directory).unwrap();
        commit_move(&root, &source, &destination, "after\n", &directories).unwrap();

        assert!(!moved_directory.join("before.txt").exists());
        assert_eq!(
            fs::read_to_string(moved_directory.join("after.txt")).unwrap(),
            "after\n"
        );
        assert!(
            !destination.exists(),
            "replacement parent must not receive the descriptor-relative move"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_target_is_rejected_without_following_it() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let outside = temp_root();
        fs::write(outside.join("outside.txt"), "outside\n").unwrap();
        symlink(outside.join("outside.txt"), root.join("linked.txt")).unwrap();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Update File: linked.txt\n@@\n-outside\n+overwritten\n*** End Patch\n",
        )
        .unwrap_err();
        assert_eq!(error, failed("apply_patch_path_symlink"));
        assert_eq!(
            fs::read_to_string(outside.join("outside.txt")).unwrap(),
            "outside\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn temp_sibling_collision_does_not_follow_existing_symlink() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let outside = temp_root();
        let target = root.join("file.txt");
        let outside_target = outside.join("outside.txt");
        fs::write(&outside_target, "outside\n").unwrap();
        let stamp = 42;
        let planted = temp_sibling_path(&target, stamp, 0).unwrap();
        symlink(&outside_target, &planted).unwrap();

        let (temporary, mut file) = create_temp_sibling_with_stamp(&target, stamp).unwrap();
        assert_eq!(
            temporary,
            temp_sibling_path(&target, stamp, 1).unwrap(),
            "an occupied first candidate must be skipped"
        );
        file.write_all(b"temporary\n").unwrap();
        drop(file);

        assert_eq!(fs::read_to_string(&outside_target).unwrap(), "outside\n");
        assert_eq!(fs::read_to_string(&temporary).unwrap(), "temporary\n");
        fs::remove_file(temporary).unwrap();
    }

    #[test]
    fn exhausted_temp_sibling_names_fail_without_reusing_a_file() {
        let root = temp_root();
        let target = root.join("file.txt");
        let stamp = 43;
        for attempt in 0..TEMP_SIBLING_ATTEMPTS {
            fs::write(
                temp_sibling_path(&target, stamp, attempt).unwrap(),
                "occupied\n",
            )
            .unwrap();
        }

        assert_eq!(
            create_temp_sibling_with_stamp(&target, stamp).unwrap_err(),
            failed("apply_patch_temp_unavailable")
        );
        for attempt in 0..TEMP_SIBLING_ATTEMPTS {
            assert_eq!(
                fs::read_to_string(temp_sibling_path(&target, stamp, attempt).unwrap()).unwrap(),
                "occupied\n"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn hardlinked_target_is_rejected_without_mutating_peer() {
        let root = temp_root();
        let outside = temp_root();
        fs::write(outside.join("outside.txt"), "outside\n").unwrap();
        fs::hard_link(outside.join("outside.txt"), root.join("linked.txt")).unwrap();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Update File: linked.txt\n@@\n-outside\n+overwritten\n*** End Patch\n",
        )
        .unwrap_err();
        assert_eq!(error, failed("apply_patch_path_hardlink"));
        assert_eq!(
            fs::read_to_string(outside.join("outside.txt")).unwrap(),
            "outside\n"
        );
    }

    #[test]
    fn add_file_is_confined_to_project_root() {
        let root = temp_root();
        apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Add File: notes/hello.txt\n+hello\n*** End Patch\n",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("notes/hello.txt")).unwrap(),
            "hello\n"
        );
    }

    #[test]
    fn update_replaces_a_unique_hunk() {
        let root = temp_root();
        fs::write(root.join("file.txt"), "old\nkeep\n").unwrap();
        apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Update File: file.txt\n@@\n-old\n+new\n*** End Patch\n",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("file.txt")).unwrap(),
            "new\nkeep\n"
        );
    }

    #[test]
    fn parent_dir_escape_is_rejected() {
        let root = temp_root();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Add File: ../escape.txt\n+nope\n*** End Patch\n",
        )
        .unwrap_err();
        assert_eq!(error, failed("apply_patch_path_not_relative"));
    }

    #[test]
    fn later_file_hunk_mismatch_leaves_earlier_file_untouched() {
        let root = temp_root();
        fs::write(root.join("first.txt"), "keep-me\n").unwrap();
        fs::write(root.join("second.txt"), "original\n").unwrap();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Update File: first.txt\n@@\n-keep-me\n+changed\n*** Update File: second.txt\n@@\n-missing\n+nope\n*** End Patch\n",
        )
        .unwrap_err();
        assert_eq!(error, failed("apply_patch_hunk_mismatch"));
        assert_eq!(
            fs::read_to_string(root.join("first.txt")).unwrap(),
            "keep-me\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("second.txt")).unwrap(),
            "original\n"
        );
    }

    #[test]
    fn later_add_conflict_does_not_keep_earlier_add() {
        let root = temp_root();
        fs::write(root.join("exists.txt"), "already\n").unwrap();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Add File: new.txt\n+fresh\n*** Add File: exists.txt\n+nope\n*** End Patch\n",
        )
        .unwrap_err();
        assert_eq!(error, failed("apply_patch_add_exists"));
        assert!(!root.join("new.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("exists.txt")).unwrap(),
            "already\n"
        );
    }

    #[test]
    fn later_chunk_mismatch_does_not_write_partial_update() {
        let root = temp_root();
        fs::write(root.join("file.txt"), "alpha\nbeta\n").unwrap();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Update File: file.txt\n@@\n-alpha\n+ALPHA\n@@\n-missing\n+nope\n*** End Patch\n",
        )
        .unwrap_err();
        assert_eq!(error, failed("apply_patch_hunk_mismatch"));
        assert_eq!(
            fs::read_to_string(root.join("file.txt")).unwrap(),
            "alpha\nbeta\n"
        );
    }

    #[test]
    fn later_mismatch_does_not_keep_earlier_delete() {
        let root = temp_root();
        fs::write(root.join("gone.txt"), "delete-me\n").unwrap();
        fs::write(root.join("keep.txt"), "stay\n").unwrap();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Delete File: gone.txt\n*** Update File: keep.txt\n@@\n-missing\n+nope\n*** End Patch\n",
        )
        .unwrap_err();
        assert_eq!(error, failed("apply_patch_hunk_mismatch"));
        assert_eq!(
            fs::read_to_string(root.join("gone.txt")).unwrap(),
            "delete-me\n"
        );
        assert_eq!(fs::read_to_string(root.join("keep.txt")).unwrap(), "stay\n");
    }

    #[test]
    fn binary_file_delete_is_supported_and_rollback_safe() {
        let root = temp_root();
        let binary = root.join("binary.dat");
        fs::write(&binary, [0, 159, 146, 150, 255]).unwrap();
        fs::write(root.join("blocker"), "block").unwrap();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Delete File: binary.dat\n*** Add File: blocker/second.txt\n+second\n*** End Patch\n",
        )
        .unwrap_err();
        assert!(
            matches!(error, PortError::Failed(message) if message.starts_with("apply_patch_io:"))
        );
        assert_eq!(fs::read(&binary).unwrap(), [0, 159, 146, 150, 255]);

        apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Delete File: binary.dat\n*** End Patch\n",
        )
        .unwrap();
        assert!(!binary.exists());
    }

    #[test]
    fn multiple_hunks_apply_after_full_preflight() {
        let root = temp_root();
        fs::write(root.join("file.txt"), "alpha\nbeta\n").unwrap();
        apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Update File: file.txt\n@@\n-alpha\n+ALPHA\n@@\n-beta\n+BETA\n*** End Patch\n",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("file.txt")).unwrap(),
            "ALPHA\nBETA\n"
        );
    }

    #[test]
    fn successful_update_does_not_leave_temp_siblings() {
        let root = temp_root();
        fs::write(root.join("file.txt"), "old\n").unwrap();
        apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Update File: file.txt\n@@\n-old\n+new\n*** End Patch\n",
        )
        .unwrap();
        let leftovers: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .filter(|name| name.to_string_lossy().contains("kiana-patch"))
            .collect();
        assert!(leftovers.is_empty());
        assert_eq!(fs::read_to_string(root.join("file.txt")).unwrap(), "new\n");
    }
}
