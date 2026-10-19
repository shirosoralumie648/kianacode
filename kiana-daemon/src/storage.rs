//! Daemon storage-root resolver and single-writer lease.
//!
//! Resolution is shared by every daemon-backed surface. It derives a user-level storage root,
//! keeps it outside the project tree, persists StoreIdentity, and acquires one create-new lock;
//! storage itself never becomes a second ControlPlane execution path.
use crate::local_packages::LocalDir;
use kiana_domain::{
    canonical_journal_bytes, json_digest, StorageBackend, StorageLockRecord, StorageNamespace,
    StorageOwnerScope, StorageRoot, StorageRootId, StoreIdentity,
};
use kiana_ports::PortError;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const KIANA_HOME_ENV: &str = "KIANA_HOME";
const HOME_ENV: &str = "HOME";
const STORE_IDENTITY_FILE: &str = "store-identity.json";
const STORAGE_LOCK_FILE: &str = "storage.lock";
const MAX_STORAGE_METADATA_BYTES: usize = 16 * 1024;

/// Binding schema for the surface-to-root record used by CLI/Web/Workbench.
pub const STORAGE_SURFACE_BINDING_SCHEMA: &str = "kiana.storage-surface-binding.v1";
/// Restart-stable local installation identity. Every product surface resolves the store with this
/// instance so the persisted `StoreIdentity` is reused across processes instead of being rebound
/// to a per-run process id.
pub const LOCAL_STORAGE_INSTANCE_ID: &str = "kiana-local-store";
/// Frozen authority epoch for the local product store. Advancement is a future migration concern
/// (PD-12+/SC-08), not something a surface may choose per invocation.
pub const LOCAL_STORAGE_AUTHORITY_EPOCH: u64 = 1;

/// The product surfaces that must observe the same daemon-owned storage root.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageSurface {
    Cli,
    Web,
    Workbench,
}

impl StorageSurface {
    pub const ALL: [StorageSurface; 3] = [Self::Cli, Self::Web, Self::Workbench];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Web => "web",
            Self::Workbench => "workbench",
        }
    }
}

/// Evidence record that one surface resolved to one storage root.
///
/// The record is deliberately scoped to the local product store: `validate` rejects any instance
/// or authority epoch other than the frozen local constants. Its digest binds the root identity
/// (root id, canonical path, owner scope, instance, epoch) together with the `surface` label, so
/// two surfaces that agree on the store share every root-derived field and differ only in
/// `surface` and the dependent `binding_digest`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageSurfaceBinding {
    pub schema: String,
    pub surface: StorageSurface,
    pub root_id: StorageRootId,
    pub canonical_path: String,
    pub owner_scope_digest: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub binding_digest: String,
}

impl StorageSurfaceBinding {
    pub fn new(surface: StorageSurface, root: &StorageRoot) -> Result<Self, String> {
        root.validate()?;
        let mut binding = Self {
            schema: STORAGE_SURFACE_BINDING_SCHEMA.to_owned(),
            surface,
            root_id: root.root_id,
            canonical_path: root.canonical_path.clone(),
            owner_scope_digest: root.owner_scope.scope_digest.clone(),
            instance_id: root.owner_scope.instance_id.clone(),
            authority_epoch: root.owner_scope.authority_epoch,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_SURFACE_BINDING_SCHEMA || self.root_id.as_uuid().is_nil() {
            return Err("storage_surface_binding_header_invalid".to_owned());
        }
        if self.instance_id != LOCAL_STORAGE_INSTANCE_ID
            || self.authority_epoch != LOCAL_STORAGE_AUTHORITY_EPOCH
        {
            return Err("storage_surface_binding_scope_invalid".to_owned());
        }
        if self.canonical_path.trim().is_empty() {
            return Err("storage_surface_binding_path_invalid".to_owned());
        }
        require_digest(
            &self.owner_scope_digest,
            "storage_surface_binding_owner_scope_digest",
        )?;
        require_digest(&self.binding_digest, "storage_surface_binding_digest")?;
        if self.binding_digest != self.digest() {
            return Err("storage_surface_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "surface": self.surface,
            "root_id": self.root_id,
            "canonical_path": self.canonical_path,
            "owner_scope_digest": self.owner_scope_digest,
            "instance_id": self.instance_id,
            "authority_epoch": self.authority_epoch,
        }))
    }
}

