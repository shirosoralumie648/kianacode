//! Complete, checksummed JSONL transaction frames with bounded blocking I/O.
use crate::event_store_core::{validate_idempotency_key, AppendPlan};
use crate::journal_core::{append_result, capabilities, JournalState, TransitionPlan};
use async_trait::async_trait;
use kiana_domain::*;
use kiana_ports::{EventAppendResult, EventStorePort, PortError};
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::Semaphore;

const MAX_STORAGE_WORKERS: usize = 16;
fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}
fn io_error(code: &str, e: std::io::Error) -> PortError {
    failed(format!("{code}:{e}"))
}
fn unknown(command_id: RequestId, reason: impl ToString) -> CommitOutcome {
    CommitOutcome::Unknown {
        command_id,
        reason: reason.to_string(),
    }
}

#[derive(Debug)]
struct JournalFiles {
    path: PathBuf,
    parent: PathBuf,
    #[cfg(unix)]
    directory: File,
}
impl JournalFiles {
    fn open(path: PathBuf) -> Result<Self, PortError> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        create_parent_directories(&parent)?;
        #[cfg(unix)]
        let directory = open_directory(&parent)?;
        Ok(Self {
            path,
            parent,
            #[cfg(unix)]
            directory,
        })
    }
    fn verify_parent(&self) -> Result<(), PortError> {
        #[cfg(unix)]
        {
            let current = fs::symlink_metadata(&self.parent)
                .map_err(|e| io_error("eventlog_parent_changed", e))?;
            let pinned = self
                .directory
                .metadata()
                .map_err(|e| io_error("eventlog_parent_metadata_failed", e))?;
            if !current.is_dir() || current.dev() != pinned.dev() || current.ino() != pinned.ino() {
                return Err(failed("eventlog_parent_changed"));
            }
        }
        Ok(())
    }
    fn open_name(&self, path: &Path, create: bool) -> Result<Option<File>, PortError> {
        self.verify_parent()?;
        #[cfg(unix)]
        {
            let name = path
                .file_name()
                .ok_or_else(|| failed("eventlog_path_required"))?;
            let name = std::ffi::CString::new(name.as_bytes())
                .map_err(|_| failed("eventlog_path_invalid"))?;
            let flags = libc::O_RDWR
                | libc::O_APPEND
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | if create { libc::O_CREAT } else { 0 };
            let fd =
                unsafe { libc::openat(self.directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
            if fd < 0 {
                let e = std::io::Error::last_os_error();
                if !create && e.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                if e.raw_os_error() == Some(libc::ELOOP) {
                    return Err(failed("eventlog_symlink_rejected"));
                }
                return Err(io_error("eventlog_open_failed", e));
            }
            let file = unsafe { File::from_raw_fd(fd) };
            let metadata = file
                .metadata()
                .map_err(|e| io_error("eventlog_metadata_failed", e))?;
            validate_unix_storage_file(&metadata)?;
            Ok(Some(file))
        }
        #[cfg(not(unix))]
        {
            // OpenOptions has no portable no-follow flag. Check the directory entry both before
            // and after opening, and expose the remaining race through the capability contract.
            reject_non_unix_symlink(path)?;
            let mut options = OpenOptions::new();
            options.read(true).write(true).append(true).create(create);
            match options.open(path) {
                Ok(file) => {
                    reject_non_unix_symlink(path)?;
                    let metadata = file
                        .metadata()
                        .map_err(|e| io_error("eventlog_metadata_failed", e))?;
                    if !metadata.is_file() {
                        return Err(failed("eventlog_not_regular_file"));
                    }
                    Ok(Some(file))
                }
                Err(e) if !create && e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(io_error("eventlog_open_failed", e)),
            }
        }
    }
    fn lock(&self) -> Result<ProcessEventLogLock, PortError> {
        let path = self.path.with_extension("jsonl.lock");
        let file = self
            .open_name(&path, true)?
            .ok_or_else(|| failed("eventlog_lock_missing"))?;
        #[cfg(unix)]
        {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(io_error(
                    "eventlog_lock_acquire_failed",
                    std::io::Error::last_os_error(),
                ));
            }
        }
        self.verify_named_file(&path, &file)?;
        Ok(ProcessEventLogLock(file))
    }
    fn verify_named_file(&self, path: &Path, file: &File) -> Result<(), PortError> {
        self.verify_parent()?;
        #[cfg(unix)]
        {
            let current = self
                .open_name(path, false)?
                .ok_or_else(|| failed("eventlog_file_removed"))?;
            let current = current
                .metadata()
                .map_err(|e| io_error("eventlog_metadata_failed", e))?;
            let pinned = file
                .metadata()
                .map_err(|e| io_error("eventlog_metadata_failed", e))?;
            if current.dev() != pinned.dev() || current.ino() != pinned.ino() {
                return Err(failed("eventlog_file_replaced"));
            }
        }
        #[cfg(not(unix))]
        {
            reject_non_unix_symlink(path)?;
            if !file
                .metadata()
                .map_err(|e| io_error("eventlog_metadata_failed", e))?
                .is_file()
            {
                return Err(failed("eventlog_not_regular_file"));
            }
        }
        Ok(())
    }
    fn sync_directory(&self) -> Result<(), PortError> {
        self.verify_parent()?;
        #[cfg(unix)]
        {
            self.directory
                .sync_all()
                .map_err(|e| io_error("eventlog_directory_sync_failed", e))?;
        }
        self.verify_parent()
    }
}
struct ProcessEventLogLock(File);
impl Drop for ProcessEventLogLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

