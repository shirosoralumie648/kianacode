//! INT-24 durable replay window for signed Webhook/A2A ingress.
//!
//! The verifier owns signatures and policy. This adapter owns exactly one thing: a bounded,
//! crash-consistent replay window keyed by `source_id + event_id` and by `source_id + nonce`.
//!
//! ```text
//! ConnectorIngressVerifier::verify()  (signature / policy / clock)
//!     -> ConnectorIngressReplayEntry (digest-only identity)
//!     -> ConnectorIngressReplayStore::admit()
//!     -> Accepted | Duplicate | EventConflict | NonceReplay
//! ```
//!
//! # Boundaries
//!
//! - This module never appends an EventLog fact and never invokes a connector. Accepting a replay
//!   entry is not an authorization; the derived intent still has to enter `ControlPlane`.
//! - Entries are digest-only. Raw payloads, signatures and signing keys never reach the file.
//! - The file is rewritten atomically (temp sibling + rename + fsync) while an OS-level exclusive
//!   lock is held, so two processes cannot each admit the same occurrence.
//! - The window is bounded twice: by time (`expires_at_unix_ms`) and by entry count. A full store
//!   first prunes expired entries; if it is still full it fails closed with
//!   `connector_ingress_replay_capacity_exceeded` instead of evicting live evidence.

use async_trait::async_trait;
use kiana_domain::{
    ConnectorIngressReplayEntry, ConnectorIngressReplayOutcome,
    CONNECTOR_INGRESS_REPLAY_MAX_ENTRIES, CONNECTOR_INGRESS_REPLAY_MAX_WINDOW_MS,
};
use kiana_ports::{ConnectorIngressReplayStore, PortError};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

const REPLAY_FILE_SCHEMA: &str = "kiana.connector-ingress-replay-store.v1";
/// A store larger than this is treated as corrupt/foreign instead of being loaded.
const MAX_REPLAY_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// File-backed replay store shared by every clone and by every process using the same path.
pub struct JsonlConnectorIngressReplayStore {
    path: Arc<PathBuf>,
    window_ms: u64,
    max_entries: usize,
    /// Serializes in-process readers/appenders; the OS lock covers other processes.
    gate: Mutex<()>,
}

impl std::fmt::Debug for JsonlConnectorIngressReplayStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JsonlConnectorIngressReplayStore")
            .field("path", &"[REPLAY_STORE_PATH]")
            .field("window_ms", &self.window_ms)
            .field("max_entries", &self.max_entries)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayFile {
    schema: String,
    entries: Vec<ConnectorIngressReplayEntry>,
}

/// Snapshot of the store configuration usable from a blocking task.
struct ReplayStoreHandle {
    path: Arc<PathBuf>,
    max_entries: usize,
}

impl JsonlConnectorIngressReplayStore {
    /// Open (or create) the replay store at `path` with the default bounded window.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, PortError> {
        Self::open_with_window(
            path,
            kiana_domain::CONNECTOR_INGRESS_REPLAY_DEFAULT_WINDOW_MS,
        )
    }

    /// Open with an explicit retention window. `window_ms` is clamped by the domain contract.
    pub fn open_with_window(path: impl Into<PathBuf>, window_ms: u64) -> Result<Self, PortError> {
        if window_ms == 0 || window_ms > CONNECTOR_INGRESS_REPLAY_MAX_WINDOW_MS {
            return Err(PortError::Failed(
                "connector_ingress_replay_window_invalid".to_owned(),
            ));
        }
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(replay_io)?;
        }
        let store = Self {
            path: Arc::new(path),
            window_ms,
            max_entries: CONNECTOR_INGRESS_REPLAY_MAX_ENTRIES,
            gate: Mutex::new(()),
        };
        // Fail at construction on an unreadable/corrupt store rather than at first delivery.
        store.read_file()?;
        Ok(store)
    }

    /// Retention window applied to newly admitted entries.
    pub fn window_ms(&self) -> u64 {
        self.window_ms
    }

    /// Path of the replay store file (audit reference, not a secret).
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    fn handle(&self) -> ReplayStoreHandle {
        ReplayStoreHandle {
            path: self.path.clone(),
            max_entries: self.max_entries,
        }
    }

    fn read_file(&self) -> Result<Vec<ConnectorIngressReplayEntry>, PortError> {
        read_entries(self.path.as_path(), self.max_entries)
    }
}

