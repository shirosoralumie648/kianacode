//! Bounded observability queue with explicit critical/best-effort semantics.
//!
//! The queue is a delivery aid, never an EventLog. Critical facts are not silently evicted. When
//! no critical slot is available, enqueue returns immediately so an EventStore commit cannot be
//! held hostage by a slow consumer; the caller must retain the committed fact and rescan it later.

use crate::ObservabilitySignalRecord;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::Notify;

const MAX_DROP_REASON_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservabilityQueueClass {
    Event,
    Audit,
    Approval,
    Recovery,
    Terminal,
    Log,
    Trace,
    Metric,
}

impl ObservabilityQueueClass {
    pub const fn is_critical(self) -> bool {
        matches!(
            self,
            Self::Event | Self::Audit | Self::Approval | Self::Recovery | Self::Terminal
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Audit => "audit",
            Self::Approval => "approval",
            Self::Recovery => "recovery",
            Self::Terminal => "terminal",
            Self::Log => "log",
            Self::Trace => "trace",
            Self::Metric => "metric",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueuedObservabilityItem {
    pub class: ObservabilityQueueClass,
    pub source_cursor: u64,
    pub signal: Option<ObservabilitySignalRecord>,
}

impl QueuedObservabilityItem {
    pub fn critical(class: ObservabilityQueueClass, source_cursor: u64) -> Self {
        Self {
            class,
            source_cursor,
            signal: None,
        }
    }

    pub fn signal(
        class: ObservabilityQueueClass,
        source_cursor: u64,
        signal: ObservabilitySignalRecord,
    ) -> Self {
        Self {
            class,
            source_cursor,
            signal: Some(signal),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservabilityQueueError {
    InvalidCapacity,
    Closed,
    BestEffortDropped { reason: String },
    CriticalQueueFull { class: ObservabilityQueueClass },
    FlushCancelled,
}

impl std::fmt::Display for ObservabilityQueueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCapacity => formatter.write_str("observability_queue_capacity_invalid"),
            Self::Closed => formatter.write_str("observability_queue_closed"),
            Self::BestEffortDropped { reason } => {
                write!(formatter, "observability_best_effort_dropped:{reason}")
            }
            Self::CriticalQueueFull { class } => {
                write!(
                    formatter,
                    "observability_critical_queue_full:{}",
                    class.as_str()
                )
            }
            Self::FlushCancelled => formatter.write_str("observability_flush_cancelled"),
        }
    }
}

impl std::error::Error for ObservabilityQueueError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservabilityQueueStats {
    pub capacity: usize,
    pub depth: usize,
    pub enqueued_total: u64,
    pub dequeued_total: u64,
    pub dropped_best_effort_total: u64,
    pub critical_rejected_total: u64,
    pub flush_sequence: u64,
    pub reopen_total: u64,
    pub closed: bool,
    pub last_drop_reason: Option<String>,
}

#[derive(Default)]
struct QueueState {
    items: VecDeque<QueuedObservabilityItem>,
    enqueued_total: u64,
    dequeued_total: u64,
    dropped_best_effort_total: u64,
    critical_rejected_total: u64,
    flush_sequence: u64,
    reopen_total: u64,
    closed: bool,
    last_drop_reason: Option<String>,
}

/// A bounded queue that never awaits while accepting an item.
#[derive(Clone)]
pub struct ObservabilityQueue {
    capacity: usize,
    state: Arc<Mutex<QueueState>>,
    item_available: Arc<Notify>,
    drained: Arc<Notify>,
}

impl std::fmt::Debug for ObservabilityQueue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ObservabilityQueue")
            .field("stats", &self.stats())
            .finish()
    }
}

impl ObservabilityQueue {
    pub fn new(capacity: usize) -> Result<Self, ObservabilityQueueError> {
        if capacity == 0 {
            return Err(ObservabilityQueueError::InvalidCapacity);
        }
        Ok(Self {
            capacity,
            state: Arc::new(Mutex::new(QueueState::default())),
            item_available: Arc::new(Notify::new()),
            drained: Arc::new(Notify::new()),
        })
    }

    pub fn stats(&self) -> ObservabilityQueueStats {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        ObservabilityQueueStats {
            capacity: self.capacity,
            depth: state.items.len(),
            enqueued_total: state.enqueued_total,
            dequeued_total: state.dequeued_total,
            dropped_best_effort_total: state.dropped_best_effort_total,
            critical_rejected_total: state.critical_rejected_total,
            flush_sequence: state.flush_sequence,
            reopen_total: state.reopen_total,
            closed: state.closed,
            last_drop_reason: state.last_drop_reason.clone(),
        }
    }