/// Resolve the one local storage root that CLI, Web and Workbench must share.
///
/// Every surface calls this same function, so the derived root id, canonical path and owner scope
/// digest are identical for all of them, and the frozen local instance/epoch make the persisted
/// `StoreIdentity` reusable across process restarts. Resolution is metadata-only: it never
/// acquires the writer lease, authorizes a command or opens a second execution path.
pub fn resolve_surface_storage_root(
    surface: StorageSurface,
    project_root: &Path,
    owner_id: impl Into<String>,
) -> Result<(StorageRoot, StorageSurfaceBinding), PortError> {
    let root = resolve_storage_root(
        project_root,
        owner_id,
        LOCAL_STORAGE_INSTANCE_ID,
        LOCAL_STORAGE_AUTHORITY_EPOCH,
    )?;
    let binding = StorageSurfaceBinding::new(surface, &root).map_err(PortError::Failed)?;
    Ok((root, binding))
}

/// Resolve the shared surface root and open the single-writer lease on it.
pub fn acquire_surface_storage(
    surface: StorageSurface,
    project_root: &Path,
    owner_id: impl Into<String>,
) -> Result<(StorageLease, StorageSurfaceBinding), PortError> {
    let (root, binding) = resolve_surface_storage_root(surface, project_root, owner_id)?;
    let lease = StorageLease::acquire(root)?;
    Ok((lease, binding))
}

fn require_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

pub fn resolve_storage_root(
    project_root: &Path,
    owner_id: impl Into<String>,
    instance_id: impl Into<String>,
    authority_epoch: u64,
) -> Result<StorageRoot, PortError> {
    reject_symlink_components(project_root, "storage_project_root_symlink")?;
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
    reject_symlink_components(&configured, "storage_root_symlink")?;
    let root = canonicalize_nonexistent(&configured)?;
    if root.parent().is_none() {
        return Err(PortError::Failed(
            "storage_root_filesystem_root_denied".to_owned(),
        ));
    }
    if root.exists() && !root.is_dir() {
        return Err(PortError::Failed("storage_root_not_directory".to_owned()));
    }
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
    lock_directory: LocalDir,
    lock_file: Option<File>,
}

impl StorageLease {
    pub fn acquire(root: StorageRoot) -> Result<Self, PortError> {
        root.validate().map_err(PortError::Failed)?;
        let root_path = PathBuf::from(&root.canonical_path);
        let directory = LocalDir::open(&root_path, true).map_err(storage_io_error)?;
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
            directory
                .subdir(namespace.as_str(), true)
                .map_err(storage_io_error)?;
        }
        let now = now_unix_ms();
        let meta_directory = directory
            .subdir(StorageNamespace::Meta.as_str(), false)
            .map_err(storage_io_error)?;
        let identity = load_or_create_identity(&root, &meta_directory, now)?;
        let lock_directory = directory
            .subdir(StorageNamespace::Locks.as_str(), false)
            .map_err(storage_io_error)?;
        let lock_path = root_path
            .join(StorageNamespace::Locks.as_str())
            .join(STORAGE_LOCK_FILE);
        let mut lock_file = match lock_directory.create_new_file(STORAGE_LOCK_FILE) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(lock_conflict(&lock_directory, &identity, &root.owner_scope));
            }
            Err(error) => {
                return Err(PortError::Failed(format!("storage_lock_open:{error}")));
            }
        };
        let record = (|| {
            let record = StorageLockRecord::new(&identity, &root.owner_scope, now)
                .map_err(PortError::Failed)?;
            let bytes = canonical_journal_bytes(&record).map_err(PortError::Failed)?;
            lock_file
                .write_all(&bytes)
                .and_then(|_| lock_file.sync_all())
                .map_err(|error| PortError::Failed(format!("storage_lock_sync:{error}")))?;
            Ok::<_, PortError>(record)
        })();
        let record = match record {
            Ok(record) => record,
            Err(initialization_error) => {
                if let Err(cleanup_error) = lock_directory
                    .remove_owned_file(STORAGE_LOCK_FILE, &lock_file)
                    .map_err(storage_io_error)
                {
                    return Err(PortError::Failed(format!(
                        "storage_lock_initialization_cleanup_failed:setup={initialization_error};cleanup={cleanup_error}"
                    )));
                }
                return Err(initialization_error);
            }
        };
        Ok(Self {
            root,
            identity,
            record,
            lock_path,
            lock_directory,
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
        self.cleanup()
    }

    fn cleanup(&mut self) -> Result<(), PortError> {
        if let Some(file) = self.lock_file.take() {
            self.lock_directory
                .remove_owned_file(STORAGE_LOCK_FILE, &file)
                .map_err(storage_io_error)?;
        }
        Ok(())
    }
}

