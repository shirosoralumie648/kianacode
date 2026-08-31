//! Append-only event storage adapters for Kiana.

use async_trait::async_trait;
use kiana_domain::{RequestId, RuntimeEvent};
use kiana_ports::{EventAppendResult, EventStorePort, PortError};
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct MemoryEventLog {
    events: RwLock<Vec<RuntimeEvent>>,
}

impl MemoryEventLog {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EventStorePort for MemoryEventLog {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.append_expected(event, None).await
    }

    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        let mut events = self.events.write().await;
        reject_expected_version(&events, &event, expected_version)?;
        reject_conflicts(&events, &event)?;
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
        let key = event_idempotency_key(&event)?;
        let mut events = self.events.write().await;
        if let Some(existing) = events
            .iter()
            .find(|existing| existing.idempotency_key.as_deref() == Some(key))
        {
            ensure_idempotent_match(existing, &event)?;
            return Ok(EventAppendResult {
                event: existing.clone(),
                replayed: true,
            });
        }
        reject_expected_version(&events, &event, expected_version)?;
        reject_conflicts(&events, &event)?;
        events.push(event.clone());
        Ok(EventAppendResult {
            event,
            replayed: false,
        })
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self
            .events
            .read()
            .await
            .iter()
            .filter(|event| &event.request_id == request_id)
            .cloned()
            .collect())
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.events.read().await.clone())
    }
}

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
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
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
        Self::open(default_sessions_log_path()?)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, PortError> {
        let path = path.as_ref().to_path_buf();
        let _process_lock = ProcessEventLogLock::acquire(&path)?;
        let events = if path.exists() {
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
        if self.path.exists() {
            *events = load_jsonl(&self.path)?;
        }
        reject_expected_version(&events, &event, expected_version)?;
        reject_conflicts(&events, &event)?;
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
        let key = event_idempotency_key(&event)?;
        let _process_lock = ProcessEventLogLock::acquire(&self.path)?;
        let mut events = self.events.write().await;
        if self.path.exists() {
            *events = load_jsonl(&self.path)?;
        }
        if let Some(existing) = events
            .iter()
            .find(|existing| existing.idempotency_key.as_deref() == Some(key))
        {
            ensure_idempotent_match(existing, &event)?;
            return Ok(EventAppendResult {
                event: existing.clone(),
                replayed: true,
            });
        }
        reject_expected_version(&events, &event, expected_version)?;
        reject_conflicts(&events, &event)?;
        append_jsonl_line(&self.path, &event)?;
        events.push(event.clone());
        Ok(EventAppendResult {
            event,
            replayed: false,
        })
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        let _process_lock = ProcessEventLogLock::acquire(&self.path)?;
        let mut events = self.events.write().await;
        if self.path.exists() {
            *events = load_jsonl(&self.path)?;
        }
        Ok(events
            .iter()
            .filter(|event| &event.request_id == request_id)
            .cloned()
            .collect())
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        let _process_lock = ProcessEventLogLock::acquire(&self.path)?;
        let mut events = self.events.write().await;
        if self.path.exists() {
            *events = load_jsonl(&self.path)?;
        }
        Ok(events.clone())
    }
}

pub fn default_sessions_log_path() -> Result<PathBuf, PortError> {
    let home = if let Ok(value) = std::env::var("KIANA_HOME") {
        let home = PathBuf::from(value.trim());
        if !home.is_absolute() {
            return Err(PortError::Failed("kiana_home_must_be_absolute".to_owned()));
        }
        home
    } else {
        let home = std::env::var("HOME")
            .map_err(|_| PortError::Failed("kiana_home_required".to_owned()))?;
        PathBuf::from(home).join(".kiana")
    };
    Ok(home.join("sessions").join("events.jsonl"))
}

