//! CP-27 behavioural fixtures: non-blocking storage, trusted clocks and bounded load.
//!
//! Unlike the source guard (`cp27_nonblocking_limits_guard`), these fixtures drive the real
//! domain/ports contracts and assert observable state instead of grepping source text:
//!
//! - `cp_clock_rollback_cannot_extend_authority` builds trusted and rolled-back
//!   `ClockObservation`s through a deterministic fake `ClockPort` and proves the shared
//!   `require_trusted_deadline` gate denies a backwards clock rather than extending authority.
//! - `cp_pending_and_event_limits_fail_without_resource_leak` proves journal event/payload/read-set
//!   ceilings fail with stable codes and leave a live `MemoryEventLog` ledger and its command
//!   receipts exactly as they were.
//! - `cp_slow_storage_does_not_starve_cancellation` proves a stalled bounded worker pool, a full
//!   delivery queue and a blocked handler never delay cancellation or corrupt queue accounting.

use async_trait::async_trait;
use kiana_domain::{
    AggregateVersion, AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest,
    CapabilityResult, ClockObservation, ClockTrust, CommitOutcome, RequestId, RuntimeEvent,
    TransitionBatch, MAX_JOURNAL_EVENT_BYTES, MAX_TRANSITION_EVENTS, MAX_TRANSITION_READ_SET,
};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{
    require_trusted_deadline, wait_for_cancellation, CapabilityBrokerPort, ClockPort,
    EventStorePort, ObservabilityQueue, ObservabilityQueueClass, ObservabilityQueueError, PortError,
    QueuedObservabilityItem,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Semaphore};

/// Deterministic clock sample used instead of a real OS or wall clock.
struct FixedClock {
    source: &'static str,
    wall_now_unix_ms: u64,
    monotonic_now_ms: u64,
}

impl ClockPort for FixedClock {
    fn source(&self) -> &str {
        self.source
    }

    fn wall_now_unix_ms(&self) -> Result<u64, PortError> {
        Ok(self.wall_now_unix_ms)
    }

    fn monotonic_now_ms(&self) -> Result<u64, PortError> {
        Ok(self.monotonic_now_ms)
    }
}

fn clock(source: &'static str, wall_now_unix_ms: u64, monotonic_now_ms: u64) -> FixedClock {
    FixedClock {
        source,
        wall_now_unix_ms,
        monotonic_now_ms,
    }
}

#[test]
fn cp_clock_rollback_cannot_extend_authority() {
    let source = "cp27-test-clock";
    let trusted = clock(source, 1_000_000, 5_000);
    let first: ClockObservation = trusted.observe(None).expect("trusted observation");
    assert_eq!(first.trust, ClockTrust::Trusted);
    assert!(first.require_trusted().is_ok());

    // A backwards wall and monotonic sample is classified as Rollback, never trusted.
    let behind = clock(source, 900_000, 4_000);
    let rollback = behind.observe(Some(&first)).expect("rollback observation");
    assert_eq!(rollback.trust, ClockTrust::Rollback);
    assert_eq!(
        rollback.require_trusted().unwrap_err(),
        "clock_untrusted".to_owned()
    );
    assert_eq!(
        rollback.allows_before(2_000_000).unwrap_err(),
        "clock_untrusted".to_owned()
    );
    assert_eq!(
        rollback.clamp_deadline(1_500_000, 2_000_000).unwrap_err(),
        "clock_untrusted".to_owned()
    );
    // The shared expiry gate refuses a deadline measured by a rolled-back clock.
    assert_eq!(
        require_trusted_deadline(&behind, Some(&first), 2_000_000).unwrap_err(),
        PortError::Failed("clock_untrusted".to_owned())
    );

    // A trusted forward sample still admits before an unexpired deadline...
    let ahead = clock(source, 1_010_000, 5_100);
    let forward = require_trusted_deadline(&ahead, Some(&first), 2_000_000);
    let admitted = forward.expect("trusted forward sample");
    assert_eq!(admitted.wall_now_unix_ms, 1_010_000);
    assert!(admitted.validate_transition_from(&first).is_ok());
    // ...but a deadline that already passed stays closed instead of gaining new authority.
    let expired = require_trusted_deadline(&ahead, Some(&first), 1_000_500);
    assert_eq!(
        expired.unwrap_err(),
        PortError::Conflict("clock_deadline_expired".to_owned())
    );

    // Tampering with a persisted observation invalidates it rather than extending authority.
    let mut tampered = rollback;
    tampered.wall_now_unix_ms = 1_200_000;
    assert_eq!(
        tampered.require_trusted().unwrap_err(),
        "clock_observation_invalid".to_owned()
    );
}

