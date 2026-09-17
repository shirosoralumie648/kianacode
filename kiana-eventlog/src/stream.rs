//! Commit-boundary observer wrapper for EventStore adapters.
//!
//! This module deliberately observes the existing EventStore contract instead of creating a
//! second write path. A transition is offered to observers only after the wrapped store returns
//! `CommitOutcome::Committed`; replay, conflict and unknown outcomes never fan out.

use async_trait::async_trait;
use kiana_domain::{CommitOutcome, EventStoreCapabilities, JournalPage, RequestId, RuntimeEvent};
use kiana_ports::{
    CommitObserverFailure, CommittedTransition, EventAppendResult, EventStoreCommitObserver,
    EventStorePort, PortError,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

const MAX_COMMIT_OBSERVERS: usize = 32;
const MAX_OBSERVER_FAILURES: usize = 256;

/// EventStore decorator that broadcasts fresh atomic commits to registered observers.
///
/// The observer list is a wake/diagnostic mechanism, not a durability guarantee. Projection code
/// must still scan `read_from` from its durable cursor after restart or a dropped callback.
pub struct StreamEventStore {
    inner: Arc<dyn EventStorePort>,
    observers: Mutex<Vec<Arc<dyn EventStoreCommitObserver>>>,
    failures: Mutex<Vec<CommitObserverFailure>>,
    committed_cursor: AtomicU64,
}

impl std::fmt::Debug for StreamEventStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let observers = self
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        formatter
            .debug_struct("StreamEventStore")
            .field("observers", &observers)
            .field("failure_count", &self.observer_failures().len())
            .finish_non_exhaustive()
    }
}

impl StreamEventStore {
    pub fn new(inner: Arc<dyn EventStorePort>) -> Self {
        Self {
            inner,
            observers: Mutex::new(Vec::new()),
            failures: Mutex::new(Vec::new()),
            committed_cursor: AtomicU64::new(0),
        }
    }

    pub fn wrap(inner: Arc<dyn EventStorePort>) -> Arc<Self> {
        Arc::new(Self::new(inner))
    }

    pub fn with_observer(
        inner: Arc<dyn EventStorePort>,
        observer: Arc<dyn EventStoreCommitObserver>,
    ) -> Result<Arc<Self>, PortError> {
        let store = Arc::new(Self::new(inner));
        store.register_observer(observer)?;
        Ok(store)
    }

    pub fn register_observer(
        &self,
        observer: Arc<dyn EventStoreCommitObserver>,
    ) -> Result<(), PortError> {
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| PortError::Failed("eventlog_observer_registry_poisoned".to_owned()))?;
        if observers.len() >= MAX_COMMIT_OBSERVERS {
            return Err(PortError::Conflict("eventlog_observer_limit".to_owned()));
        }
        observers.push(observer);
        Ok(())
    }

    pub fn observer_failures(&self) -> Vec<CommitObserverFailure> {
        self.failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Latest cursor observed from a fresh commit in this process.
    ///
    /// This is only a bounded wake/projection hint. It is not a durable checkpoint and must not
    /// replace `EventStorePort::read_from` during restart or after a dropped callback.
    pub fn committed_cursor(&self) -> kiana_domain::EventCursor {
        self.committed_cursor.load(Ordering::SeqCst)
    }

    fn record_failure(&self, failure: CommitObserverFailure) {
        let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        if failures.len() >= MAX_OBSERVER_FAILURES {
            failures.remove(0);
        }
        failures.push(failure);
    }

    async fn notify_committed(
        &self,
        batch: kiana_domain::TransitionBatch,
        receipt: kiana_domain::CommandReceipt,
    ) {
        let command_id = batch.command_id;
        let commit_id = receipt.commit_id;
        let source_cursor = receipt.cursor;
        let notification = match CommittedTransition::new(batch, receipt) {
            Ok(notification) => notification,
            Err(error) => {
                // The wrapped store has already committed. Preserve that fact while exposing an
                // invariant breach as a bounded diagnostic instead of inventing a false reject.
                self.record_failure(CommitObserverFailure {
                    command_id,
                    commit_id,
                    source_cursor,
                    reason: format!("eventlog_commit_notification_invalid:{error}"),
                });
                return;
            }
        };
        let observers = self
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for observer in observers {
            if let Err(error) = observer.on_committed(notification.clone()).await {
                self.record_failure(CommitObserverFailure {
                    command_id: notification.batch.command_id,
                    commit_id: notification.receipt.commit_id,
                    source_cursor: notification.source_cursor(),
                    reason: error.to_string(),
                });
            }
        }
    }
}

/// Descriptive alias for callers that do not use the historical `StreamEventStore` name.
pub type CommitObservedEventStore = StreamEventStore;

#[async_trait]
impl EventStorePort for StreamEventStore {
    fn supports_atomic_transitions(&self) -> bool {
        self.inner.supports_atomic_transitions()
    }

    fn capabilities(&self) -> EventStoreCapabilities {
        self.inner.capabilities()
    }

    async fn flush(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.flush().await
    }

    async fn health(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.health().await
    }

    async fn last_durable_cursor(&self) -> Result<kiana_domain::EventCursor, PortError> {
        self.inner.last_durable_cursor().await
    }

    async fn close(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.close().await
    }

    async fn commit_transition(
        &self,
        batch: kiana_domain::TransitionBatch,
    ) -> Result<CommitOutcome, PortError> {
        let outcome = self.inner.commit_transition(batch.clone()).await?;
        if let CommitOutcome::Committed { receipt } = &outcome {
            self.committed_cursor
                .fetch_max(receipt.cursor, Ordering::SeqCst);
            self.notify_committed(batch, receipt.clone()).await;
        }
        Ok(outcome)
    }

    async fn read_command(
        &self,
        id: &RequestId,
    ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
        self.inner.read_command(id).await
    }

    async fn read_from(
        &self,
        cursor: kiana_domain::EventCursor,
        limit: usize,
    ) -> Result<JournalPage, PortError> {
        self.inner.read_from(cursor, limit).await
    }

    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }

    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        self.inner.append_expected(event, expected_version).await
    }

    async fn append_idempotent(&self, event: RuntimeEvent) -> Result<EventAppendResult, PortError> {
        self.inner.append_idempotent(event).await
    }

    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        self.inner
            .append_idempotent_expected(event, expected_version)
            .await
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(request_id).await
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_all().await
    }

    async fn read_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_stream(aggregate_type, aggregate_id).await
    }
}