fn reject_expected_version(
    events: &[RuntimeEvent],
    event: &RuntimeEvent,
    expected_version: Option<u64>,
) -> Result<(), PortError> {
    let Some(expected_version) = expected_version else {
        return Ok(());
    };
    let current_version = events
        .iter()
        .filter(|existing| same_stream(existing, event))
        .map(stream_version)
        .max()
        .unwrap_or(0);
    if current_version != expected_version {
        return Err(PortError::Conflict(
            "event_stream_version_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn reject_conflicts(events: &[RuntimeEvent], event: &RuntimeEvent) -> Result<(), PortError> {
    if events
        .iter()
        .any(|existing| existing.event_id == event.event_id)
    {
        return Err(PortError::Conflict("event_id_duplicate".to_owned()));
    }
    let version = stream_version(event);
    if events
        .iter()
        .any(|existing| same_stream(existing, event) && stream_version(existing) >= version)
    {
        return Err(PortError::Conflict(
            "event_sequence_not_monotonic".to_owned(),
        ));
    }
    Ok(())
}

fn same_stream(left: &RuntimeEvent, right: &RuntimeEvent) -> bool {
    let left_has_metadata = left.aggregate_type.is_some() || left.aggregate_id.is_some();
    let right_has_metadata = right.aggregate_type.is_some() || right.aggregate_id.is_some();
    if left_has_metadata || right_has_metadata {
        return matches!(
            (
                left.aggregate_type.as_deref(),
                left.aggregate_id.as_deref(),
                right.aggregate_type.as_deref(),
                right.aggregate_id.as_deref(),
            ),
            (Some(left_type), Some(left_id), Some(right_type), Some(right_id))
                if left_type == right_type && left_id == right_id
        );
    }
    left.request_id == right.request_id
}

fn stream_version(event: &RuntimeEvent) -> u64 {
    event.stream_version.unwrap_or(event.sequence)
}

fn event_idempotency_key(event: &RuntimeEvent) -> Result<&str, PortError> {
    event
        .idempotency_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| PortError::Failed("event_idempotency_key_required".to_owned()))
}

fn ensure_idempotent_match(
    existing: &RuntimeEvent,
    candidate: &RuntimeEvent,
) -> Result<(), PortError> {
    let same_payload = existing.request_id == candidate.request_id
        && existing.sequence == candidate.sequence
        && existing.kind == candidate.kind
        && existing.data == candidate.data
        && existing.aggregate_type == candidate.aggregate_type
        && existing.aggregate_id == candidate.aggregate_id
        && existing.stream_version == candidate.stream_version;
    if !same_payload {
        return Err(PortError::Conflict(
            "event_idempotency_key_payload_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn load_jsonl(path: &Path) -> Result<Vec<RuntimeEvent>, PortError> {
    let bytes = fs::read(path)
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
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| PortError::Failed(format!("eventlog_repair_open_failed:{error}")))?;
    file.set_len(complete_bytes as u64)
        .map_err(|error| PortError::Failed(format!("eventlog_repair_failed:{error}")))?;
    file.sync_data()
        .map_err(|error| PortError::Failed(format!("eventlog_repair_sync_failed:{error}")))
}

fn append_jsonl_line(path: &Path, event: &RuntimeEvent) -> Result<(), PortError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| PortError::Failed(format!("eventlog_create_failed:{error}")))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_log() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kiana-eventlog-{stamp}.jsonl"))
    }

    #[tokio::test]
    async fn append_order_is_preserved_per_request() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        store
            .append(RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap())
            .await
            .unwrap();
        store
            .append(RuntimeEvent::new(request_id, 2, "completed", Value::Null).unwrap())
            .await
            .unwrap();
        let events = store.read_request(&request_id).await.unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            ["accepted", "completed"]
        );
    }

    #[tokio::test]
    async fn duplicate_or_non_monotonic_sequence_is_rejected() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        store
            .append(RuntimeEvent::new(request_id, 2, "running", Value::Null).unwrap())
            .await
            .unwrap();
        let error = store
            .append(RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Conflict("event_sequence_not_monotonic".to_owned())
        );
    }

    #[tokio::test]
    async fn jsonl_survives_reopen_and_does_not_truncate_the_first_run() {
        let path = temp_log();
        let first_request = RequestId::new();
        let second_request = RequestId::new();
        {
            let store = JsonlEventLog::open(&path).unwrap();
            store
                .append(RuntimeEvent::new(first_request, 1, "run.completed", Value::Null).unwrap())
                .await
                .unwrap();
        }
        {
            let store = JsonlEventLog::open(&path).unwrap();
            store
                .append(RuntimeEvent::new(second_request, 1, "run.completed", Value::Null).unwrap())
                .await
                .unwrap();
            let all = store.read_all().await.unwrap();
            assert_eq!(all.len(), 2);
            assert_eq!(all[0].request_id, first_request);
            assert_eq!(all[1].request_id, second_request);
        }
        let reopened = JsonlEventLog::open(&path).unwrap();
        let all = reopened.read_all().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].request_id, first_request);
    }

    #[test]
    fn jsonl_corrupt_line_fails_closed() {
        let path = temp_log();
        fs::write(&path, "{\"kind\":\"not-an-event\"}\n").unwrap();
        let error = JsonlEventLog::open(&path).unwrap_err();
        assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    }

    #[tokio::test]
    async fn jsonl_repairs_a_torn_final_line_and_preserves_prior_events() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap();
        let encoded = serde_json::to_string(&first).unwrap();
        fs::write(&path, format!("{encoded}\n{{\"event_id\":")).unwrap();

        let store = JsonlEventLog::open(&path).unwrap();
        assert_eq!(store.read_all().await.unwrap(), vec![first.clone()]);
        let repaired = fs::read_to_string(&path).unwrap();
        assert_eq!(repaired, format!("{encoded}\n"));

        let second = RuntimeEvent::new(request_id, 2, "run.completed", Value::Null).unwrap();
        store.append(second.clone()).await.unwrap();
        assert_eq!(
            store.read_request(&request_id).await.unwrap(),
            vec![first, second]
        );
    }

    #[test]
    fn jsonl_malformed_final_complete_line_fails_closed() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap();
        let encoded = serde_json::to_string(&first).unwrap();
        fs::write(&path, format!("{encoded}\n{{\"kind\":\"not-an-event\"}}")).unwrap();
        let error = JsonlEventLog::open(&path).unwrap_err();
        assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    }

    #[test]
    fn jsonl_malformed_first_line_fails_closed() {
        let path = temp_log();
        fs::write(&path, "{\"event_id\":").unwrap();
        let error = JsonlEventLog::open(&path).unwrap_err();
        assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    }

    #[tokio::test]
    async fn append_expected_zero_on_empty_stream_succeeds() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        store
            .append_expected(
                RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap(),
                Some(0),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn append_expected_requires_matching_stream_version() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        store
            .append(RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap())
            .await
            .unwrap();

        let error = store
            .append_expected(
                RuntimeEvent::new(request_id, 2, "completed", Value::Null).unwrap(),
                Some(0),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Conflict("event_stream_version_mismatch".to_owned())
        );

        store
            .append_expected(
                RuntimeEvent::new(request_id, 2, "completed", Value::Null).unwrap(),
                Some(1),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn concurrent_expected_appends_allow_only_one_writer() {
        let store = Arc::new(MemoryEventLog::new());
        let request_id = RequestId::new();
        let first = store.append_expected(
            RuntimeEvent::new(request_id, 1, "first", Value::Null).unwrap(),
            Some(0),
        );
        let second = store.append_expected(
            RuntimeEvent::new(request_id, 1, "second", Value::Null).unwrap(),
            Some(0),
        );
        let (first, second) = tokio::join!(first, second);
        let results = [first, second];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    matches!(
                        result,
                        Err(PortError::Conflict(reason))
                            if reason == "event_stream_version_mismatch"
                    )
                })
                .count(),
            1
        );
        assert_eq!(store.read_request(&request_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn idempotent_retry_replays_without_appending() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_stream_metadata("request", request_id.to_string(), 1)
            .with_idempotency_key("request-accepted-1");
        let first = store.append_idempotent(event.clone()).await.unwrap();
        let retry = store.append_idempotent(event).await.unwrap();

        assert!(!first.replayed);
        assert!(retry.replayed);
        assert_eq!(first.event.event_id, retry.event.event_id);
        assert_eq!(store.read_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn idempotent_expected_retry_replays_before_cas_check() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_idempotency_key("expected-retry");
        let first = store
            .append_idempotent_expected(event.clone(), Some(0))
            .await
            .unwrap();
        let retry = store
            .append_idempotent_expected(event, Some(0))
            .await
            .unwrap();
        assert!(!first.replayed);
        assert!(retry.replayed);
        assert_eq!(store.read_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn idempotency_key_rejects_a_different_payload() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        let first = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_idempotency_key("same-key");
        store.append_idempotent(first).await.unwrap();

        let second = RuntimeEvent::new(request_id, 1, "run.completed", Value::Null)
            .unwrap()
            .with_idempotency_key("same-key");
        let error = store.append_idempotent(second).await.unwrap_err();
        assert_eq!(
            error,
            PortError::Conflict("event_idempotency_key_payload_mismatch".to_owned())
        );
    }

    #[tokio::test]
    async fn jsonl_idempotent_retry_reloads_and_replays() {
        let path = temp_log();
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_idempotency_key("disk-key");
        let first = JsonlEventLog::open(&path).unwrap();
        let first_result = first
            .append_idempotent_expected(event.clone(), Some(0))
            .await
            .unwrap();
        let second = JsonlEventLog::open(&path).unwrap();
        let retry = second
            .append_idempotent_expected(event, Some(0))
            .await
            .unwrap();

        assert!(!first_result.replayed);
        assert!(retry.replayed);
        assert_eq!(retry.event.event_id, first_result.event.event_id);
        assert_eq!(second.read_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn concurrent_jsonl_idempotent_appends_create_one_record() {
        let path = temp_log();
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_idempotency_key("concurrent-key");
        let first = std::sync::Arc::new(JsonlEventLog::open(&path).unwrap());
        let second = std::sync::Arc::new(JsonlEventLog::open(&path).unwrap());
        let (left, right) = tokio::join!(
            first.append_idempotent(event.clone()),
            second.append_idempotent(event),
        );
        let results = [left.unwrap(), right.unwrap()];

        assert_eq!(results.iter().filter(|result| !result.replayed).count(), 1);
        assert_eq!(results.iter().filter(|result| result.replayed).count(), 1);
        assert_eq!(
            JsonlEventLog::open(&path)
                .unwrap()
                .read_all()
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn open_jsonl_instance_reads_events_appended_by_another_instance() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = JsonlEventLog::open(&path).unwrap();
        let second = JsonlEventLog::open(&path).unwrap();
        first
            .append(RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap())
            .await
            .unwrap();

        assert_eq!(second.read_request(&request_id).await.unwrap().len(), 1);
        assert_eq!(second.read_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn concurrent_jsonl_read_and_append_do_not_deadlock() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = std::sync::Arc::new(JsonlEventLog::open(&path).unwrap());
        let second = std::sync::Arc::new(JsonlEventLog::open(&path).unwrap());
        first
            .append(RuntimeEvent::new(request_id, 1, "run.started", Value::Null).unwrap())
            .await
            .unwrap();

        let read = second.read_all();
        let append =
            first.append(RuntimeEvent::new(request_id, 2, "run.completed", Value::Null).unwrap());
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let (read, append) = tokio::join!(read, append);
            (read.unwrap(), append.unwrap())
        })
        .await
        .expect("eventlog append/read lock-order deadlock");
        assert_eq!(result.0.len(), 1);
        assert_eq!(second.read_all().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn jsonl_expected_append_reloads_under_process_lock() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = JsonlEventLog::open(&path).unwrap();
        let second = JsonlEventLog::open(&path).unwrap();
        first
            .append_expected(
                RuntimeEvent::new(request_id, 1, "first", Value::Null).unwrap(),
                Some(0),
            )
            .await
            .unwrap();
        second
            .append_expected(
                RuntimeEvent::new(request_id, 2, "second", Value::Null).unwrap(),
                Some(1),
            )
            .await
            .unwrap();
        assert_eq!(second.read_request(&request_id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn aggregate_stream_cas_spans_request_ids() {
        let store = MemoryEventLog::new();
        let first_request = RequestId::new();
        let second_request = RequestId::new();
        let first = RuntimeEvent::new(first_request, 1, "run.started", Value::Null)
            .unwrap()
            .with_stream_metadata("run", "run-1", 1)
            .with_idempotency_key("run-1:1");
        store.append_expected(first, Some(0)).await.unwrap();

        let second = RuntimeEvent::new(second_request, 1, "run.completed", Value::Null)
            .unwrap()
            .with_stream_metadata("run", "run-1", 2)
            .with_idempotency_key("run-1:2");
        store.append_expected(second, Some(1)).await.unwrap();

        let stale = RuntimeEvent::new(RequestId::new(), 1, "run.failed", Value::Null)
            .unwrap()
            .with_stream_metadata("run", "run-1", 2)
            .with_idempotency_key("run-1:stale");
        assert_eq!(
            store.append_expected(stale, Some(1)).await.unwrap_err(),
            PortError::Conflict("event_stream_version_mismatch".to_owned())
        );
        assert_eq!(store.read_all().await.unwrap().len(), 2);
        assert_eq!(store.read_stream("run", "run-1").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn append_expected_conflict_does_not_write_jsonl() {
        let path = temp_log();
        let request_id = RequestId::new();
        let store = JsonlEventLog::open(&path).unwrap();
        store
            .append(RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap())
            .await
            .unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let error = store
            .append_expected(
                RuntimeEvent::new(request_id, 2, "completed", Value::Null).unwrap(),
                Some(0),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Conflict("event_stream_version_mismatch".to_owned())
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }
}
