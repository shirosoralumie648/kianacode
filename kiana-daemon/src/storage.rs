//! Daemon storage-root resolver and single-writer lease.
//!
//! Resolution is shared by every daemon-backed surface. It derives a user-level storage root,
//! keeps it outside the project tree, persists StoreIdentity, and acquires one create-new lock;
//! storage itself never becomes a second ControlPlane execution path.
use kiana_domain::{
    canonical_journal_bytes, StorageBackend, StorageLockRecord, StorageNamespace,
    StorageOwnerScope, StorageRoot, StoreIdentity,
};
use kiana_ports::PortError;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const KIANA_HOME_ENV: &str = "KIANA_HOME";
const HOME_ENV: &str = "HOME";
const STORE_IDENTITY_FILE: &str = "store-identity.json";
const STORAGE_LOCK_FILE: &str = "storage.lock";

pub fn resolve_storage_root(
    project_root: &Path,
    owner_id: impl Into<String>,
    instance_id: impl Into<String>,
    authority_epoch: u64,
) -> Result<StorageRoot, PortError> {
    let project = fs::canonicalize(project_root)
        .map_err(|error| PortError::Failed(format!("storage_project_root_invalid:{error}")))?;
    if !project.is_dir() {
        return Err(PortError::Failed(
            "storage_project_root_not_directory".to_owned(),
        ));
    }
    let configured = std::env::var_os(KIANA_HOME_ENV)
        .or_else(|| {
            std::env::var_os(HOME_ENV)
                .map(|home| PathBuf::from(home).join(".kiana").into_os_string())
        })
        .ok_or_else(|| PortError::Failed("storage_home_required".to_owned()))?;
    let configured = PathBuf::from(configured);
    if !configured.is_absolute() {
        return Err(PortError::Failed(
            "storage_root_must_be_absolute".to_owned(),
        ));
    }
    let root = canonicalize_nonexistent(&configured)?;
    if root.starts_with(&project) {
        return Err(PortError::Failed("storage_root_inside_project".to_owned()));
    }
    let owner_scope = StorageOwnerScope::new(owner_id, instance_id, None, authority_epoch)
        .map_err(PortError::Failed)?;
    let backend = detect_backend(&root)?;
    StorageRoot::new(root.to_string_lossy().into_owned(), backend, owner_scope)
        .map_err(PortError::Failed)
}

#[derive(Debug)]
pub struct StorageLease {
    root: StorageRoot,
    identity: StoreIdentity,
    record: StorageLockRecord,
    lock_path: PathBuf,
    lock_file: Option<File>,
}

