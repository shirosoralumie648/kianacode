//! Append-only event storage adapters for Kiana.

use async_trait::async_trait;
use kiana_domain::{RequestId, RuntimeEvent};
use kiana_ports::{EventStorePort, PortError};
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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
}
