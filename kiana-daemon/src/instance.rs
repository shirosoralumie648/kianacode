//! Local daemon instance identity, ready record and single-instance lock.
//!
//! The lock/record are discovery aids only.  A discovered peer must still pass protocol,
//! workspace and authority-epoch checks before a client sends commands.

use kiana_domain::parse_bounded_json;
use kiana_ports::PortError;
use kiana_protocol::{UiInstanceRecord, UiTransportKind, PROTOCOL_SCHEMA};
use serde_json::to_vec;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const INSTANCE_DIR: &str = ".kiana/instances";
const LOCK_NAME: &str = "instance.lock";
const MAX_INSTANCE_RECORD_BYTES: u64 = 64 * 1024;

pub struct InstanceLease {
    record: UiInstanceRecord,
    lock_path: PathBuf,
    record_path: PathBuf,
    lock_file: Option<File>,
}

impl InstanceLease {
    pub fn acquire(
        workspace: impl AsRef<Path>,
        transport: UiTransportKind,
        endpoint: &str,
    ) -> Result<Self, PortError> {
        Self::acquire_with_identity(
            workspace,
            transport,
            endpoint,
            kiana_domain::RunId::new().to_string(),
            next_epoch(),
        )
    }

    pub fn acquire_with_identity(
        workspace: impl AsRef<Path>,
        transport: UiTransportKind,
        endpoint: &str,
        instance_id: String,
        authority_epoch: u64,
    ) -> Result<Self, PortError> {
        let workspace = canonical_workspace(workspace.as_ref())?;
        let directory = workspace.join(INSTANCE_DIR);
        ensure_directory(&directory)?;
        let lock_path = directory.join(LOCK_NAME);
        reject_alias(&lock_path)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let lock_file = options.open(&lock_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                PortError::Conflict("ui_instance_already_running".to_owned())
            } else {
                PortError::Failed(format!("ui_instance_lock_failed:{error}"))
            }
        })?;
        if instance_id.trim().is_empty()
            || instance_id.len() > 256
            || instance_id.contains('/')
            || instance_id.contains('\\')
            || instance_id.contains('\0')
        {
            let _ = fs::remove_file(&lock_path);
            return Err(PortError::Failed("ui_instance_id_invalid".to_owned()));
        }
        let record_path = directory.join(format!("{instance_id}.json"));
        if reject_alias(&record_path).is_err() || record_path.exists() {
            let _ = fs::remove_file(&lock_path);
            return Err(PortError::Conflict("ui_instance_record_exists".to_owned()));
        }
        let record = UiInstanceRecord::new(
            instance_id,
            authority_epoch,
            workspace.to_string_lossy().as_ref(),
            transport,
            endpoint,
            std::process::id(),
            true,
        )
        .map_err(PortError::Failed)?;
        let bytes = to_vec(&record).map_err(|error| PortError::Failed(error.to_string()))?;
        if bytes.len() as u64 > MAX_INSTANCE_RECORD_BYTES {
            let _ = fs::remove_file(&lock_path);
            return Err(PortError::Failed("ui_instance_record_too_large".to_owned()));
        }
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&record_path)
        {
            Ok(file) => file,
            Err(error) => {
                let _ = fs::remove_file(&lock_path);
                return Err(PortError::Failed(format!(
                    "ui_instance_record_write_failed:{error}"
                )));
            }
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
                drop(file);
                let _ = fs::remove_file(&record_path);
                let _ = fs::remove_file(&lock_path);
                return Err(PortError::Failed(format!(
                    "ui_instance_record_permissions:{error}"
                )));
            }
        }
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&record_path);
            let _ = fs::remove_file(&lock_path);
            return Err(PortError::Failed(format!(
                "ui_instance_record_sync_failed:{error}"
            )));
        }
        Ok(Self {
            record,
            lock_path,
            record_path,
            lock_file: Some(lock_file),
        })
    }

    pub fn record(&self) -> &UiInstanceRecord {
        &self.record
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    pub fn record_path(&self) -> &Path {
        &self.record_path
    }

    pub fn release(mut self) -> Result<(), PortError> {
        self.cleanup();
        Ok(())
    }

    fn cleanup(&mut self) {
        // Drop the descriptor before removing the lock so another process never observes a
        // removable lock while this process still owns it.
        self.lock_file.take();
        if !is_alias(&self.record_path) {
            let _ = fs::remove_file(&self.record_path);
        }
        if !is_alias(&self.lock_path) {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

impl Drop for InstanceLease {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub fn discover(workspace: impl AsRef<Path>) -> Result<UiInstanceRecord, PortError> {
    let workspace = canonical_workspace(workspace.as_ref())?;
    let directory = workspace.join(INSTANCE_DIR);
    ensure_directory_readable(&directory)?;
    let lock_path = directory.join(LOCK_NAME);
    let lock_meta = fs::symlink_metadata(&lock_path)
        .map_err(|_| PortError::Unavailable("ui_instance_not_running".to_owned()))?;
    if lock_meta.file_type().is_symlink() || !lock_meta.is_file() {
        return Err(PortError::Unavailable(
            "ui_instance_lock_invalid".to_owned(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if lock_meta.permissions().mode() & 0o077 != 0 {
            return Err(PortError::Unavailable(
                "ui_instance_lock_permissions_invalid".to_owned(),
            ));
        }
    }
    let mut records = fs::read_dir(&directory)
        .map_err(|error| PortError::Failed(format!("ui_instance_discovery_failed:{error}")))?
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|entry| entry.file_name());
    if records.len() != 1 {
        return Err(if records.is_empty() {
            PortError::Unavailable("ui_instance_not_ready".to_owned())
        } else {
            PortError::Conflict("ui_instance_duplicate_records".to_owned())
        });
    }
    let path = records.remove(0).path();
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| PortError::Failed(format!("ui_instance_record_stat_failed:{error}")))?;
    if metadata.file_type().is_symlink() || metadata.len() > MAX_INSTANCE_RECORD_BYTES {
        return Err(PortError::Failed("ui_instance_record_invalid".to_owned()));
    }
    let bytes = fs::read(&path)
        .map_err(|error| PortError::Failed(format!("ui_instance_record_read_failed:{error}")))?;
    let record: UiInstanceRecord =
        serde_json::from_value(parse_bounded_json(&bytes).map_err(PortError::Failed)?)
            .map_err(|_| PortError::Failed("ui_instance_record_invalid".to_owned()))?;
    record
        .validate_peer(&workspace.to_string_lossy(), PROTOCOL_SCHEMA, None)
        .map_err(PortError::Failed)?;
    Ok(record)
}

pub fn validate_peer(
    record: &UiInstanceRecord,
    workspace: impl AsRef<Path>,
    expected_epoch: Option<u64>,
) -> Result<(), PortError> {
    let workspace = canonical_workspace(workspace.as_ref())?;
    record
        .validate_peer(
            &workspace.to_string_lossy(),
            PROTOCOL_SCHEMA,
            expected_epoch,
        )
        .map_err(PortError::Failed)
}

fn canonical_workspace(path: &Path) -> Result<PathBuf, PortError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PortError::Failed("ui_workspace_invalid".to_owned()));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| PortError::Failed("ui_workspace_unavailable".to_owned()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PortError::Failed("ui_workspace_invalid".to_owned()));
    }
    fs::canonicalize(path).map_err(|_| PortError::Failed("ui_workspace_unavailable".to_owned()))
}

fn ensure_directory(path: &Path) -> Result<(), PortError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(PortError::Failed("ui_instance_directory_alias".to_owned()))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)
                .map_err(|error| {
                    PortError::Failed(format!("ui_instance_directory_failed:{error}"))
                })?,
            Err(error) => {
                return Err(PortError::Failed(format!(
                    "ui_instance_directory_failed:{error}"
                )))
            }
        }
    }
    Ok(())
}

fn ensure_directory_readable(path: &Path) -> Result<(), PortError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| PortError::Unavailable("ui_instance_not_ready".to_owned()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PortError::Unavailable(
            "ui_instance_directory_invalid".to_owned(),
        ));
    }
    Ok(())
}

fn reject_alias(path: &Path) -> Result<(), PortError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(PortError::Failed("ui_instance_path_symlink".to_owned()))
        }
        Ok(_) => Err(PortError::Conflict("ui_instance_path_exists".to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PortError::Failed(format!(
            "ui_instance_path_stat_failed:{error}"
        ))),
    }
}

fn is_alias(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn next_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(1)
        .max(1)
}