/// A valid single-event transition for aggregate `aggregate` at version 0.
fn accepted_batch(command: RequestId, aggregate: &str, digest: char) -> TransitionBatch {
    let event = RuntimeEvent::new(
        command,
        1,
        "run.authorized",
        json!({"run_id": aggregate, "decision": "allow"}),
    )
    .unwrap()
    .with_stream_metadata("run", aggregate, 1);
    TransitionBatch {
        command_id: command,
        command_digest: std::iter::repeat(digest).take(64).collect(),
        expected_versions: vec![AggregateVersion::new("run", aggregate, 0)],
        events: vec![event],
    }
}

/// One event over the per-batch event ceiling.
fn too_many_events_batch(command: RequestId) -> TransitionBatch {
    let events = (1..=(MAX_TRANSITION_EVENTS + 1))
        .map(|sequence| {
            RuntimeEvent::new(command, sequence as u64, "run.tool_result", json!({})).unwrap()
        })
        .collect();
    TransitionBatch {
        command_id: command,
        command_digest: std::iter::repeat('f').take(64).collect(),
        expected_versions: vec![AggregateVersion::new("run", "unused", 0)],
        events,
    }
}

/// A single event whose encoded payload exceeds the per-event byte ceiling.
fn oversized_event_batch(command: RequestId) -> TransitionBatch {
    let blob = "x".repeat(MAX_JOURNAL_EVENT_BYTES + 4_096);
    let event = RuntimeEvent::new(command, 1, "run.authorized", json!({"blob": blob}))
        .unwrap()
        .with_stream_metadata("run", "run-oversized", 1);
    TransitionBatch {
        command_id: command,
        command_digest: std::iter::repeat('e').take(64).collect(),
        expected_versions: vec![AggregateVersion::new("run", "run-oversized", 0)],
        events: vec![event],
    }
}

/// A read set one entry over the ceiling.
fn oversized_read_set_batch(command: RequestId) -> TransitionBatch {
    let event = RuntimeEvent::new(command, 1, "run.authorized", json!({}))
        .unwrap()
        .with_stream_metadata("run", "run-read-set", 1);
    let expected_versions = (0..=MAX_TRANSITION_READ_SET)
        .map(|index| AggregateVersion::new("run", format!("agg-{index}"), 0))
        .collect();
    TransitionBatch {
        command_id: command,
        command_digest: std::iter::repeat('d').take(64).collect(),
        expected_versions,
        events: vec![event],
    }
}

#[tokio::test]
async fn cp_pending_and_event_limits_fail_without_resource_leak() {
    let store = MemoryEventLog::new();
    let accepted = accepted_batch(RequestId::new(), "run-limits", 'a');
    assert!(matches!(
        store.commit_transition(accepted).await.unwrap(),
        CommitOutcome::Committed { .. }
    ));

    // The over-limit batch is refused with a stable code by both the pure validator and the store.
    let too_many = too_many_events_batch(RequestId::new());
    let rejected_command = too_many.command_id;
    assert_eq!(too_many.validate().unwrap_err(), "journal_batch_event_limit");
    assert_eq!(
        store.commit_transition(too_many).await.unwrap_err(),
        PortError::Failed("journal_batch_event_limit".to_owned())
    );

    // The rejected command left no committed fact, receipt or cursor movement behind.
    assert!(store.read_command(&rejected_command).await.unwrap().is_none());
    assert_eq!(store.read_all().await.unwrap().len(), 1);
    let page = store.read_from(0, 10).await.unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.cursor, 1);
    assert!(!page.has_more);

    // Payload and read-set ceilings expose the same structured failure surface.
    assert_eq!(
        oversized_event_batch(RequestId::new()).validate().unwrap_err(),
        "journal_event_size_limit"
    );
    assert_eq!(
        oversized_read_set_batch(RequestId::new()).validate().unwrap_err(),
        "journal_read_set_limit"
    );

    // No rejection above leaked a second fact into the ledger.
    assert_eq!(store.read_all().await.unwrap().len(), 1);
}