impl StorageLease {
    pub fn acquire(root: StorageRoot) -> Result<Self, PortError> {
        root.validate().map_err(PortError::Failed)?;
        let root_path = PathBuf::from(&root.canonical_path);
        for namespace in [
            StorageNamespace::Meta,
            StorageNamespace::Facts,
            StorageNamespace::Projections,
            StorageNamespace::Artifacts,
            StorageNamespace::Memory,
            StorageNamespace::Indexes,
            StorageNamespace::Checkpoints,
            StorageNamespace::Backups,
            StorageNamespace::Migrations,
            StorageNamespace::Quarantine,
            StorageNamespace::Locks,
        ] {
            let path = root.namespace_path(namespace).map_err(PortError::Failed)?;
            fs::create_dir_all(path)
                .map_err(|error| PortError::Failed(format!("storage_namespace_create:{error}")))?;
        }
        let now = now_unix_ms();
        let identity_path = root_path
            .join(StorageNamespace::Meta.as_str())
            .join(STORE_IDENTITY_FILE);
        let identity = load_or_create_identity(&root, &identity_path, now)?;
        let lock_path = root_path
            .join(StorageNamespace::Locks.as_str())
            .join(STORAGE_LOCK_FILE);
        reject_symlink(&lock_path, "storage_lock_symlink")?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut lock_file = match options.open(&lock_path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(lock_conflict(&lock_path, &identity, &root.owner_scope));
            }
            Err(error) => {
                return Err(PortError::Failed(format!("storage_lock_open:{error}")));
            }
        };
        let record =
            StorageLockRecord::new(&identity, &root.owner_scope, now).map_err(PortError::Failed)?;
        let bytes = canonical_journal_bytes(&record).map_err(PortError::Failed)?;
        lock_file
            .write_all(&bytes)
            .and_then(|_| lock_file.sync_all())
            .map_err(|error| PortError::Failed(format!("storage_lock_sync:{error}")))?;
        Ok(Self {
            root,
            identity,
            record,
            lock_path,
            lock_file: Some(lock_file),
        })
    }

    pub fn root(&self) -> &StorageRoot {
        &self.root
    }

    pub fn identity(&self) -> &StoreIdentity {
        &self.identity
    }

    pub fn record(&self) -> &StorageLockRecord {
        &self.record
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    pub fn release(mut self) -> Result<(), PortError> {
        self.cleanup();
        Ok(())
    }

    fn cleanup(&mut self) {
        self.lock_file.take();
        if !is_symlink(&self.lock_path) {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

impl Drop for StorageLease {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn load_or_create_identity(
    root: &StorageRoot,
    path: &Path,
    now: u64,
) -> Result<StoreIdentity, PortError> {
    if path.exists() {
        reject_symlink(path, "storage_identity_symlink")?;
        let bytes = fs::read(path)
            .map_err(|error| PortError::Failed(format!("storage_identity_read:{error}")))?;
        let identity: StoreIdentity = serde_json::from_slice(&bytes)
            .map_err(|_| PortError::Failed("storage_identity_invalid".to_owned()))?;
        identity
            .validate(root)
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        return Ok(identity);
    }
    let identity = StoreIdentity::new(root, 1, 1, now).map_err(PortError::Failed)?;
    let bytes = canonical_journal_bytes(&identity).map_err(PortError::Failed)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| PortError::Failed(format!("storage_identity_create:{error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| PortError::Failed(format!("storage_identity_permissions:{error}")))?;
    }
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| PortError::Failed(format!("storage_identity_sync:{error}")))?;
    Ok(identity)
}

fn lock_conflict(
    path: &Path,
    identity: &StoreIdentity,
    owner_scope: &StorageOwnerScope,
) -> PortError {
    if let Ok(bytes) = fs::read(path) {
        if let Ok(record) = serde_json::from_slice::<StorageLockRecord>(&bytes) {
            if record.store_id != identity.store_id
                || record.owner_id != owner_scope.owner_id
                || record.instance_id != owner_scope.instance_id
            {
                return PortError::Conflict("storage_lock_owner_mismatch".to_owned());
            }
        }
    }
    PortError::Conflict("storage_lock_conflict".to_owned())
}

fn reject_symlink(path: &Path, reason: &str) -> Result<(), PortError> {
    if fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(PortError::Failed(reason.to_owned()));
    }
    Ok(())
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn canonicalize_nonexistent(path: &Path) -> Result<PathBuf, PortError> {
    if path.exists() {
        return fs::canonicalize(path)
            .map_err(|error| PortError::Failed(format!("storage_root_canonicalize:{error}")));
    }
    let parent = path
        .parent()
        .ok_or_else(|| PortError::Failed("storage_root_parent_missing".to_owned()))?;
    let parent = fs::canonicalize(parent)
        .map_err(|error| PortError::Failed(format!("storage_root_parent_invalid:{error}")))?;
    let name = path
        .file_name()
        .ok_or_else(|| PortError::Failed("storage_root_name_missing".to_owned()))?;
    Ok(parent.join(name))
}

fn detect_backend(path: &Path) -> Result<StorageBackend, PortError> {
    #[cfg(target_os = "linux")]
    {
        let mounts = fs::read_to_string("/proc/mounts")
            .map_err(|error| PortError::Failed(format!("storage_mount_probe_failed:{error}")))?;
        let mut current = path;
        while !current.exists() {
            let Some(parent) = current.parent() else {
                break;
            };
            if parent == current {
                break;
            }
            current = parent;
        }
        for line in mounts.lines() {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 3 {
                continue;
            }
            let mount = fields[1].replace("\\040", " ");
            let filesystem = fields[2];
            if matches!(filesystem, "nfs" | "nfs4" | "cifs" | "smbfs" | "sshfs")
                && current.starts_with(Path::new(&mount))
            {
                return Ok(StorageBackend::NetworkFilesystem);
            }
        }
    }
    Ok(StorageBackend::LocalFilesystem)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
