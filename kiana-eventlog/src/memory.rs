//! In-memory event-store adapter used by tests and ephemeral hosts.

use crate::event_store_core::{
    plan_append, plan_idempotent_append, validate_idempotency_key, AppendPlan,
};
use async_trait::async_trait;
use kiana_domain::{RequestId, RuntimeEvent};
use kiana_ports::{EventAppendResult, EventStorePort, PortError};
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
        let event = plan_append(&events, event, expected_version)?;
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
        let mut events = self.events.write().await;
        match plan_idempotent_append(&events, event, expected_version)? {
            AppendPlan::Replay(event) => Ok(EventAppendResult {
                event,
                replayed: true,
            }),
            AppendPlan::Append(event) => {
                events.push(event.clone());
                Ok(EventAppendResult {
                    event,
                    replayed: false,
                })
            }
        }
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
