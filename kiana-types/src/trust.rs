use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

pub const PROJECT_TRUST_SCHEMA: &str = "kiana.project-trust.v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTrust {
    Unknown,
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrustRecord {
    pub schema: String,
    pub project_id: String,
    pub project_root: String,
    pub trusted: bool,
}

impl ProjectTrust {
    pub fn allows_project_resources(self) -> bool {
        matches!(self, ProjectTrust::Trusted)
    }

    pub fn as_bool(self) -> bool {
        matches!(self, ProjectTrust::Trusted)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProjectTrust::Unknown => "unknown",
            ProjectTrust::Trusted => "trusted",
            ProjectTrust::Untrusted => "untrusted",
        }
    }
}

pub fn project_trust_from_app_state(app_state: &HashMap<String, Value>) -> ProjectTrust {
    if let Some(project_trust) = app_state_project_trust(app_state) {
        return project_trust;
    }
    if let Some(cwd) = app_state_path(app_state, "cwd")
        .or_else(|| app_state_path(app_state, "project_root"))
        .or_else(|| app_state_path(app_state, "projectRoot"))
    {
        if let Ok(Some(project_trust)) = read_project_trust(&cwd) {
            return project_trust;
        }
    }

    ProjectTrust::Unknown
}

pub fn has_explicit_project_trust(app_state: &HashMap<String, Value>) -> bool {
    app_state_project_trust(app_state).is_some()
        || app_state_path(app_state, "cwd")
            .or_else(|| app_state_path(app_state, "project_root"))
            .or_else(|| app_state_path(app_state, "projectRoot"))
            .and_then(|cwd| read_project_trust(&cwd).ok().flatten())
            .is_some()
}

fn app_state_project_trust(app_state: &HashMap<String, Value>) -> Option<ProjectTrust> {
    app_state_bool(app_state, "project_trusted")
        .or_else(|| app_state_bool(app_state, "projectTrusted"))
        .map(|trusted| {
            if trusted {
                ProjectTrust::Trusted
            } else {
                ProjectTrust::Untrusted
            }
        })
}

pub fn project_trust_root(cwd: impl AsRef<Path>) -> PathBuf {
    let cwd = canonicalize_or_absolute(cwd.as_ref());
    if let Some(root) = cwd.ancestors().find(|ancestor| {
        let git_marker = ancestor.join(".git");
        git_marker.is_dir() || git_marker.is_file()
    }) {
        return root.to_path_buf();
    }
    cwd
}

pub fn project_trust_id(cwd: impl AsRef<Path>) -> String {
    let root = project_trust_root(cwd);
    project_trust_id_for_root(&root)
}

pub fn project_trust_file_path(cwd: impl AsRef<Path>) -> Result<PathBuf, String> {
    let root = project_trust_root(cwd);
    Ok(validated_project_trust_store_dir(&root)?
        .join(project_trust_id_for_root(&root))
        .with_extension("json"))
}

pub fn legacy_project_trust_file_path(cwd: impl AsRef<Path>) -> PathBuf {
    project_trust_root(cwd).join(".kiana").join("trust.json")
}

pub fn find_project_trust_file(cwd: impl AsRef<Path>) -> Option<PathBuf> {
    let root = project_trust_root(cwd);
    let project_id = project_trust_id_for_root(&root);
    let store_dir = validated_project_trust_store_dir(&root).ok()?;
    let pending_path = project_trust_pending_path(&store_dir, &project_id);
    if pending_marker_exists(&pending_path).ok()? {
        return None;
    }
    let path = store_dir.join(project_id).with_extension("json");
    match open_regular_file_no_follow(&path) {
        Ok(Some(_)) if pending_marker_exists(&pending_path).ok()? => None,
        Ok(Some(_)) => Some(path),
        Ok(None) | Err(_) => None,
    }
}

pub fn read_project_trust(cwd: impl AsRef<Path>) -> Result<Option<ProjectTrust>, String> {
    let root = project_trust_root(cwd);
    let expected_project_id = project_trust_id_for_root(&root);
    let store_dir = validated_project_trust_store_dir(&root)?;
    let pending_path = project_trust_pending_path(&store_dir, &expected_project_id);
    reject_pending_project_trust(&pending_path)?;
    #[cfg(test)]
    pause_reader_after_initial_pending_check();
    let path = store_dir.join(&expected_project_id).with_extension("json");
    let mut file = match open_regular_file_no_follow(&path)? {
        Some(file) => file,
        None => return Ok(None),
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    let record = serde_json::from_str::<ProjectTrustRecord>(&contents)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))?;

    if record.schema != PROJECT_TRUST_SCHEMA {
        return Err(format!(
            "project trust record {} schema mismatch: expected {}, found {}",
            path.display(),
            PROJECT_TRUST_SCHEMA,
            record.schema
        ));
    }
    if record.project_id != expected_project_id {
        return Err(format!(
            "project trust record {} project_id mismatch: expected {}, found {}",
            path.display(),
            expected_project_id,
            record.project_id
        ));
    }
    let expected_project_root = project_root_string(&root);
    if record.project_root != expected_project_root {
        return Err(format!(
            "project trust record {} project_root mismatch: expected {}, found {}",
            path.display(),
            expected_project_root,
            record.project_root
        ));
    }

    let project_trust = if record.trusted {
        ProjectTrust::Trusted
    } else {
        ProjectTrust::Untrusted
    };

    // A writer may install pending after the first check and replace the record
    // before this reader opens it. Rechecking after validation prevents an
    // incomplete Trusted write from becoming observable. If pending appears
    // after this check, this read linearizes before that writer starts.
    reject_pending_project_trust(&pending_path)?;
    Ok(Some(project_trust))
}

pub fn write_project_trust(
    cwd: impl AsRef<Path>,
    project_trust: ProjectTrust,
) -> Result<PathBuf, String> {
    let trusted = match project_trust {
        ProjectTrust::Unknown => return Err("cannot persist unknown project trust".to_string()),
        ProjectTrust::Trusted => true,
        ProjectTrust::Untrusted => false,
    };
    let root = project_trust_root(cwd);
    let store_dir = validated_project_trust_store_dir(&root)?;
    persist_project_trust_record(&root, &store_dir, trusted)
}