impl Drop for StorageLease {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn load_or_create_identity(
    root: &StorageRoot,
    directory: &LocalDir,
    now: u64,
) -> Result<StoreIdentity, PortError> {
    if let Some(bytes) = directory
        .read_optional(STORE_IDENTITY_FILE, MAX_STORAGE_METADATA_BYTES)
        .map_err(storage_io_error)?
    {
        let identity: StoreIdentity = serde_json::from_slice(&bytes)
            .map_err(|_| PortError::Failed("storage_identity_invalid".to_owned()))?;
        identity
            .validate(root)
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        return Ok(identity);
    }
    let identity = StoreIdentity::new(root, 1, 1, now).map_err(PortError::Failed)?;
    let bytes = canonical_journal_bytes(&identity).map_err(PortError::Failed)?;
    directory
        .publish(STORE_IDENTITY_FILE, &bytes)
        .map_err(storage_io_error)?;
    Ok(identity)
}

fn lock_conflict(
    directory: &LocalDir,
    identity: &StoreIdentity,
    owner_scope: &StorageOwnerScope,
) -> PortError {
    match directory
        .read(STORAGE_LOCK_FILE, MAX_STORAGE_METADATA_BYTES)
        .map_err(storage_io_error)
    {
        Ok(bytes) => {
            let record = match serde_json::from_slice::<StorageLockRecord>(&bytes) {
                Ok(record) => record,
                Err(_) => return PortError::Failed("storage_lock_corrupt".to_owned()),
            };
            if record.schema != kiana_domain::STORAGE_LOCK_SCHEMA
                || record.lock_id.as_uuid().is_nil()
            {
                return PortError::Failed("storage_lock_corrupt".to_owned());
            }
            if record.validate_integrity().is_err() {
                return PortError::Failed("storage_lock_corrupt".to_owned());
            }
            if record.store_id != identity.store_id
                || record.owner_id != owner_scope.owner_id
                || record.instance_id != owner_scope.instance_id
                || identity.owner_scope_digest != owner_scope.scope_digest
                || identity.instance_id != owner_scope.instance_id
            {
                return PortError::Conflict("storage_lock_owner_mismatch".to_owned());
            }
            if record.validate(identity, owner_scope).is_err() {
                return PortError::Failed("storage_lock_corrupt".to_owned());
            }
        }
        Err(error) => return error,
    }
    PortError::Conflict("storage_lock_conflict".to_owned())
}

fn storage_io_error(error: PortError) -> PortError {
    match error {
        PortError::Failed(reason) if reason == "local_package_platform_unsupported" => {
            PortError::Failed("storage_platform_unsupported".to_owned())
        }
        PortError::Failed(reason) => PortError::Failed(reason.replacen("package_", "storage_", 1)),
        error => error,
    }
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

/// Reject symlink components before canonicalization so a configured root cannot silently move
/// across the project or trust boundary. Missing leaf components are allowed for later creation;
/// every existing component is checked with no-follow metadata.
fn reject_symlink_components(path: &Path, reason: &str) -> Result<(), PortError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(PortError::Failed(format!("{reason}:parent_component")));
            }
            Component::Normal(part) => current.push(part),
        }
        // A bare drive prefix (`C:`) or the root separator alone is not a filesystem object on
        // Windows; probing it returns ERROR_INVALID_FUNCTION. Only probe the path once it names a
        // real entry, i.e. once a normal component has been appended.
        #[cfg(windows)]
        if !matches!(component, Component::Normal(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(PortError::Failed(reason.to_owned()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(PortError::Failed(format!("{reason}:probe:{error}")));
            }
        }
    }
    Ok(())
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
