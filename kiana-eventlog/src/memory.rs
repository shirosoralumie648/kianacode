//! In-memory adapter with the same atomic transition contract as the durable journal.
use crate::event_store_core::{validate_idempotency_key, AppendPlan};
use crate::journal_core::{append_result, capabilities, JournalState, TransitionPlan};
use async_trait::async_trait;
use kiana_domain::*;
use kiana_ports::{EventAppendResult, EventStorePort, PortError};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct MemoryEventLog {
    state: RwLock<JournalState>,
}
impl MemoryEventLog {
    pub fn new() -> Self {
        Self::default()
    }
}
#[async_trait]
impl EventStorePort for MemoryEventLog {
    fn supports_atomic_transitions(&self) -> bool {
        true
    }
    fn capabilities(&self) -> EventStoreCapabilities {
        capabilities(false)
    }
    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        let mut state = self.state.write().await;
        match state.plan_transition(batch, EventId::new())? {
            TransitionPlan::Replay(original) => Ok(CommitOutcome::Replayed { original }),
            TransitionPlan::Conflict(changed) => Ok(CommitOutcome::Conflict { changed }),
            TransitionPlan::Append { batch, receipt } => {
                state.apply_transition(batch, receipt.clone());
                Ok(CommitOutcome::Committed { receipt })
            }
        }
    }
    async fn read_command(&self, id: &RequestId) -> Result<Option<CommandReceipt>, PortError> {
        Ok(self.state.read().await.commands.get(id).cloned())
    }
    async fn read_from(&self, cursor: u64, limit: usize) -> Result<JournalPage, PortError> {
        self.state.read().await.page(cursor, limit)
    }
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.append_expected(event, None).await
    }
    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<(), PortError> {
        let mut state = self.state.write().await;
        let AppendPlan::Append(event) = state.plan_legacy(event, expected, false)? else {
            unreachable!("non-idempotent append");
        };
        state.apply_legacy(event);
        Ok(())
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
        let mut state = self.state.write().await;
        match state.plan_legacy(event, expected, true)? {
            AppendPlan::Replay(event) => Ok(append_result(event, true)),
            AppendPlan::Append(event) => {
                state.apply_legacy(event.clone());
                Ok(append_result(event, false))
            }
        }
    }
    async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.state.read().await.request(id))
    }
    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.state.read().await.events.clone())
    }
    async fn read_stream(&self, kind: &str, id: &str) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.state.read().await.stream(kind, id))
    }
}