pub fn remove_project_trust(cwd: impl AsRef<Path>) -> Result<Option<PathBuf>, String> {
    let root = project_trust_root(cwd);
    let store_dir = validated_project_trust_store_dir(&root)?;
    let project_id = project_trust_id_for_root(&root);
    let path = store_dir.join(&project_id).with_extension("json");
    let pending_path = project_trust_pending_path(&store_dir, &project_id);
    let _writer_lease = ProjectTrustWriterLease::acquire(&store_dir, &project_id)?;
    let had_record = path_entry_exists_no_follow(&path)?;
    if !had_record && !pending_marker_exists(&pending_path)? {
        cleanup_project_trust_reset_tombstones(&store_dir, &project_id)?;
        return Ok(None);
    }

    install_pending_marker(&store_dir, &pending_path, &project_id)?;
    let tombstone_path =
        had_record.then(|| project_trust_reset_tombstone_path(&store_dir, &project_id));
    if let Some(tombstone_path) = tombstone_path.as_ref() {
        replace_file_atomically(&path, tombstone_path).map_err(|error| {
            format!(
                "failed to move project trust record {} to reset tombstone {}: {}",
                path.display(),
                tombstone_path.display(),
                error
            )
        })?;
    }
    sync_store_dir(&store_dir)?;
    clear_pending_marker_at_commit(&store_dir, &pending_path)?;

    // The reset is committed once pending is gone: the authoritative record
    // path was durably moved away first. Tombstone cleanup is non-authoritative.
    let _ = cleanup_project_trust_reset_tombstones(&store_dir, &project_id);
    let _ = sync_store_dir(&store_dir);
    Ok(had_record.then_some(path))
}

fn app_state_bool(app_state: &HashMap<String, Value>, key: &str) -> Option<bool> {
    app_state.get(key).and_then(Value::as_bool)
}

fn app_state_path(app_state: &HashMap<String, Value>, key: &str) -> Option<PathBuf> {
    app_state
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn canonicalize_or_absolute(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    fs::canonicalize(&absolute).unwrap_or_else(|_| normalize_path_lexically(&absolute))
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let is_absolute = path.is_absolute();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !is_absolute {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn project_root_string(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

fn project_trust_id_for_root(root: &Path) -> String {
    let mut hasher = Sha256::new();
    hash_project_root_identity(&mut hasher, root);
    let digest = hasher.finalize();
    format!("{digest:x}")
}

fn project_trust_pending_path(store_dir: &Path, project_id: &str) -> PathBuf {
    store_dir.join(project_id).with_extension("pending")
}

fn project_trust_lock_path(store_dir: &Path, project_id: &str) -> PathBuf {
    store_dir.join(project_id).with_extension("lock")
}

fn project_trust_reset_tombstone_path(store_dir: &Path, project_id: &str) -> PathBuf {
    store_dir.join(format!(".{project_id}.{}.reset", uuid::Uuid::new_v4()))
}

#[cfg(unix)]
fn hash_project_root_identity(hasher: &mut Sha256, root: &Path) {
    use std::os::unix::ffi::OsStrExt;

    hasher.update(root.as_os_str().as_bytes());
}

#[cfg(windows)]
fn hash_project_root_identity(hasher: &mut Sha256, root: &Path) {
    use std::os::windows::ffi::OsStrExt;

    for code_unit in root.as_os_str().encode_wide() {
        let folded = if (u16::from(b'A')..=u16::from(b'Z')).contains(&code_unit) {
            code_unit + u16::from(b'a' - b'A')
        } else {
            code_unit
        };
        hasher.update(folded.to_le_bytes());
    }
}

#[cfg(not(any(unix, windows)))]
fn hash_project_root_identity(hasher: &mut Sha256, root: &Path) {
    hasher.update(root.as_os_str().as_encoded_bytes());
}

fn validated_project_trust_store_dir(project_root: &Path) -> Result<PathBuf, String> {
    let home = kiana_home()?;
    let store_dir = canonicalize_with_missing_tail(&home.join("trust").join("projects"));
    if store_dir == project_root || store_dir.starts_with(project_root) {
        return Err(format!(
            "project trust store {} must stay outside project root {}",
            store_dir.display(),
            project_root.display()
        ));
    }
    Ok(store_dir)
}

fn kiana_home() -> Result<PathBuf, String> {
    if let Some(path) = non_empty_env_path("KIANA_HOME") {
        if !path.is_absolute() {
            return Err(format!(
                "KIANA_HOME must be an absolute path for project trust: {}",
                path.display()
            ));
        }
        return Ok(path);
    }
    #[cfg(windows)]
    if let Some(profile) = non_empty_env_path("USERPROFILE") {
        if !profile.is_absolute() {
            return Err("USERPROFILE must be an absolute user home for project trust".to_string());
        }
        return Ok(profile.join(".kiana"));
    }
    if let Some(home) = non_empty_env_path("HOME") {
        if !home.is_absolute() {
            return Err("HOME must be an absolute user home for project trust".to_string());
        }
        return Ok(home.join(".kiana"));
    }
    Err("project trust requires an absolute KIANA_HOME or user home".to_string())
}

fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.as_os_str().is_empty())
        .map(PathBuf::from)
}

fn canonicalize_with_missing_tail(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }

    let mut cursor = path;
    let mut tail = Vec::<OsString>::new();
    while !cursor.exists() {
        let Some(name) = cursor.file_name() else {
            return normalize_path_lexically(path);
        };
        tail.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            return normalize_path_lexically(path);
        };
        cursor = parent;
    }

    let mut resolved = fs::canonicalize(cursor).unwrap_or_else(|_| cursor.to_path_buf());
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    normalize_path_lexically(&resolved)
}

fn path_entry_exists_no_follow(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("failed to inspect {}: {}", path.display(), error)),
    }
}

fn pending_marker_exists(path: &Path) -> Result<bool, String> {
    path_entry_exists_no_follow(path).map_err(|error| {
        format!(
            "failed to inspect project trust pending marker {}: {}",
            path.display(),
            error
        )
    })
}