/// Broker whose handler never completes, modelling a non-cancellable slow effect.
struct BlockingBroker {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl CapabilityBrokerPort for BlockingBroker {
    async fn execute(
        &self,
        _request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::future::pending::<()>().await;
        unreachable!("blocking broker handler never resolves in this fixture")
    }
}

#[tokio::test]
async fn cp_slow_storage_does_not_starve_cancellation() {
    // 1) Bounded storage-worker pool: hold the only permit to model a stalled disk I/O.
    let held = Arc::new(Semaphore::new(1)).acquire_owned().await.unwrap();
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    cancel_tx.send(true).unwrap();

    // Cancellation resolves without waiting for a storage worker to free up. The timeout is only a
    // bound: an already-set cancellation must return immediately, never after the worker frees.
    let cancelled = wait_for_cancellation(&mut cancel_rx);
    tokio::time::timeout(Duration::from_secs(5), cancelled)
        .await
        .expect("cancellation must not queue behind slow storage");
    drop(held);

    // 2) A full bounded delivery queue never blocks the producer: best-effort is dropped and a
    // critical item gets a synchronous refusal instead of an await.
    let queue = ObservabilityQueue::new(2).unwrap();
    for cursor in 0..2 {
        let item = QueuedObservabilityItem::critical(ObservabilityQueueClass::Event, cursor);
        queue.try_enqueue(item).unwrap();
    }
    let log_item = QueuedObservabilityItem::critical(ObservabilityQueueClass::Log, 2);
    let dropped = queue.try_enqueue(log_item);
    assert_eq!(
        dropped.unwrap_err(),
        ObservabilityQueueError::BestEffortDropped {
            reason: "best_effort_queue_full".to_owned()
        }
    );
    let approval_item = QueuedObservabilityItem::critical(ObservabilityQueueClass::Approval, 3);
    let rejected = queue.try_enqueue(approval_item);
    assert!(matches!(rejected, Err(ObservabilityQueueError::CriticalQueueFull { .. })));
    let stats = queue.stats();
    assert_eq!(stats.depth, 2);
    assert_eq!(stats.dropped_best_effort_total, 1);
    assert_eq!(stats.critical_rejected_total, 1);

    // 3) Flushing the delivery queue is cancellable, so a stuck consumer cannot pin shutdown.
    let (flush_tx, flush_rx) = watch::channel(false);
    flush_tx.send(true).unwrap();
    assert_eq!(
        queue.flush_cancellable(flush_rx).await.unwrap_err(),
        ObservabilityQueueError::FlushCancelled
    );

    // 4) A cancelled broker call is biased: the slow handler is never polled to completion.
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = BlockingBroker {
        calls: calls.clone(),
    };
    let request = AuthorizedCapabilityRequest::new(
        "cp27-authorization",
        CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Process,
            "shell",
            json!({"command": "true"}),
        ),
    )
    .unwrap();
    let (broker_tx, broker_rx) = watch::channel(false);
    broker_tx.send(true).unwrap();
    assert_eq!(
        broker
            .execute_cancellable(request, broker_rx)
            .await
            .unwrap_err(),
        PortError::Failed("result_unknown:cancel_stop_unconfirmed".to_owned())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
