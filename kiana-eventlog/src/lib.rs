//! Append-only event storage adapters for Kiana.

use async_trait::async_trait;
use kiana_domain::{RequestId, RuntimeEvent};
use kiana_ports::{EventStorePort, PortError};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
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
        let mut events = self.events.write().await;
        reject_conflicts(&events, &event)?;
        events.push(event);
        Ok(())
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
        let mut events = self.events.write().await;
        reject_conflicts(&events, &event)?;
        append_jsonl_line(&self.path, &event)?;
        events.push(event);
        Ok(())
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

fn reject_conflicts(events: &[RuntimeEvent], event: &RuntimeEvent) -> Result<(), PortError> {
    if events
        .iter()
        .any(|existing| existing.event_id == event.event_id)
    {
        return Err(PortError::Conflict("event_id_duplicate".to_owned()));
    }
    if events.iter().rev().any(|existing| {
        existing.request_id == event.request_id && existing.sequence >= event.sequence
    }) {
        return Err(PortError::Conflict(
            "event_sequence_not_monotonic".to_owned(),
        ));
    }
    Ok(())
}

fn load_jsonl(path: &Path) -> Result<Vec<RuntimeEvent>, PortError> {
    let file = fs::File::open(path)
        .map_err(|error| PortError::Failed(format!("eventlog_open_failed:{error}")))?;
    let mut events = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line =
            line.map_err(|error| PortError::Failed(format!("eventlog_read_failed:{error}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<RuntimeEvent>(&line).map_err(|error| {
            PortError::Failed(format!("eventlog_corrupt:line={}:{}", index + 1, error))
        })?;
        reject_conflicts(&events, &event)?;
        events.push(event);
    }
    Ok(events)
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

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
}
