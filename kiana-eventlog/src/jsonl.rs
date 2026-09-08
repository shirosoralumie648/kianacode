//! Durable JSONL event-store adapter.

use crate::event_store_core::{
    plan_append, plan_idempotent_append, reject_conflicts, validate_idempotency_key, AppendPlan,
};
use async_trait::async_trait;
use kiana_domain::{RequestId, RuntimeEvent};
use kiana_ports::{EventAppendResult, EventStorePort, PortError};
#[cfg(target_os = "linux")]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

#[cfg(unix)]
struct ProcessEventLogLock(std::fs::File);

#[cfg(unix)]
impl ProcessEventLogLock {
    fn acquire(path: &Path) -> Result<Self, PortError> {
        let lock_path = path.with_extension("jsonl.lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                PortError::Failed(format!("eventlog_lock_create_failed:{error}"))
            })?;
        }
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW);
        let file = options
            .open(lock_path)
            .map_err(|error| PortError::Failed(format!("eventlog_lock_open_failed:{error}")))?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result != 0 {
            return Err(PortError::Failed(format!(
                "eventlog_lock_acquire_failed:{}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self(file))
    }
}

#[cfg(unix)]
impl Drop for ProcessEventLogLock {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(not(unix))]
struct ProcessEventLogLock;

#[cfg(not(unix))]
impl ProcessEventLogLock {
    fn acquire(_path: &Path) -> Result<Self, PortError> {
        Ok(Self)
    }
}

#[derive(Debug)]
pub struct JsonlEventLog {
    path: PathBuf,
    events: RwLock<Vec<RuntimeEvent>>,
}

impl JsonlEventLog {
    pub fn open_default() -> Result<Self, PortError> {
        Self::open(crate::default_sessions_log_path()?)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, PortError> {
        let path = path.as_ref().to_path_buf();
        let _process_lock = ProcessEventLogLock::acquire(&path)?;
        let events = if path_is_present(&path)? {
            load_jsonl(&path)?
        } else {
            Vec::new()
        };
        Ok(Self {
            path,
            events: RwLock::new(events),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl EventStorePort for JsonlEventLog {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.append_expected(event, None).await
    }

    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        let _process_lock = ProcessEventLogLock::acquire(&self.path)?;
        let mut events = self.events.write().await;
        reload_if_present(&self.path, &mut events)?;
        let event = plan_append(&events, event, expected_version)?;
        append_jsonl_line(&self.path, &event)?;
        events.push(event);
        Ok(())
    }

    async fn append_idempotent(&self, event: RuntimeEvent) -> Result<EventAppendResult, PortError> {
        self.append_idempotent_expected(event, None).await
    }

    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        validate_idempotency_key(&event)?;
        let _process_lock = ProcessEventLogLock::acquire(&self.path)?;
        let mut events = self.events.write().await;
        reload_if_present(&self.path, &mut events)?;
        match plan_idempotent_append(&events, event, expected_version)? {
            AppendPlan::Replay(event) => Ok(EventAppendResult {
                event,
                replayed: true,
            }),
            AppendPlan::Append(event) => {
                append_jsonl_line(&self.path, &event)?;
                events.push(event.clone());
                Ok(EventAppendResult {
                    event,
                    replayed: false,
                })
            }
        }
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        let _process_lock = ProcessEventLogLock::acquire(&self.path)?;
        let mut events = self.events.write().await;
        reload_if_present(&self.path, &mut events)?;
        Ok(events
            .iter()
            .filter(|event| &event.request_id == request_id)
            .cloned()
            .collect())
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        let _process_lock = ProcessEventLogLock::acquire(&self.path)?;
        let mut events = self.events.write().await;
        reload_if_present(&self.path, &mut events)?;
        Ok(events.clone())
    }
}

fn reload_if_present(path: &Path, events: &mut Vec<RuntimeEvent>) -> Result<(), PortError> {
    if path_is_present(path)? {
        *events = load_jsonl(path)?;
    }
    Ok(())
}

fn path_is_present(path: &Path) -> Result<bool, PortError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PortError::Failed(format!("eventlog_open_failed:{error}"))),
    }
}

fn load_jsonl(path: &Path) -> Result<Vec<RuntimeEvent>, PortError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|error| PortError::Failed(format!("eventlog_open_failed:{error}")))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| PortError::Failed(format!("eventlog_open_failed:{error}")))?;
    let mut events = Vec::new();
    let mut offset = 0usize;
    for (line_index, segment) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let has_newline = segment.last() == Some(&b'\n');
        let line = if has_newline {
            &segment[..segment.len() - 1]
        } else {
            segment
        };
        let line_number = line_index + 1;
        offset += segment.len();
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<RuntimeEvent>(line) {
            Ok(event) => {
                reject_conflicts(&events, &event)?;
                events.push(event);
                if !has_newline && offset == bytes.len() {
                    repair_missing_final_newline(path)?;
                }
            }
            Err(error)
                if !has_newline
                    && offset == bytes.len()
                    && !events.is_empty()
                    && is_torn_tail_error(&error) =>
            {
                truncate_torn_tail(path, offset - segment.len())?;
                break;
            }
            Err(error) => {
                return Err(PortError::Failed(format!(
                    "eventlog_corrupt:line={line_number}:{error}"
                )))
            }
        }
    }
    Ok(events)
}

fn is_torn_tail_error(error: &serde_json::Error) -> bool {
    error.to_string().to_ascii_lowercase().contains("eof")
}

fn truncate_torn_tail(path: &Path, complete_bytes: usize) -> Result<(), PortError> {
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options
        .open(path)
        .map_err(|error| PortError::Failed(format!("eventlog_repair_open_failed:{error}")))?;
    file.set_len(complete_bytes as u64)
        .map_err(|error| PortError::Failed(format!("eventlog_repair_failed:{error}")))?;
    file.sync_data()
        .map_err(|error| PortError::Failed(format!("eventlog_repair_sync_failed:{error}")))
}

fn repair_missing_final_newline(path: &Path) -> Result<(), PortError> {
    let mut options = OpenOptions::new();
    options.append(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|error| {
        PortError::Failed(format!("eventlog_repair_newline_open_failed:{error}"))
    })?;
    file.write_all(b"\n").map_err(|error| {
        PortError::Failed(format!("eventlog_repair_newline_write_failed:{error}"))
    })?;
    file.flush().map_err(|error| {
        PortError::Failed(format!("eventlog_repair_newline_flush_failed:{error}"))
    })?;
    file.sync_data()
        .map_err(|error| PortError::Failed(format!("eventlog_repair_newline_sync_failed:{error}")))
}

fn append_jsonl_line(path: &Path, event: &RuntimeEvent) -> Result<(), PortError> {
    #[cfg(target_os = "linux")]
    {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| PortError::Failed(format!("eventlog_create_failed:{error}")))?;
        let directory = open_eventlog_directory(path)?;
        append_jsonl_line_at(path, event, &directory)
    }
    #[cfg(not(target_os = "linux"))]
    {
        append_jsonl_line_by_path(path, event)
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

#[cfg(not(target_os = "linux"))]
fn append_jsonl_line_by_path(path: &Path, event: &RuntimeEvent) -> Result<(), PortError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| PortError::Failed(format!("eventlog_create_failed:{error}")))?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|error| PortError::Failed(format!("eventlog_open_failed:{error}")))?;
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