impl ReplayStoreHandle {
    fn read(&self) -> Result<Vec<ConnectorIngressReplayEntry>, PortError> {
        read_entries(self.path.as_path(), self.max_entries)
    }

    fn write(&self, entries: &[ConnectorIngressReplayEntry]) -> Result<(), PortError> {
        write_entries(self.path.as_path(), entries)
    }

    fn admit_loaded(
        entries: &mut Vec<ConnectorIngressReplayEntry>,
        entry: ConnectorIngressReplayEntry,
        now_unix_ms: u64,
    ) -> Result<ConnectorIngressReplayOutcome, PortError> {
        entry.validate().map_err(PortError::Failed)?;
        entries.retain(|existing| !existing.is_expired_at(now_unix_ms));
        let mut nonce_replay = false;
        for existing in entries.iter() {
            if existing.source_id != entry.source_id {
                continue;
            }
            if existing.event_id == entry.event_id {
                return Ok(if existing.ingress_digest == entry.ingress_digest {
                    ConnectorIngressReplayOutcome::Duplicate
                } else {
                    ConnectorIngressReplayOutcome::EventConflict
                });
            }
            if existing.nonce == entry.nonce {
                nonce_replay = true;
            }
        }
        if nonce_replay {
            return Ok(ConnectorIngressReplayOutcome::NonceReplay);
        }
        if entries.len() >= CONNECTOR_INGRESS_REPLAY_MAX_ENTRIES {
            return Err(PortError::Failed(
                "connector_ingress_replay_capacity_exceeded".to_owned(),
            ));
        }
        entries.push(entry);
        Ok(ConnectorIngressReplayOutcome::Accepted)
    }
}

#[async_trait]
impl ConnectorIngressReplayStore for JsonlConnectorIngressReplayStore {
    async fn admit(
        &self,
        entry: ConnectorIngressReplayEntry,
        now_unix_ms: u64,
    ) -> Result<ConnectorIngressReplayOutcome, PortError> {
        if now_unix_ms == 0 {
            return Err(PortError::Failed(
                "connector_ingress_replay_clock_invalid".to_owned(),
            ));
        }
        let _gate = self.gate.lock().await;
        let store = self.handle();
        // The OS lock and file IO are blocking; keep them off the async runtime.
        tokio::task::spawn_blocking(move || {
            let _lock = ReplayFileLock::acquire(store.path.as_path())?;
            let mut entries = store.read()?;
            let outcome = ReplayStoreHandle::admit_loaded(&mut entries, entry, now_unix_ms)?;
            if outcome == ConnectorIngressReplayOutcome::Accepted {
                store.write(&entries)?;
            }
            Ok(outcome)
        })
        .await
        .map_err(|error| PortError::Failed(format!("connector_ingress_replay_join:{error}")))?
    }

    async fn window(
        &self,
        source_id: &str,
        now_unix_ms: u64,
        limit: usize,
    ) -> Result<Vec<ConnectorIngressReplayEntry>, PortError> {
        if source_id.trim().is_empty() || limit == 0 {
            return Err(PortError::Failed(
                "connector_ingress_replay_query_invalid".to_owned(),
            ));
        }
        let _gate = self.gate.lock().await;
        let store = self.handle();
        let source_id = source_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let entries = store.read()?;
            Ok(entries
                .into_iter()
                .filter(|entry| entry.source_id == source_id && !entry.is_expired_at(now_unix_ms))
                .take(limit)
                .collect())
        })
        .await
        .map_err(|error| PortError::Failed(format!("connector_ingress_replay_join:{error}")))?
    }
}