fn reject_pending_project_trust(path: &Path) -> Result<(), String> {
    if pending_marker_exists(path)? {
        return Err(format!(
            "project trust update is incomplete while pending marker {} exists",
            path.display()
        ));
    }
    Ok(())
}

fn open_regular_file_no_follow(path: &Path) -> Result<Option<fs::File>, String> {
    open_regular_file_no_follow_with_access(path, false)
}

fn open_regular_file_no_follow_with_access(
    path: &Path,
    writable: bool,
) -> Result<Option<fs::File>, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    if writable {
        options.write(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "failed to open project trust record {} without following links: {}",
                path.display(),
                error
            ))
        }
    };
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect {}: {}", path.display(), error))?;

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!(
                "project trust record {} must not be a reparse point",
                path.display()
            ));
        }
    }
    if !metadata.file_type().is_file() {
        return Err(format!(
            "project trust record {} must be a regular file",
            path.display()
        ));
    }

    Ok(Some(file))
}

struct ProjectTrustWriterLease {
    _file: fs::File,
}

impl ProjectTrustWriterLease {
    fn acquire(store_dir: &Path, project_id: &str) -> Result<Self, String> {
        const LOCK_TIMEOUT: Duration = Duration::from_millis(500);
        const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(5);

        create_secure_store_dir(store_dir)?;
        let path = project_trust_lock_path(store_dir, project_id);
        let mut options = fs::OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }

        let file = options.open(&path).map_err(|error| {
            format!(
                "failed to open project trust writer lock {} without following links: {}",
                path.display(),
                error
            )
        })?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {}", path.display(), error))?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(format!(
                    "project trust writer lock {} must not be a reparse point",
                    path.display()
                ));
            }
        }
        if !metadata.file_type().is_file() {
            return Err(format!(
                "project trust writer lock {} must be a regular file",
                path.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| {
                    format!(
                        "failed to set permissions on project trust writer lock {}: {}",
                        path.display(),
                        error
                    )
                })?;
        }

        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(LOCK_RETRY_INTERVAL);
                }
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(format!(
                        "project trust writer is busy for lock {} after {} ms",
                        path.display(),
                        LOCK_TIMEOUT.as_millis()
                    ));
                }
                Err(fs::TryLockError::Error(error)) => {
                    return Err(format!(
                        "failed to lock project trust writer {}: {}",
                        path.display(),
                        error
                    ));
                }
            }
        }

        Ok(Self { _file: file })
    }
}

fn persist_project_trust_record(
    project_root: &Path,
    store_dir: &Path,
    trusted: bool,
) -> Result<PathBuf, String> {
    let project_id = project_trust_id_for_root(project_root);
    let path = store_dir.join(&project_id).with_extension("json");
    let pending_path = project_trust_pending_path(store_dir, &project_id);
    let _writer_lease = ProjectTrustWriterLease::acquire(store_dir, &project_id)?;
    install_pending_marker(store_dir, &pending_path, &project_id)?;
    let record = ProjectTrustRecord {
        schema: PROJECT_TRUST_SCHEMA.to_string(),
        project_id: project_id.clone(),
        project_root: project_root_string(project_root),
        trusted,
    };
    let contents = serde_json::to_string_pretty(&record)
        .map_err(|error| format!("failed to serialize project trust: {error}"))?;
    let temporary_path = store_dir.join(format!(".{project_id}.{}.tmp", uuid::Uuid::new_v4()));
    write_synced_temporary_file(&temporary_path, format!("{contents}\n").as_bytes())?;

    if let Err(error) = replace_file_atomically(&temporary_path, &path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!(
            "failed to replace {} with {}: {}",
            path.display(),
            temporary_path.display(),
            error
        ));
    }
    let record_file = open_regular_file_no_follow_with_access(&path, true)?.ok_or_else(|| {
        format!(
            "project trust record {} disappeared before sync",
            path.display()
        )
    })?;
    record_file
        .sync_all()
        .map_err(|error| format!("failed to sync {}: {}", path.display(), error))?;
    sync_store_dir(store_dir)?;
    clear_pending_marker_at_commit(store_dir, &pending_path)?;
    Ok(path)
}

fn install_pending_marker(
    store_dir: &Path,
    pending_path: &Path,
    project_id: &str,
) -> Result<(), String> {
    let temporary_path = store_dir.join(format!(
        ".{project_id}.{}.pending.tmp",
        uuid::Uuid::new_v4()
    ));
    write_synced_temporary_file(&temporary_path, b"project trust update pending\n")?;

    if let Err(error) = replace_file_atomically(&temporary_path, pending_path) {
        let marker_already_exists = pending_marker_exists(pending_path)?;
        let _ = fs::remove_file(&temporary_path);
        if !marker_already_exists {
            return Err(format!(
                "failed to install project trust pending marker {} from {}: {}",
                pending_path.display(),
                temporary_path.display(),
                error
            ));
        }
    }

    sync_store_dir(store_dir)
}

fn remove_path_entry_no_follow(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("failed to inspect {}: {}", path.display(), error)),
    };
    let result = if metadata.file_type().is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| format!("failed to remove {}: {}", path.display(), error))
}

fn cleanup_project_trust_reset_tombstones(
    store_dir: &Path,
    project_id: &str,
) -> Result<(), String> {
    let entries = match fs::read_dir(store_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to list project trust store {}: {}",
                store_dir.display(),
                error
            ))
        }
    };
    let prefix = format!(".{project_id}.");
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "failed to inspect project trust store {}: {}",
                store_dir.display(),
                error
            )
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(&prefix) && name.ends_with(".reset") {
            remove_path_entry_no_follow(&entry.path())?;
        }
    }
    Ok(())
}

fn clear_pending_marker_at_commit(store_dir: &Path, pending_path: &Path) -> Result<(), String> {
    remove_path_entry_no_follow(pending_path)?;

    // Commit point: the record (or its reset deletion) was file- and directory-synced
    // while pending still blocked readers. If this final directory sync fails, returning
    // Err would report a failed operation that is already active. A crash can only restore
    // pending (fail closed) or preserve its removal and expose the committed state.
    let _ = sync_store_dir(store_dir);
    Ok(())
}