    fn set_drop_reason(state: &mut QueueState, reason: &str) {
        let mut reason = reason.to_owned();
        reason.truncate(MAX_DROP_REASON_BYTES);
        state.last_drop_reason = Some(reason);
    }

    /// Try to enqueue without waiting. A critical item may evict one best-effort item, never a
    /// critical item. If all slots are critical, the caller gets a synchronous refusal.
    pub fn try_enqueue(
        &self,
        item: QueuedObservabilityItem,
    ) -> Result<(), ObservabilityQueueError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return Err(ObservabilityQueueError::Closed);
        }
        if state.items.len() >= self.capacity {
            if item.class.is_critical() {
                if let Some(index) = state
                    .items
                    .iter()
                    .position(|queued| !queued.class.is_critical())
                {
                    state.items.remove(index);
                    state.dropped_best_effort_total =
                        state.dropped_best_effort_total.saturating_add(1);
                    Self::set_drop_reason(&mut state, "critical_admission_evicted_best_effort");
                } else {
                    state.critical_rejected_total = state.critical_rejected_total.saturating_add(1);
                    Self::set_drop_reason(&mut state, "critical_queue_full");
                    return Err(ObservabilityQueueError::CriticalQueueFull { class: item.class });
                }
            } else {
                state.dropped_best_effort_total = state.dropped_best_effort_total.saturating_add(1);
                Self::set_drop_reason(&mut state, "best_effort_queue_full");
                return Err(ObservabilityQueueError::BestEffortDropped {
                    reason: "best_effort_queue_full".to_owned(),
                });
            }
        }
        state.items.push_back(item);
        state.enqueued_total = state.enqueued_total.saturating_add(1);
        drop(state);
        self.item_available.notify_one();
        Ok(())
    }

    pub fn try_dequeue(&self) -> Option<QueuedObservabilityItem> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let item = state.items.pop_front();
        if item.is_some() {
            state.dequeued_total = state.dequeued_total.saturating_add(1);
            if state.items.is_empty() {
                state.flush_sequence = state.flush_sequence.saturating_add(1);
                self.drained.notify_waiters();
            }
        }
        item
    }

    pub async fn dequeue(&self) -> Option<QueuedObservabilityItem> {
        loop {
            if let Some(item) = self.try_dequeue() {
                return Some(item);
            }
            if self.stats().closed {
                return None;
            }
            self.item_available.notified().await;
        }
    }

    /// Wait until all queued items have been accepted by a consumer. It does not claim that an
    /// exporter persisted them; EventLog/MetricSink receipts remain separate evidence.
    pub async fn flush(&self) -> u64 {
        loop {
            let stats = self.stats();
            if stats.depth == 0 {
                return stats.flush_sequence;
            }
            self.drained.notified().await;
        }
    }

    pub async fn flush_cancellable(
        &self,
        mut cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<u64, ObservabilityQueueError> {
        loop {
            let stats = self.stats();
            if stats.depth == 0 {
                return Ok(stats.flush_sequence);
            }
            if *cancellation.borrow() {
                return Err(ObservabilityQueueError::FlushCancelled);
            }
            tokio::select! {
                _ = self.drained.notified() => {},
                changed = cancellation.changed() => {
                    if changed.is_err() || *cancellation.borrow() {
                        return Err(ObservabilityQueueError::FlushCancelled);
                    }
                }
            }
        }
    }

    pub fn shutdown(&self) -> ObservabilityQueueStats {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        let stats = ObservabilityQueueStats {
            capacity: self.capacity,
            depth: state.items.len(),
            enqueued_total: state.enqueued_total,
            dequeued_total: state.dequeued_total,
            dropped_best_effort_total: state.dropped_best_effort_total,
            critical_rejected_total: state.critical_rejected_total,
            flush_sequence: state.flush_sequence,
            reopen_total: state.reopen_total,
            closed: true,
            last_drop_reason: state.last_drop_reason.clone(),
        };
        drop(state);
        self.item_available.notify_waiters();
        self.drained.notify_waiters();
        stats
    }

    /// Reopen delivery after a controlled shutdown. Existing queued items are retained; this is
    /// a queue lifecycle operation, not a claim that a durable spool survived a process crash.
    pub fn reopen(&self) -> ObservabilityQueueStats {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = false;
        state.reopen_total = state.reopen_total.saturating_add(1);
        drop(state);
        self.stats()
    }
}