#[derive(Debug, Default)]
struct DiskCache {
    journal: JournalState,
    offset: u64,
    line: u64,
    writer_version: u32,
    identity: Option<(u64, u64)>,
    modified: Option<SystemTime>,
    uncertain_write: bool,
}
#[derive(Debug)]
struct Inner {
    files: JournalFiles,
    cache: Mutex<DiskCache>,
    lifecycle: AtomicU8,
}
#[derive(Debug)]
pub struct JsonlEventLog {
    path: PathBuf,
    inner: Arc<Inner>,
    workers: Arc<Semaphore>,
}
impl JsonlEventLog {
    pub fn open_default() -> Result<Self, PortError> {
        Self::open(crate::default_sessions_log_path()?)
    }
    /// Synchronous startup compatibility API. Async applications should use open_async.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PortError> {
        let path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| io_error("eventlog_current_directory_failed", e))?
                .join(path)
        };
        let files = JournalFiles::open(path.clone())?;
        let _lock = files.lock()?;
        let mut cache = DiskCache::default();
        if let Some(mut file) = files.open_name(&path, false)? {
            load_delta(&files, &mut file, &mut cache)?;
        }
        Ok(Self {
            path,
            inner: Arc::new(Inner {
                files,
                cache: Mutex::new(cache),
                lifecycle: AtomicU8::new(0),
            }),
            workers: Arc::new(Semaphore::new(MAX_STORAGE_WORKERS)),
        })
    }
    pub async fn open_async(path: impl AsRef<Path>) -> Result<Self, PortError> {
        let path = path.as_ref().to_path_buf();
        tokio::task::spawn_blocking(move || Self::open(path))
            .await
            .map_err(|_| failed("eventlog_open_worker_failed"))?
    }
    pub async fn open_default_async() -> Result<Self, PortError> {
        Self::open_async(crate::default_sessions_log_path()?).await
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Report the platform-specific guarantees that protect journal and lock files. Non-Unix
    /// adapters retain an explicit symlink check but cannot prove a race-free no-follow open or
    /// portable hardlink identity, so those limitations remain visible to callers.
    pub fn storage_security_capabilities(&self) -> StorageSecurityCapabilities {
        StorageSecurityCapabilities::current()
    }
    async fn with_store<T: Send + 'static>(
        &self,
        f: impl FnOnce(&JournalFiles, &mut DiskCache) -> Result<T, PortError> + Send + 'static,
    ) -> Result<T, PortError> {
        self.with_store_state(f, false).await
    }
    async fn with_store_state<T: Send + 'static>(
        &self,
        f: impl FnOnce(&JournalFiles, &mut DiskCache) -> Result<T, PortError> + Send + 'static,
        allow_closing: bool,
    ) -> Result<T, PortError> {
        let lifecycle = self.inner.lifecycle.load(Ordering::Acquire);
        if lifecycle != 0 && !(allow_closing && lifecycle == 1) {
            return Err(failed("eventlog_closed"));
        }
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| PortError::Unavailable("eventlog_worker_queue_full".into()))?;
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let lifecycle = inner.lifecycle.load(Ordering::Acquire);
            if lifecycle != 0 && !(allow_closing && lifecycle == 1) {
                return Err(failed("eventlog_closed"));
            }
            let _lock = inner.files.lock()?;
            let mut cache = inner
                .cache
                .lock()
                .map_err(|_| failed("eventlog_cache_poisoned"))?;
            if let Some(mut file) = inner.files.open_name(&inner.files.path, false)? {
                load_delta(&inner.files, &mut file, &mut cache)?;
            } else if cache.identity.is_some() {
                return Err(failed("eventlog_file_removed"));
            }
            f(&inner.files, &mut cache)
        })
        .await
        .map_err(|_| PortError::Unavailable("eventlog_worker_failed".into()))?
    }

    async fn flush_health(&self) -> Result<EventStoreHealth, PortError> {
        self.with_store(|files, cache| {
            flush_store(files, cache)?;
            Ok(EventStoreHealth::new(
                cache.writer_version,
                true,
                true,
                cache.journal.events.len() as EventCursor,
                false,
            ))
        })
        .await
    }
}
#[async_trait]
impl EventStorePort for JsonlEventLog {
    fn supports_atomic_transitions(&self) -> bool {
        cfg!(unix)
    }
    fn capabilities(&self) -> EventStoreCapabilities {
        let mut result = capabilities(cfg!(unix));
        result.atomic_transitions = cfg!(unix);
        result
    }
    async fn flush(&self) -> Result<EventStoreHealth, PortError> {
        self.flush_health().await
    }
    async fn health(&self) -> Result<EventStoreHealth, PortError> {
        self.with_store(|_, cache| {
            Ok(EventStoreHealth::new(
                cache.writer_version,
                true,
                true,
                cache.journal.events.len() as EventCursor,
                false,
            ))
        })
        .await
    }
    async fn last_durable_cursor(&self) -> Result<EventCursor, PortError> {
        Ok(self.flush_health().await?.last_durable_cursor)
    }
    async fn close(&self) -> Result<EventStoreHealth, PortError> {
        self.inner
            .lifecycle
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|state| {
                if state == 1 {
                    failed("eventlog_close_in_progress")
                } else {
                    failed("eventlog_closed")
                }
            })?;
        let result = self
            .with_store_state(
                |files, cache| {
                    flush_store(files, cache)?;
                    Ok(EventStoreHealth::new(
                        cache.writer_version,
                        true,
                        true,
                        cache.journal.events.len() as EventCursor,
                        true,
                    ))
                },
                true,
            )
            .await;
        self.inner.lifecycle.store(2, Ordering::Release);
        result
    }
    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        if !self.supports_atomic_transitions() {
            return Err(PortError::Unavailable(
                "event_store_atomic_transitions_unsupported".into(),
            ));
        }
        batch.validate_identity().map_err(failed)?;
        let command_id = batch.command_id;
        let result = self
            .with_store(move |files, cache| {
                match cache.journal.plan_transition(batch, EventId::new())? {
                    TransitionPlan::Conflict(changed) => Ok(CommitOutcome::Conflict { changed }),
                    TransitionPlan::Replay(original) => match confirm_sync(files, cache) {
                        Ok(()) => Ok(CommitOutcome::Replayed { original }),
                        Err(e) => Ok(unknown(command_id, e)),
                    },
                    TransitionPlan::Append { batch, receipt } => {
                        let frame = JournalFrame::new(JournalFramePayload::Transition {
                            batch: batch.clone(),
                            receipt: receipt.clone(),
                        })
                        .map_err(failed)?;
                        let encoded = encode_line(&frame)?;
                        check_disk_capacity(cache, encoded.len())?;
                        if let Err(e) = write_frame(files, cache, &encoded, true) {
                            cache.uncertain_write = true;
                            return Ok(unknown(command_id, e));
                        }
                        cache.journal.apply_transition(batch, receipt.clone());
                        Ok(CommitOutcome::Committed { receipt })
                    }
                }
            })
            .await;
        match result {
            Err(PortError::Unavailable(reason)) if reason == "eventlog_worker_failed" => {
                Ok(unknown(command_id, reason))
            }
            other => other,
        }
    }
    async fn read_command(&self, id: &RequestId) -> Result<Option<CommandReceipt>, PortError> {
        let id = *id;
        self.with_store(move |files, cache| {
            let receipt = cache.journal.commands.get(&id).cloned();
            if receipt.is_some() {
                confirm_sync(files, cache)?;
            }
            Ok(receipt)
        })
        .await
    }
    async fn read_from(&self, cursor: u64, limit: usize) -> Result<JournalPage, PortError> {
        self.with_store(move |_, cache| cache.journal.page(cursor, limit))
            .await
    }
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.append_expected(event, None).await
    }
    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<(), PortError> {
        self.with_store(move |files, cache| {
            let AppendPlan::Append(event) = cache.journal.plan_legacy(event, expected, false)?
            else {
                unreachable!("non-idempotent append");
            };
            let (encoded, upgrade) = encode_legacy_record(cache, &event)?;
            check_disk_capacity(cache, encoded.len())?;
            if let Err(e) = write_frame(files, cache, &encoded, upgrade) {
                cache.uncertain_write = true;
                return Err(PortError::Unavailable(format!(
                    "eventlog_append_result_unknown:{e}"
                )));
            }
            cache.journal.apply_legacy(event);
            Ok(())
        })
        .await
    }
    async fn append_idempotent(&self, event: RuntimeEvent) -> Result<EventAppendResult, PortError> {
        self.append_idempotent_expected(event, None).await
    }
    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        validate_idempotency_key(&event)?;
        self.with_store(move |files, cache| {
            match cache.journal.plan_legacy(event, expected, true)? {
                AppendPlan::Replay(event) => {
                    confirm_sync(files, cache)?;
                    Ok(append_result(event, true))
                }
                AppendPlan::Append(event) => {
                    let (encoded, upgrade) = encode_legacy_record(cache, &event)?;
                    check_disk_capacity(cache, encoded.len())?;
                    if let Err(e) = write_frame(files, cache, &encoded, upgrade) {
                        cache.uncertain_write = true;
                        return Err(PortError::Unavailable(format!(
                            "eventlog_append_result_unknown:{e}"
                        )));
                    }
                    cache.journal.apply_legacy(event.clone());
                    Ok(append_result(event, false))
                }
            }
        })
        .await
    }
    async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        let id = *id;
        self.with_store(move |_, cache| Ok(cache.journal.request(&id)))
            .await
    }
    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        self.with_store(|_, cache| Ok(cache.journal.events.clone()))
            .await
    }
    async fn read_stream(&self, kind: &str, id: &str) -> Result<Vec<RuntimeEvent>, PortError> {
        let (kind, id) = (kind.to_owned(), id.to_owned());
        self.with_store(move |_, cache| Ok(cache.journal.stream(&kind, &id)))
            .await
    }
}
fn encode_line(value: &impl serde_compat::JournalEncode) -> Result<Vec<u8>, PortError> {
    let mut bytes = value.journal_bytes().map_err(failed)?;
    if bytes.len() > MAX_JOURNAL_FRAME_BYTES {
        return Err(failed("eventlog_frame_size_limit"));
    }
    bytes.push(b'\n');
    Ok(bytes)
}
// The adapter deliberately needs no additional serde dependency: its wire DTOs own serialization.
mod serde_compat {
    pub(super) trait JournalEncode {
        fn journal_bytes(&self) -> Result<Vec<u8>, String>;
    }
    impl JournalEncode for kiana_domain::JournalFrame {
        fn journal_bytes(&self) -> Result<Vec<u8>, String> {
            kiana_domain::canonical_journal_bytes(self)
        }
    }
    impl JournalEncode for kiana_domain::JournalHeader {
        fn journal_bytes(&self) -> Result<Vec<u8>, String> {
            kiana_domain::canonical_journal_bytes(self)
        }
    }
}
fn check_disk_capacity(cache: &DiskCache, next: usize) -> Result<(), PortError> {
    let header = if cache.writer_version < JOURNAL_WRITER_VERSION {
        256
    } else {
        0
    };
    if cache
        .offset
        .saturating_add(next as u64)
        .saturating_add(header)
        > MAX_JOURNAL_LOG_BYTES
    {
        return Err(failed("eventlog_disk_limit"));
    }
    Ok(())
}
fn encode_legacy_record(
    cache: &DiskCache,
    event: &RuntimeEvent,
) -> Result<(Vec<u8>, bool), PortError> {
    if cache.writer_version == 0 {
        // Compatibility-only logs retain their existing single-event wire format until
        // the first atomic command durably installs the required writer header.
        let mut bytes =
            serde_json::to_vec(event).map_err(|e| failed(format!("eventlog_encode_failed:{e}")))?;
        if bytes.len() > MAX_JOURNAL_EVENT_BYTES {
            return Err(failed("eventlog_event_size_limit"));
        }
        bytes.push(b'\n');
        return Ok((bytes, false));
    }
    let frame = JournalFrame::new(JournalFramePayload::Event {
        event: event.clone(),
    })
    .map_err(failed)?;
    Ok((encode_line(&frame)?, true))
}
fn write_frame(
    files: &JournalFiles,
    cache: &mut DiskCache,
    bytes: &[u8],
    upgrade: bool,
) -> Result<(), PortError> {
    let mut file = files
        .open_name(&files.path, true)?
        .ok_or_else(|| failed("eventlog_file_missing"))?;
    verify_cached_file(&file, cache)?;
    if upgrade && cache.writer_version < JOURNAL_WRITER_VERSION {
        let header = encode_line(&JournalHeader::default())?;
        file.write_all(&header)
            .map_err(|e| io_error("eventlog_header_write_failed", e))?;
        file.sync_all()
            .map_err(|e| io_error("eventlog_header_sync_failed", e))?;
        files.sync_directory()?;
        cache.offset += header.len() as u64;
        cache.line += 1;
        cache.writer_version = JOURNAL_WRITER_VERSION;
    }
    file.write_all(bytes)
        .map_err(|e| io_error("eventlog_write_failed", e))?;
    file.flush()
        .map_err(|e| io_error("eventlog_flush_failed", e))?;
    file.sync_all()
        .map_err(|e| io_error("eventlog_sync_failed", e))?;
    files.sync_directory()?;
    files.verify_named_file(&files.path, &file)?;
    // No fallible operation may follow cursor publication: otherwise an Unknown response
    // could skip the durable frame during the next read and lose its command receipt.
    remember_file(&file, cache)?;
    cache.offset += bytes.len() as u64;
    cache.line += 1;
    cache.uncertain_write = false;
    Ok(())
}
fn verify_cached_file(file: &File, cache: &DiskCache) -> Result<(), PortError> {
    let metadata = file
        .metadata()
        .map_err(|e| io_error("eventlog_metadata_failed", e))?;
    if metadata.len() != cache.offset {
        return Err(failed("eventlog_file_changed_during_commit"));
    }
    #[cfg(unix)]
    if cache
        .identity
        .is_some_and(|id| id != (metadata.dev(), metadata.ino()))
    {
        return Err(failed("eventlog_file_replaced"));
    }
    Ok(())
}
fn confirm_sync(files: &JournalFiles, cache: &DiskCache) -> Result<(), PortError> {
    let file = files
        .open_name(&files.path, false)?
        .ok_or_else(|| failed("eventlog_file_removed"))?;
    verify_cached_file(&file, cache)?;
    file.sync_all()
        .map_err(|e| io_error("eventlog_confirm_sync_failed", e))?;
    files.sync_directory()?;
    files.verify_named_file(&files.path, &file)
}
fn flush_store(files: &JournalFiles, cache: &DiskCache) -> Result<(), PortError> {
    if cache.identity.is_some() {
        confirm_sync(files, cache)
    } else {
        files.sync_directory()
    }
}
fn remember_file(file: &File, cache: &mut DiskCache) -> Result<(), PortError> {
    let metadata = file
        .metadata()
        .map_err(|e| io_error("eventlog_metadata_failed", e))?;
    #[cfg(unix)]
    {
        cache.identity = Some((metadata.dev(), metadata.ino()));
    }
    #[cfg(not(unix))]
    {
        cache.identity = Some((0, 0));
    }
    cache.modified = metadata.modified().ok();
    Ok(())
}
fn load_delta(
    files: &JournalFiles,
    file: &mut File,
    cache: &mut DiskCache,
) -> Result<(), PortError> {
    let original_offset = cache.offset;
    let metadata = file
        .metadata()
        .map_err(|e| io_error("eventlog_metadata_failed", e))?;
    if metadata.len() > MAX_JOURNAL_LOG_BYTES {
        return Err(failed("eventlog_disk_limit"));
    }
    #[cfg(unix)]
    if cache
        .identity
        .is_some_and(|id| id != (metadata.dev(), metadata.ino()))
    {
        return Err(failed("eventlog_file_replaced"));
    }
    if metadata.len() < cache.offset {
        return Err(failed("eventlog_committed_prefix_truncated"));
    }
    if metadata.len() == cache.offset
        && cache.offset > 0
        && cache.modified.is_some()
        && cache.modified != metadata.modified().ok()
        && !cache.uncertain_write
    {
        // A peer may have removed its own incomplete tail without changing this
        // committed prefix. Revalidate once instead of accepting mtime as authority.
        let mut verified = DiskCache::default();
        load_delta(files, file, &mut verified)?;
        if verified.offset != cache.offset
            || verified.writer_version != cache.writer_version
            || verified.journal.events != cache.journal.events
            || verified.journal.commands != cache.journal.commands
        {
            return Err(failed("eventlog_committed_prefix_modified"));
        }
        *cache = verified;
        return Ok(());
    }
    file.seek(SeekFrom::Start(cache.offset))
        .map_err(|e| io_error("eventlog_seek_failed", e))?;
    let mut reader = BufReader::new(
        file.try_clone()
            .map_err(|e| io_error("eventlog_clone_failed", e))?,
    );
    loop {
        let bytes = read_bounded_line(&mut reader)?;
        if bytes.is_empty() {
            break;
        }
        let newline = bytes.last() == Some(&b'\n');
        let payload = if newline {
            &bytes[..bytes.len() - 1]
        } else {
            &bytes[..]
        };
        let line_number = cache.line + 1;
        if payload.iter().all(u8::is_ascii_whitespace) {
            cache.offset += bytes.len() as u64;
            cache.line += 1;
            continue;
        }
        let value = match serde_json::from_slice::<serde_json::Value>(payload) {
            Ok(value) => value,
            Err(e)
                if !newline
                    && e.is_eof()
                    && (cache.writer_version == JOURNAL_WRITER_VERSION
                        || !cache.journal.events.is_empty()) =>
            {
                file.set_len(cache.offset)
                    .map_err(|e| io_error("eventlog_repair_failed", e))?;
                file.sync_all()
                    .map_err(|e| io_error("eventlog_repair_sync_failed", e))?;
                files.sync_directory()?;
                break;
            }
            Err(_) => return Err(failed(format!("eventlog_corrupt:line={line_number}"))),
        };
        let schema = value.get("schema").and_then(serde_json::Value::as_str);
        if schema == Some(JOURNAL_HEADER_SCHEMA) {
            let header: JournalHeader =
                serde_json::from_value(value).map_err(|_| failed("eventlog_header_invalid"))?;
            header.validate().map_err(failed)?;
            if cache.writer_version == JOURNAL_WRITER_VERSION {
                return Err(failed("eventlog_writer_version_unsupported"));
            }
            cache.writer_version = JOURNAL_WRITER_VERSION;
        } else if schema == Some(JOURNAL_FRAME_SCHEMA) {
            if cache.writer_version != JOURNAL_WRITER_VERSION {
                return Err(failed("eventlog_frame_header_missing"));
            }
            let source_shape = canonical_journal_bytes(&value).map_err(failed)?;
            let frame: JournalFrame = serde_json::from_value(value)
                .map_err(|_| failed(format!("eventlog_frame_invalid:line={line_number}")))?;
            // RuntimeEvent remains permissive for v1 callers, but a v2 authority frame
            // must not silently discard newly required fields nested in an event.
            if canonical_journal_bytes(&frame).map_err(failed)? != source_shape {
                return Err(failed("eventlog_frame_shape_unsupported"));
            }
            cache
                .journal
                .accept_frame(frame)
                .map_err(|e| failed(format!("eventlog_corrupt:line={line_number}:{e}")))?;
        } else if value.get("schema").is_some()
            || value.get("required").is_some()
            || value.get("writer_version").is_some()
        {
            return Err(failed("eventlog_required_record_unsupported"));
        } else {
            if cache.writer_version == JOURNAL_WRITER_VERSION {
                return Err(failed("eventlog_legacy_writer_after_upgrade"));
            }
            let event: RuntimeEvent = serde_json::from_value(value)
                .map_err(|_| failed(format!("eventlog_corrupt:line={line_number}")))?;
            let AppendPlan::Append(event) = cache.journal.plan_legacy(event, None, false)? else {
                unreachable!("legacy replay is not idempotent");
            };
            cache.journal.apply_legacy(event);
        }
        cache.offset += bytes.len() as u64;
        cache.line += 1;
        if !newline {
            file.write_all(b"\n")
                .map_err(|e| io_error("eventlog_repair_newline_write_failed", e))?;
            file.sync_all()
                .map_err(|e| io_error("eventlog_repair_newline_sync_failed", e))?;
            files.sync_directory()?;
            cache.offset += 1;
            break;
        }
    }
    if cache.offset != original_offset || cache.uncertain_write {
        // A previous process may have lost its sync/commit response. Establish the same
        // persistence barrier before any reader exposes its complete authority frames.
        cache.uncertain_write = true;
        file.sync_all()
            .map_err(|e| io_error("eventlog_recovery_sync_failed", e))?;
        files.sync_directory()?;
    }
    files.verify_named_file(&files.path, file)?;
    remember_file(file, cache)?;
    cache.uncertain_write = false;
    Ok(())
}
fn read_bounded_line(reader: &mut impl BufRead) -> Result<Vec<u8>, PortError> {
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|e| io_error("eventlog_read_failed", e))?;
        if available.is_empty() {
            break;
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        if line.len().saturating_add(take) > MAX_JOURNAL_FRAME_BYTES + 1 {
            return Err(failed("eventlog_frame_size_limit"));
        }
        let ended = available[take - 1] == b'\n';
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if ended {
            break;
        }
    }
    Ok(line)
}
fn create_parent_directories(parent: &Path) -> Result<(), PortError> {
    let mut missing = Vec::new();
    let mut current = parent;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(failed("eventlog_parent_not_directory"));
                }
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                current = current
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
            }
            Err(e) => return Err(io_error("eventlog_parent_metadata_failed", e)),
        }
    }
    for directory in missing.into_iter().rev() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        match builder.create(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_error("eventlog_create_failed", e)),
        }
        #[cfg(unix)]
        {
            open_directory(&directory)?
                .sync_all()
                .map_err(|e| io_error("eventlog_directory_sync_failed", e))?;
            if let Some(parent) = directory.parent() {
                open_directory(parent)?
                    .sync_all()
                    .map_err(|e| io_error("eventlog_directory_sync_failed", e))?;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_unix_storage_file(metadata: &std::fs::Metadata) -> Result<(), PortError> {
    if !metadata.is_file() {
        return Err(failed("eventlog_not_regular_file"));
    }
    if metadata.nlink() > 1 {
        return Err(failed("eventlog_hardlink_rejected"));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(failed("eventlog_permissions_too_broad"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_non_unix_symlink(path: &Path) -> Result<(), PortError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(failed("eventlog_symlink_rejected"))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error("eventlog_metadata_failed", e)),
    }
}

#[cfg(unix)]
fn open_directory(path: &Path) -> Result<File, PortError> {
    let name = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| failed("eventlog_parent_invalid"))?;
    let fd = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io_error(
            "eventlog_open_failed",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

// Preserve the existing explicit-directory compatibility helper and its fixture.
#[cfg(all(test, target_os = "linux"))]
fn open_eventlog_directory(path: &Path) -> Result<File, PortError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_name = std::ffi::CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| PortError::Failed("eventlog_parent_invalid".to_owned()))?;
    let fd = unsafe {
        libc::open(
            parent_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(PortError::Failed(format!(
            "eventlog_open_failed:{}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(all(test, target_os = "linux"))]
fn append_jsonl_line_at(
    path: &Path,
    event: &RuntimeEvent,
    directory: &File,
) -> Result<(), PortError> {
    let target_name = path
        .file_name()
        .ok_or_else(|| PortError::Failed("eventlog_path_required".to_owned()))?;
    let target_name = std::ffi::CString::new(target_name.as_bytes())
        .map_err(|_| PortError::Failed("eventlog_path_invalid".to_owned()))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            target_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(PortError::Failed(format!(
            "eventlog_open_failed:{}",
            std::io::Error::last_os_error()
        )));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut encoded = serde_json::to_string(event)
        .map_err(|error| PortError::Failed(format!("eventlog_encode_failed:{error}")))?;
    encoded.push('\n');
    file.write_all(encoded.as_bytes())
        .map_err(|error| PortError::Failed(format!("eventlog_write_failed:{error}")))?;
    file.flush()
        .map_err(|error| PortError::Failed(format!("eventlog_flush_failed:{error}")))?;
    file.sync_data()
        .map_err(|error| PortError::Failed(format!("eventlog_sync_failed:{error}")))?;
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn append_keeps_the_opened_parent_after_path_replacement() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-eventlog-parent-{}-{stamp}",
            std::process::id()
        ));
        let parent = root.join("sessions");
        let moved_parent = root.join("sessions-original");
        let path = parent.join("events.jsonl");
        fs::create_dir_all(&parent).unwrap();
        let directory = open_eventlog_directory(&path).unwrap();

        fs::rename(&parent, &moved_parent).unwrap();
        fs::create_dir(&parent).unwrap();
        let event = RuntimeEvent::new(RequestId::new(), 1, "run.accepted", Value::Null).unwrap();
        append_jsonl_line_at(&path, &event, &directory).unwrap();

        assert_eq!(
            fs::read_to_string(moved_parent.join("events.jsonl")).unwrap(),
            format!("{}\n", serde_json::to_string(&event).unwrap())
        );
        assert!(
            !path.exists(),
            "replacement parent must not receive the append"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