fn create_secure_store_dir(store_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(store_dir)
        .map_err(|error| format!("failed to create {}: {}", store_dir.display(), error))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Some(trust_dir) = store_dir.parent() {
            fs::set_permissions(trust_dir, fs::Permissions::from_mode(0o700)).map_err(|error| {
                format!(
                    "failed to set permissions on {}: {}",
                    trust_dir.display(),
                    error
                )
            })?;
        }
        fs::set_permissions(store_dir, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "failed to set permissions on {}: {}",
                store_dir.display(),
                error
            )
        })?;
    }

    Ok(())
}

fn write_synced_temporary_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .map_err(|error| format!("failed to create {}: {}", path.display(), error))?;
        file.write_all(contents)
            .map_err(|error| format!("failed to write {}: {}", path.display(), error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
                format!("failed to set permissions on {}: {}", path.display(), error)
            })?;
        }
        file.flush()
            .map_err(|error| format!("failed to flush {}: {}", path.display(), error))?;
        file.sync_all()
            .map_err(|error| format!("failed to sync {}: {}", path.display(), error))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

#[cfg(windows)]
fn replace_file_atomically(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file_atomically(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(test)]
#[derive(Debug)]
struct DirectorySyncFault {
    fail_on_call: Option<usize>,
    call_count: usize,
}

#[cfg(test)]
static DIRECTORY_SYNC_FAULT: std::sync::Mutex<DirectorySyncFault> =
    std::sync::Mutex::new(DirectorySyncFault {
        fail_on_call: None,
        call_count: 0,
    });

#[cfg(test)]
struct DirectorySyncFaultGuard;

#[cfg(test)]
#[derive(Clone)]
struct ReaderPauseHook {
    entered: std::sync::mpsc::SyncSender<()>,
    resume: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

#[cfg(test)]
static READER_PAUSE_HOOK: std::sync::Mutex<Option<ReaderPauseHook>> = std::sync::Mutex::new(None);

#[cfg(test)]
struct ReaderPauseGuard {
    resume: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

#[cfg(test)]
impl ReaderPauseGuard {
    fn resume(&self) {
        let (lock, condition) = &*self.resume;
        let mut resumed = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *resumed = true;
        condition.notify_all();
    }
}

#[cfg(test)]
impl Drop for ReaderPauseGuard {
    fn drop(&mut self) {
        self.resume();
        let mut hook = READER_PAUSE_HOOK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *hook = None;
    }
}

#[cfg(test)]
fn install_reader_pause_after_initial_pending_check(
) -> (ReaderPauseGuard, std::sync::mpsc::Receiver<()>) {
    let (entered, receiver) = std::sync::mpsc::sync_channel(1);
    let resume = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let mut hook = READER_PAUSE_HOOK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(
        hook.is_none(),
        "project trust reader pause is already active"
    );
    *hook = Some(ReaderPauseHook {
        entered,
        resume: resume.clone(),
    });
    (ReaderPauseGuard { resume }, receiver)
}

#[cfg(test)]
fn pause_reader_after_initial_pending_check() {
    let hook = READER_PAUSE_HOOK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let Some(hook) = hook else {
        return;
    };
    let _ = hook.entered.send(());
    let (lock, condition) = &*hook.resume;
    let mut resumed = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    while !*resumed {
        resumed = condition
            .wait(resumed)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

#[cfg(test)]
impl Drop for DirectorySyncFaultGuard {
    fn drop(&mut self) {
        let mut fault = DIRECTORY_SYNC_FAULT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        fault.fail_on_call = None;
        fault.call_count = 0;
    }
}

#[cfg(test)]
fn fail_directory_sync_on_call(call_index: usize) -> DirectorySyncFaultGuard {
    assert!(call_index > 0, "directory sync call indexes are one-based");
    let mut fault = DIRECTORY_SYNC_FAULT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(
        fault.fail_on_call.is_none(),
        "directory sync fault injection is already active"
    );
    fault.fail_on_call = Some(call_index);
    fault.call_count = 0;
    DirectorySyncFaultGuard
}

#[cfg(test)]
fn directory_sync_call_count() -> usize {
    DIRECTORY_SYNC_FAULT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .call_count
}

#[cfg(test)]
fn inject_directory_sync_fault(store_dir: &Path) -> Result<(), String> {
    let mut fault = DIRECTORY_SYNC_FAULT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    fault.call_count += 1;
    let call_index = fault.call_count;
    if fault.fail_on_call == Some(call_index) {
        fault.fail_on_call = None;
        return Err(format!(
            "injected directory sync failure at call {call_index} for {}",
            store_dir.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn sync_store_dir(store_dir: &Path) -> Result<(), String> {
    #[cfg(test)]
    inject_directory_sync_fault(store_dir)?;
    fs::File::open(store_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("failed to sync {}: {}", store_dir.display(), error))
}

#[cfg(not(unix))]
fn sync_store_dir(store_dir: &Path) -> Result<(), String> {
    #[cfg(test)]
    inject_directory_sync_fault(store_dir)?;
    #[cfg(not(test))]
    let _ = store_dir;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::MutexGuard;

    struct IsolatedKianaHome {
        _lock: MutexGuard<'static, ()>,
        original_kiana_home: Option<OsString>,
        original_home: Option<OsString>,
        original_user_profile: Option<OsString>,
        original_current_dir: PathBuf,
        root: PathBuf,
        kiana_home: PathBuf,
    }

    impl IsolatedKianaHome {
        fn new(label: &str) -> Self {
            let lock = crate::process_env_lock();
            let root = std::env::temp_dir().join(format!(
                "kiana-project-trust-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            let kiana_home = root.join("kiana-home");
            fs::create_dir_all(&kiana_home).unwrap();
            let original_kiana_home = std::env::var_os("KIANA_HOME");
            let original_home = std::env::var_os("HOME");
            let original_user_profile = std::env::var_os("USERPROFILE");
            let original_current_dir = std::env::current_dir().unwrap();
            std::env::set_current_dir(&root).unwrap();
            std::env::set_var("KIANA_HOME", &kiana_home);

            Self {
                _lock: lock,
                original_kiana_home,
                original_home,
                original_user_profile,
                original_current_dir,
                root,
                kiana_home,
            }
        }

        fn git_project(&self, name: &str) -> PathBuf {
            let project = self.root.join(name);
            fs::create_dir_all(project.join(".git")).unwrap();
            fs::canonicalize(project).unwrap()
        }
    }

    impl Drop for IsolatedKianaHome {
        fn drop(&mut self) {
            restore_env("KIANA_HOME", &self.original_kiana_home);
            restore_env("HOME", &self.original_home);
            restore_env("USERPROFILE", &self.original_user_profile);
            let _ = std::env::set_current_dir(&self.original_current_dir);
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn restore_env(name: &str, value: &Option<OsString>) {
        if let Some(value) = value {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }

    fn app_state_for(cwd: &Path) -> HashMap<String, Value> {
        HashMap::from([("cwd".to_string(), json!(cwd.to_string_lossy().into_owned()))])
    }

    fn pending_path_for(record_path: &Path) -> PathBuf {
        record_path.with_extension("pending")
    }

    fn reset_tombstones(store_dir: &Path, project_id: &str) -> Vec<PathBuf> {
        let prefix = format!(".{project_id}.");
        let mut paths = fs::read_dir(store_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".reset"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    #[test]
    fn writer_lease_excludes_second_holder_until_drop() {
        let env = IsolatedKianaHome::new("writer-lease");
        let project = env.git_project("project");
        let project_root = project_trust_root(&project);
        let store_dir = validated_project_trust_store_dir(&project_root).unwrap();
        let project_id = project_trust_id_for_root(&project_root);
        let lock_path = store_dir.join(&project_id).with_extension("lock");

        let first = ProjectTrustWriterLease::acquire(&store_dir, &project_id).unwrap();
        let busy_error = match ProjectTrustWriterLease::acquire(&store_dir, &project_id) {
            Ok(_) => panic!("second project trust writer lease unexpectedly acquired"),
            Err(error) => error,
        };
        assert!(
            busy_error.contains("busy"),
            "unexpected error: {busy_error}"
        );

        drop(first);
        let second = ProjectTrustWriterLease::acquire(&store_dir, &project_id).unwrap();
        drop(second);

        let metadata = fs::symlink_metadata(&lock_path).unwrap();
        assert!(metadata.file_type().is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn trust_mutations_honor_writer_lease() {
        let env = IsolatedKianaHome::new("writer-lease-integration");
        let project = env.git_project("project");
        let project_root = project_trust_root(&project);
        let store_dir = validated_project_trust_store_dir(&project_root).unwrap();
        let project_id = project_trust_id_for_root(&project_root);

        let lease = ProjectTrustWriterLease::acquire(&store_dir, &project_id).unwrap();
        let write_error = write_project_trust(&project, ProjectTrust::Trusted).unwrap_err();
        assert!(
            write_error.contains("busy"),
            "unexpected error: {write_error}"
        );
        drop(lease);

        let record_path = write_project_trust(&project, ProjectTrust::Trusted).unwrap();
        let lease = ProjectTrustWriterLease::acquire(&store_dir, &project_id).unwrap();
        let reset_error = remove_project_trust(&project).unwrap_err();
        assert!(
            reset_error.contains("busy"),
            "unexpected error: {reset_error}"
        );
        assert!(record_path.is_file());
        assert_eq!(
            read_project_trust(&project).unwrap(),
            Some(ProjectTrust::Trusted)
        );
        drop(lease);

        assert_eq!(remove_project_trust(&project).unwrap(), Some(record_path));
        assert_eq!(read_project_trust(&project).unwrap(), None);
    }

    #[test]
    fn failed_reset_sync_keeps_pending_and_moves_record_to_tombstone() {
        let env = IsolatedKianaHome::new("reset-tombstone-sync-failure");
        let project = env.git_project("project");
        let project_root = project_trust_root(&project);
        let store_dir = validated_project_trust_store_dir(&project_root).unwrap();
        let project_id = project_trust_id_for_root(&project_root);
        let record_path = write_project_trust(&project, ProjectTrust::Trusted).unwrap();
        let pending_path = pending_path_for(&record_path);

        let fault = fail_directory_sync_on_call(2);
        let reset_error = remove_project_trust(&project).unwrap_err();
        assert!(
            reset_error.contains("injected directory sync failure at call 2"),
            "unexpected error: {reset_error}"
        );
        drop(fault);

        assert!(!record_path.exists());
        assert!(fs::symlink_metadata(&pending_path).is_ok());
        let tombstones = reset_tombstones(&store_dir, &project_id);
        assert_eq!(tombstones.len(), 1, "expected one reset tombstone");
        assert!(read_project_trust(&project)
            .unwrap_err()
            .contains("pending"));
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );

        assert_eq!(remove_project_trust(&project).unwrap(), None);
        assert!(fs::symlink_metadata(&pending_path).is_err());
        assert!(reset_tombstones(&store_dir, &project_id).is_empty());
        assert_eq!(read_project_trust(&project).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_project_root_bytes_produce_distinct_ids() {
        use std::os::unix::ffi::OsStringExt;

        let env = IsolatedKianaHome::new("non-utf8-identity");
        let first = env.root.join(OsString::from_vec(b"project-\x80".to_vec()));
        let second = env.root.join(OsString::from_vec(b"project-\x81".to_vec()));
        fs::create_dir_all(first.join(".git")).unwrap();
        fs::create_dir_all(second.join(".git")).unwrap();

        assert_ne!(project_trust_id(&first), project_trust_id(&second));
    }

    #[test]
    fn missing_decision_is_unknown_and_disallows_project_resources() {
        let env = IsolatedKianaHome::new("missing");
        let project = env.git_project("project");
        let app_state = app_state_for(&project);

        assert_eq!(read_project_trust(&project).unwrap(), None);
        let trust = project_trust_from_app_state(&app_state);
        assert_eq!(trust.as_str(), "unknown");
        assert!(!trust.as_bool());
        assert!(!trust.allows_project_resources());
        assert!(!has_explicit_project_trust(&app_state));

        let mut invalid_state = app_state;
        invalid_state.insert("project_trusted".to_string(), json!("sometimes"));
        assert_eq!(
            project_trust_from_app_state(&invalid_state).as_str(),
            "unknown"
        );
        assert!(!has_explicit_project_trust(&invalid_state));

        let write_result = write_project_trust(&project, trust);
        assert!(write_result.is_err());
        assert!(!project_trust_file_path(&project).unwrap().exists());
    }

    #[test]
    fn nested_session_project_trust_is_ignored() {
        let env = IsolatedKianaHome::new("nested-session-decision");
        let stored_project = env.git_project("stored-project");
        let unknown_project = env.git_project("unknown-project");
        write_project_trust(&stored_project, ProjectTrust::Untrusted).unwrap();

        let cases = [
            (
                "trust.project",
                HashMap::from([("trust".to_string(), json!({ "project": true }))]),
            ),
            (
                "project.trusted",
                HashMap::from([("project".to_string(), json!({ "trusted": true }))]),
            ),
        ];

        for (label, invalid_decision) in cases {
            let mut stored_state = app_state_for(&stored_project);
            stored_state.extend(invalid_decision.clone());
            let mut unknown_state = app_state_for(&unknown_project);
            unknown_state.extend(invalid_decision);

            assert_eq!(
                (
                    project_trust_from_app_state(&stored_state),
                    has_explicit_project_trust(&stored_state),
                    project_trust_from_app_state(&unknown_state),
                    has_explicit_project_trust(&unknown_state),
                ),
                (ProjectTrust::Untrusted, true, ProjectTrust::Unknown, false,),
                "{label} must not be a session trust decision"
            );
        }
    }

    #[test]
    fn string_session_project_trust_is_ignored() {
        let env = IsolatedKianaHome::new("string-session-decision");
        let stored_project = env.git_project("stored-project");
        let unknown_project = env.git_project("unknown-project");
        write_project_trust(&stored_project, ProjectTrust::Untrusted).unwrap();

        for key in ["project_trusted", "projectTrusted"] {
            for value in [
                "true",
                "trusted",
                "yes",
                "on",
                "1",
                "false",
                "untrusted",
                "no",
                "off",
                "0",
            ] {
                let invalid_decision = HashMap::from([(key.to_string(), json!(value))]);
                let mut stored_state = app_state_for(&stored_project);
                stored_state.extend(invalid_decision.clone());
                let mut unknown_state = app_state_for(&unknown_project);
                unknown_state.extend(invalid_decision);

                assert_eq!(
                    (
                        project_trust_from_app_state(&stored_state),
                        has_explicit_project_trust(&stored_state),
                        project_trust_from_app_state(&unknown_state),
                        has_explicit_project_trust(&unknown_state),
                    ),
                    (ProjectTrust::Untrusted, true, ProjectTrust::Unknown, false,),
                    "{key}={value:?} must not be a session trust decision"
                );
            }
        }
    }

    #[test]
    fn project_local_trust_file_cannot_authorize_project() {
        let env = IsolatedKianaHome::new("legacy");
        let project = env.git_project("project");
        let legacy_path = project.join(".kiana").join("trust.json");
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        fs::write(&legacy_path, r#"{"trusted":true}"#).unwrap();

        assert_eq!(read_project_trust(&project).unwrap(), None);
        assert_eq!(find_project_trust_file(&project), None);
        assert_ne!(project_trust_file_path(&project).unwrap(), legacy_path);

        let app_state = app_state_for(&project);
        assert_eq!(project_trust_from_app_state(&app_state).as_str(), "unknown");
        assert!(!has_explicit_project_trust(&app_state));
    }

    #[test]
    fn user_store_trust_record_authorizes_canonical_git_root() {
        let env = IsolatedKianaHome::new("user-store");
        let project = env.git_project("project");
        let nested = project.join("src").join("nested");
        fs::create_dir_all(&nested).unwrap();

        let written_path = write_project_trust(&project, ProjectTrust::Trusted).unwrap();
        let expected_path = project_trust_file_path(&project).unwrap();
        assert_eq!(written_path, expected_path);
        assert_eq!(project_trust_file_path(&nested).unwrap(), expected_path);
        assert!(written_path.starts_with(env.kiana_home.join("trust").join("projects")));

        let project_id = written_path.file_stem().unwrap().to_str().unwrap();
        assert_eq!(project_id.len(), 64);
        assert!(project_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));

        let record: Value =
            serde_json::from_str(&fs::read_to_string(&written_path).unwrap()).unwrap();
        assert_eq!(record["schema"], "kiana.project-trust.v2");
        assert_eq!(record["project_id"], project_id);
        assert_eq!(record["project_root"], project.to_string_lossy().as_ref());
        assert_eq!(record["trusted"], true);
        assert_eq!(record.as_object().unwrap().len(), 4);

        assert_eq!(
            read_project_trust(&nested).unwrap(),
            Some(ProjectTrust::Trusted)
        );
        let app_state = app_state_for(&nested);
        assert_eq!(
            project_trust_from_app_state(&app_state),
            ProjectTrust::Trusted
        );
        assert!(has_explicit_project_trust(&app_state));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(
                fs::metadata(written_path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&written_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn nested_git_repo_does_not_inherit_parent_trust() {
        let env = IsolatedKianaHome::new("nested-git");
        let parent = env.git_project("parent");
        let parent_nested = parent.join("src");
        fs::create_dir_all(&parent_nested).unwrap();
        let child = parent.join("vendor").join("child");
        fs::create_dir_all(child.join(".git")).unwrap();
        let child = fs::canonicalize(child).unwrap();
        let child_nested = child.join("src");
        fs::create_dir_all(&child_nested).unwrap();

        write_project_trust(&parent, ProjectTrust::Trusted).unwrap();

        assert_eq!(
            read_project_trust(&parent_nested).unwrap(),
            Some(ProjectTrust::Trusted)
        );
        assert_ne!(
            project_trust_file_path(&parent).unwrap(),
            project_trust_file_path(&child_nested).unwrap()
        );
        assert_eq!(read_project_trust(&child_nested).unwrap(), None);
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&child_nested)).as_str(),
            "unknown"
        );
    }

    #[test]
    fn tampered_user_store_record_fails_closed() {
        let env = IsolatedKianaHome::new("tampered");
        let project = env.git_project("project");
        let record_path = write_project_trust(&project, ProjectTrust::Trusted).unwrap();
        let valid_record: Value =
            serde_json::from_str(&fs::read_to_string(&record_path).unwrap()).unwrap();

        let cases = [
            ("project_root", json!("/tampered/project/root")),
            ("project_id", json!("0".repeat(64))),
            ("schema", json!("kiana.project-trust.v1")),
        ];
        for (field, value) in cases {
            let mut tampered = valid_record.clone();
            tampered[field] = value;
            fs::write(&record_path, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();

            let error = read_project_trust(&project).unwrap_err();
            assert!(error.contains(field), "unexpected error: {error}");
            let app_state = app_state_for(&project);
            assert_eq!(project_trust_from_app_state(&app_state).as_str(), "unknown");
            assert!(!has_explicit_project_trust(&app_state));
        }

        let mut unknown_field = valid_record;
        unknown_field
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), json!(true));
        fs::write(
            &record_path,
            serde_json::to_vec_pretty(&unknown_field).unwrap(),
        )
        .unwrap();
        let error = read_project_trust(&project).unwrap_err();
        assert!(error.contains("unexpected"), "unexpected error: {error}");
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)).as_str(),
            "unknown"
        );
    }

    #[test]
    fn relative_kiana_home_cannot_be_trust_authority() {
        let env = IsolatedKianaHome::new("relative-home");
        let project = env.git_project("project");
        std::env::set_var("KIANA_HOME", "relative-home");

        assert!(write_project_trust(&project, ProjectTrust::Trusted).is_err());
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
        assert!(!env.root.join("relative-home").exists());
    }

    #[test]
    fn project_local_kiana_home_cannot_be_trust_authority() {
        let env = IsolatedKianaHome::new("project-local-home");
        let project = env.git_project("project");
        let project_local_home = project.join(".kiana-user-home");
        std::env::set_var("KIANA_HOME", &project_local_home);

        let project_id = project_trust_id(&project);
        let record_path = project_local_home
            .join("trust")
            .join("projects")
            .join(&project_id)
            .with_extension("json");
        fs::create_dir_all(record_path.parent().unwrap()).unwrap();
        let record = ProjectTrustRecord {
            schema: PROJECT_TRUST_SCHEMA.to_string(),
            project_id,
            project_root: project.to_string_lossy().into_owned(),
            trusted: true,
        };
        fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
        let original_contents = fs::read(&record_path).unwrap();

        assert!(read_project_trust(&project).is_err());
        assert_eq!(find_project_trust_file(&project), None);
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
        assert!(write_project_trust(&project, ProjectTrust::Untrusted).is_err());
        assert_eq!(fs::read(&record_path).unwrap(), original_contents);
        assert!(remove_project_trust(&project).is_err());
        assert!(record_path.is_file());
    }

    #[test]
    fn missing_user_home_cannot_fall_back_to_project_local_store() {
        let env = IsolatedKianaHome::new("missing-user-home");
        let project = env.git_project("project");
        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");

        let error = write_project_trust(&project, ProjectTrust::Trusted).unwrap_err();
        assert!(error.contains("user home"), "unexpected error: {error}");
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
        assert!(!env.root.join(".kiana").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_user_store_cannot_redirect_trust_authority_into_project() {
        use std::os::unix::fs::symlink;

        let env = IsolatedKianaHome::new("symlinked-store");
        let project = env.git_project("project");
        let project_controlled_store = project.join("project-controlled-trust");
        fs::create_dir_all(&project_controlled_store).unwrap();
        symlink(&project_controlled_store, env.kiana_home.join("trust")).unwrap();

        assert!(write_project_trust(&project, ProjectTrust::Trusted).is_err());
        assert!(read_project_trust(&project).is_err());
        assert_eq!(find_project_trust_file(&project), None);
        assert!(remove_project_trust(&project).is_err());
        assert!(fs::read_dir(&project_controlled_store)
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn public_trust_path_rejects_invalid_store_boundaries() {
        let env = IsolatedKianaHome::new("checked-public-path");
        let project = env.git_project("project");

        std::env::set_var("KIANA_HOME", "relative-home");
        assert!(project_trust_file_path(&project).is_err());

        std::env::set_var("KIANA_HOME", project.join("project-local-home"));
        assert!(project_trust_file_path(&project).is_err());

        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
        assert!(project_trust_file_path(&project).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            std::env::set_var("KIANA_HOME", &env.kiana_home);
            let project_controlled_store = project.join("redirected-store");
            fs::create_dir_all(&project_controlled_store).unwrap();
            symlink(&project_controlled_store, env.kiana_home.join("trust")).unwrap();
            assert!(project_trust_file_path(&project).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_trust_record_cannot_authorize_project() {
        use std::os::unix::fs::symlink;

        let env = IsolatedKianaHome::new("symlinked-record");
        let project = env.git_project("project");
        let project_id = project_trust_id(&project);
        let project_controlled_record = project.join("trusted.json");
        let record = ProjectTrustRecord {
            schema: PROJECT_TRUST_SCHEMA.to_string(),
            project_id,
            project_root: project.to_string_lossy().into_owned(),
            trusted: true,
        };
        fs::write(
            &project_controlled_record,
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();

        let record_path = project_trust_file_path(&project).unwrap();
        fs::create_dir_all(record_path.parent().unwrap()).unwrap();
        symlink(&project_controlled_record, &record_path).unwrap();

        assert_eq!(find_project_trust_file(&project), None);
        assert!(read_project_trust(&project).is_err());
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
    }

    #[test]
    fn existing_trust_record_can_be_replaced_and_reset_fail_closed() {
        let env = IsolatedKianaHome::new("replace-reset");
        let project = env.git_project("project");

        let trusted_path = write_project_trust(&project, ProjectTrust::Trusted).unwrap();
        let untrusted_path = write_project_trust(&project, ProjectTrust::Untrusted).unwrap();
        assert_eq!(trusted_path, untrusted_path);
        assert_eq!(
            read_project_trust(&project).unwrap(),
            Some(ProjectTrust::Untrusted)
        );

        assert_eq!(
            remove_project_trust(&project).unwrap(),
            Some(untrusted_path.clone())
        );
        assert!(!untrusted_path.exists());
        assert_eq!(read_project_trust(&project).unwrap(), None);
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
        assert_eq!(remove_project_trust(&project).unwrap(), None);
    }

    #[test]
    fn failed_record_directory_sync_never_activates_trusted_write() {
        let env = IsolatedKianaHome::new("failed-record-directory-sync");
        let project = env.git_project("project");
        let record_path = project_trust_file_path(&project).unwrap();
        let pending_path = pending_path_for(&record_path);
        let _fault = fail_directory_sync_on_call(2);

        let write_error = match write_project_trust(&project, ProjectTrust::Trusted) {
            Ok(_) => panic!(
                "Trusted write unexpectedly succeeded despite injected record directory sync failure"
            ),
            Err(error) => error,
        };
        assert!(
            write_error.contains("injected directory sync failure at call 2"),
            "unexpected error: {write_error}"
        );
        assert!(fs::symlink_metadata(&pending_path).is_ok());
        assert_eq!(find_project_trust_file(&project), None);
        let read_error = read_project_trust(&project).unwrap_err();
        assert!(
            read_error.contains("pending"),
            "unexpected error: {read_error}"
        );
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );

        let recovered_path = write_project_trust(&project, ProjectTrust::Untrusted).unwrap();
        assert_eq!(recovered_path, record_path);
        assert!(fs::symlink_metadata(&pending_path).is_err());
        assert_eq!(
            read_project_trust(&project).unwrap(),
            Some(ProjectTrust::Untrusted)
        );

        assert_eq!(
            remove_project_trust(&project).unwrap(),
            Some(record_path.clone())
        );
        assert!(fs::symlink_metadata(&record_path).is_err());
        assert!(fs::symlink_metadata(&pending_path).is_err());
        assert_eq!(read_project_trust(&project).unwrap(), None);
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
    }

    #[test]
    fn reset_clears_stale_pending_without_a_record() {
        let env = IsolatedKianaHome::new("stale-pending-no-record");
        let project = env.git_project("project");
        let record_path = project_trust_file_path(&project).unwrap();
        let pending_path = pending_path_for(&record_path);
        fs::create_dir_all(pending_path.parent().unwrap()).unwrap();
        fs::write(&pending_path, b"pending\n").unwrap();

        assert!(read_project_trust(&project).is_err());
        assert_eq!(find_project_trust_file(&project), None);
        assert_eq!(remove_project_trust(&project).unwrap(), None);
        assert!(fs::symlink_metadata(&record_path).is_err());
        assert!(fs::symlink_metadata(&pending_path).is_err());
        assert_eq!(read_project_trust(&project).unwrap(), None);
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
    }

    #[test]
    fn non_regular_pending_marker_fails_closed_and_can_be_reset() {
        let env = IsolatedKianaHome::new("non-regular-pending");
        let project = env.git_project("project");
        let record_path = project_trust_file_path(&project).unwrap();
        let pending_path = pending_path_for(&record_path);
        fs::create_dir_all(&pending_path).unwrap();

        assert!(read_project_trust(&project).is_err());
        assert_eq!(find_project_trust_file(&project), None);
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
        assert_eq!(remove_project_trust(&project).unwrap(), None);
        assert!(fs::symlink_metadata(&pending_path).is_err());
        assert_eq!(read_project_trust(&project).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_pending_marker_fails_closed_and_can_be_reset() {
        use std::os::unix::fs::symlink;

        let env = IsolatedKianaHome::new("symlinked-pending");
        let project = env.git_project("project");
        let record_path = project_trust_file_path(&project).unwrap();
        let pending_path = pending_path_for(&record_path);
        fs::create_dir_all(pending_path.parent().unwrap()).unwrap();
        symlink(project.join("missing-target"), &pending_path).unwrap();

        assert!(read_project_trust(&project).is_err());
        assert_eq!(find_project_trust_file(&project), None);
        assert_eq!(remove_project_trust(&project).unwrap(), None);
        assert!(fs::symlink_metadata(&pending_path).is_err());
        assert_eq!(read_project_trust(&project).unwrap(), None);
    }

    #[test]
    fn final_directory_sync_failure_is_already_committed() {
        let env = IsolatedKianaHome::new("failed-final-directory-sync");
        let project = env.git_project("project");
        let record_path = project_trust_file_path(&project).unwrap();
        let pending_path = pending_path_for(&record_path);
        let _fault = fail_directory_sync_on_call(3);

        assert_eq!(
            write_project_trust(&project, ProjectTrust::Trusted).unwrap(),
            record_path
        );
        assert_eq!(directory_sync_call_count(), 3);
        assert!(fs::symlink_metadata(&pending_path).is_err());
        assert_eq!(
            read_project_trust(&project).unwrap(),
            Some(ProjectTrust::Trusted)
        );
    }

    #[test]
    fn concurrent_reader_rejects_trusted_record_from_failed_write() {
        let env = IsolatedKianaHome::new("concurrent-failed-trusted-write");
        let project = env.git_project("project");
        let (reader_pause, reader_entered) = install_reader_pause_after_initial_pending_check();
        let reader_project = project.clone();
        let reader = std::thread::spawn(move || read_project_trust(&reader_project));

        reader_entered
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("reader must pause after its initial pending check");
        let _fault = fail_directory_sync_on_call(2);
        let write_error = write_project_trust(&project, ProjectTrust::Trusted).unwrap_err();
        assert!(
            write_error.contains("injected directory sync failure at call 2"),
            "unexpected error: {write_error}"
        );

        reader_pause.resume();
        let read_error = reader
            .join()
            .expect("reader thread must not panic")
            .unwrap_err();
        assert!(
            read_error.contains("pending"),
            "concurrent reader must fail closed, got: {read_error}"
        );
        assert_eq!(
            project_trust_from_app_state(&app_state_for(&project)),
            ProjectTrust::Unknown
        );
    }
}