fn read_entries(
    path: &Path,
    max_entries: usize,
) -> Result<Vec<ConnectorIngressReplayEntry>, PortError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(replay_io(error)),
    };
    if !metadata.is_file() || metadata.len() > MAX_REPLAY_FILE_BYTES {
        return Err(PortError::Failed(
            "connector_ingress_replay_store_invalid".to_owned(),
        ));
    }
    let bytes = fs::read(path).map_err(replay_io)?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let file: ReplayFile = serde_json::from_slice(&bytes)
        .map_err(|_| PortError::Failed("connector_ingress_replay_store_invalid".to_owned()))?;
    if file.schema != REPLAY_FILE_SCHEMA || file.entries.len() > max_entries {
        return Err(PortError::Failed(
            "connector_ingress_replay_store_invalid".to_owned(),
        ));
    }
    for entry in &file.entries {
        entry.validate().map_err(PortError::Failed)?;
    }
    Ok(file.entries)
}

fn write_entries(path: &Path, entries: &[ConnectorIngressReplayEntry]) -> Result<(), PortError> {
    let file = ReplayFile {
        schema: REPLAY_FILE_SCHEMA.to_owned(),
        entries: entries.to_vec(),
    };
    let bytes = serde_json::to_vec(&file)
        .map_err(|_| PortError::Failed("connector_ingress_replay_store_invalid".to_owned()))?;
    if bytes.len() as u64 > MAX_REPLAY_FILE_BYTES {
        return Err(PortError::Failed(
            "connector_ingress_replay_store_invalid".to_owned(),
        ));
    }
    let temporary = path.with_extension("json.tmp");
    {
        let mut handle = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(replay_io)?;
        handle.write_all(&bytes).map_err(replay_io)?;
        handle.sync_all().map_err(replay_io)?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        replay_io(error)
    })?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

fn replay_io(error: std::io::Error) -> PortError {
    PortError::Failed(format!("connector_ingress_replay_io:{error}"))
}

/// Cross-process exclusive lock around the replay file.
///
/// The lock file lives next to the replay store; the handle stays open for the whole read-modify-
/// write so another process cannot interleave. The OS releases it on close or process death.
#[derive(Debug)]
struct ReplayFileLock {
    _file: File,
}

impl ReplayFileLock {
    fn acquire(path: &Path) -> Result<Self, PortError> {
        let lock_path = path.with_extension("lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|_| {
                PortError::Failed("connector_ingress_replay_lock_unavailable".to_owned())
            })?;
        lock_file(&file)?;
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
fn lock_file(file: &File) -> Result<(), PortError> {
    use std::os::unix::io::AsRawFd;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result == 0 {
        Ok(())
    } else {
        Err(PortError::Failed(
            "connector_ingress_replay_lock_unavailable".to_owned(),
        ))
    }
}

#[cfg(windows)]
fn lock_file(file: &File) -> Result<(), PortError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let handle = file.as_raw_handle() as HANDLE;
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let flags = LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY;
    if unsafe { LockFileEx(handle, flags, 0, u32::MAX, u32::MAX, &mut overlapped) } != 0 {
        return Ok(());
    }
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut retry: OVERLAPPED = unsafe { std::mem::zeroed() };
        if unsafe {
            LockFileEx(
                handle,
                LOCKFILE_EXCLUSIVE_LOCK,
                0,
                u32::MAX,
                u32::MAX,
                &mut retry,
            )
        } != 0
        {
            return Ok(());
        }
    }
    Err(PortError::Failed(
        "connector_ingress_replay_lock_unavailable".to_owned(),
    ))
}

#[cfg(not(any(unix, windows)))]
fn lock_file(_file: &File) -> Result<(), PortError> {
    Err(PortError::Failed(
        "connector_ingress_replay_lock_unavailable".to_owned(),
    ))
}
