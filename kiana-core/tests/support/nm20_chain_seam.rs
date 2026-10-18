//! NM-20 EventLog-backed seam: the durable outbox cases whose seam is a test dependency of
//! `kiana-core`.
//!
//! `kiana-eventlog` is a dev-dependency of this crate, so a library module cannot link it;
//! making it a library dependency would pull the journal/store stack into every `kiana-core`
//! consumer for a fixture. The contract stays in `src/notification_fault_harness.rs`; only the
//! *calls* live here.
//!
//! Everything below is a real call into `MemoryNotificationOutboxStore`: the bounded admission
//! ledger, the claim that reclaims an expired lease, the lease fence and the acknowledged record
//! are the adapter's own results. Nothing is simulated.

use kiana_core::{
    Nm20FaultCase, Nm20FaultHarnessRun, Nm20FaultObservation, Nm20FaultSeam, Nm20LibrarySeam,
    NM20_FAULT_HARNESS_SCHEMA,
};
use kiana_domain::{
    ActionRefId, DeliveryReceipt, DeliveryReceiptStatus, MessageId, Notification,
    NotificationChannel, NotificationOutboxClass, NotificationOutboxRecord,
    NotificationOutboxShedReason, NotificationOutboxState, SubscriptionId,
};
use kiana_eventlog::MemoryNotificationOutboxStore;
use kiana_ports::{NotificationOutboxStore, PortError};

const EPOCH: u64 = 7;
const CRASH_SEAM: &str = "kiana_eventlog::MemoryNotificationOutboxStore::claim_next_notification";
const SHED_SEAM: &str = "kiana_eventlog::MemoryNotificationOutboxStore::admit_notification";

/// Drives the two EventLog-backed cases and delegates everything else to the library seam.
pub struct ChainSeam {
    inner: Nm20LibrarySeam,
}

impl Default for ChainSeam {
    fn default() -> Self {
        Self {
            inner: Nm20LibrarySeam,
        }
    }
}

impl Nm20FaultSeam for ChainSeam {
    fn inject(&mut self, case: Nm20FaultCase) -> Result<Nm20FaultObservation, String> {
        match case {
            Nm20FaultCase::CrashAfterClaim => drive_crash_after_claim(),
            Nm20FaultCase::BoundedQueueShed => drive_bounded_queue_shed(),
            other => self.inner.inject(other),
        }
    }
}

/// Build the report from every case this seam can reach. The daemon-only channel cases
/// (`slow_subscriber`, `multi_subscriber_isolation`) stay missing and are driven by the daemon
/// fixture target; a run never claims a seam it did not call.
pub fn run_all(
    seed: u64,
    source_cursor: u64,
    source_event_ids: Vec<String>,
) -> Nm20FaultHarnessRun {
    let mut seam = ChainSeam::default();
    let mut cases = Vec::new();
    for case in Nm20FaultCase::ALL {
        match seam.inject(case) {
            Ok(observation) => cases.push(observation),
            Err(error) if error.starts_with("nm20_adapter_seam_required:") => continue,
            Err(error) => panic!("nm20 chain seam failed on {}: {error}", case.as_str()),
        }
    }
    let run = Nm20FaultHarnessRun::from_observations(seed, source_cursor, source_event_ids, cases)
        .expect("nm20 chain seam run");
    assert_eq!(run.schema, NM20_FAULT_HARNESS_SCHEMA);
    run.validate().expect("nm20 chain seam run valid");
    run
}

/// One record over a fresh notification. `actionable` selects the protected class.
fn outbox_record(
    class: NotificationOutboxClass,
    actionable: bool,
) -> Result<NotificationOutboxRecord, String> {
    let notification = Notification::new(
        MessageId::new(),
        "actor-20",
        None,
        vec!["project/nm20".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        1,
        actionable.then(ActionRefId::new),
    )
    .map_err(|error| format!("nm20_notification_invalid:{error}"))?;
    NotificationOutboxRecord::new(notification, class, SubscriptionId::new(), EPOCH, 100)
        .map_err(|error| format!("nm20_outbox_record_invalid:{error}"))
}

fn port_error(error: PortError) -> String {
    format!("nm20_outbox_port:{error}")
}

fn port_error_code(error: &PortError) -> String {
    match error {
        PortError::Conflict(code) | PortError::Failed(code) | PortError::Unavailable(code) => {
            code.clone()
        }
    }
}

fn block_on<F, T>(future: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("nm20_runtime_unavailable:{error}"))?
        .block_on(future)
}

/// Crash after claim on the durable store: worker-a claims and never submits, worker-b reclaims
/// the expired lease, the stale lease is fenced, and exactly one acknowledged record remains.
fn drive_crash_after_claim() -> Result<Nm20FaultObservation, String> {
    block_on(async {
        let store = MemoryNotificationOutboxStore::new();
        let record = outbox_record(NotificationOutboxClass::Actionable, true)?;
        store
            .enqueue_notification(record)
            .await
            .map_err(port_error)?;

        let (claimed, lease_a) = store
            .claim_next_notification("worker-a", EPOCH, 100, 50)
            .await
            .map_err(port_error)?
            .ok_or_else(|| "nm20_crash_after_claim_not_claimed".to_owned())?;
        if claimed.state != NotificationOutboxState::Claimed || lease_a.worker_id != "worker-a" {
            return Err("nm20_crash_after_claim_state_invalid".to_owned());
        }

        // While worker-a's lease is live there is nothing to reclaim.
        if store
            .claim_next_notification("worker-b", EPOCH, 140, 50)
            .await
            .map_err(port_error)?
            .is_some()
        {
            return Err("nm20_crash_after_claim_live_lease_stolen".to_owned());
        }

        // The crash window closes only through lease expiry, and the reclaim is a new attempt.
        let (reclaimed, lease_b) = store
            .claim_next_notification("worker-b", EPOCH, 150, 50)
            .await
            .map_err(port_error)?
            .ok_or_else(|| "nm20_crash_after_claim_not_reclaimed".to_owned())?;
        if reclaimed.attempt.attempt_number != 2
            || reclaimed.claim_generation != 2
            || lease_a.lease_token == lease_b.lease_token
        {
            return Err("nm20_crash_after_claim_reclaim_not_fenced".to_owned());
        }

        // The crashed worker's lease cannot submit the reclaimed record.
        let stale_code = match store.mark_notification_submitted(&lease_a, 150).await {
            Ok(_) => return Err("nm20_crash_after_claim_stale_submitted".to_owned()),
            Err(error) => port_error_code(&error),
        };
        if stale_code != "notification_outbox_lease_fence_mismatch" {
            return Err(format!("nm20_crash_after_claim_unexpected:{stale_code}"));
        }

        store
            .mark_notification_submitted(&lease_b, 151)
            .await
            .map_err(port_error)?;
        let receipt = DeliveryReceipt::new(
            reclaimed.notification.notification_id,
            reclaimed.attempt.delivery_attempt_id,
            reclaimed.notification.recipient_id.clone(),
            DeliveryReceiptStatus::Acknowledged,
            160,
            None,
        )
        .map_err(|error| format!("nm20_crash_after_claim_receipt_invalid:{error}"))?;
        let acknowledged = store
            .acknowledge_notification(&lease_b, &receipt, 161)
            .await
            .map_err(port_error)?;
        if acknowledged.state != NotificationOutboxState::Acknowledged {
            return Err("nm20_crash_after_claim_not_acknowledged".to_owned());
        }

        // Exactly one record, one delivery, and the committed transition chain agrees.
        let records = store.records().await;
        if records.len() != 1
            || records[0].state != NotificationOutboxState::Acknowledged
            || records[0].attempt.attempt_number != 2
        {
            return Err("nm20_crash_after_claim_record_set_invalid".to_owned());
        }
        store.rebuild_report().await.map_err(port_error)?;

        Nm20FaultObservation::new(
            Nm20FaultCase::CrashAfterClaim,
            CRASH_SEAM,
            true,
            stale_code,
            Some("stale worker submitting a reclaimed notification"),
            false,
            true,
            true,
            false,
            false,
            true,
        )
    })
}

/// Bounded best-effort queue: the shed is counted with a reason, protected records are never
/// shed, and an overflow enqueue is refused instead of silently dropped.
fn drive_bounded_queue_shed() -> Result<Nm20FaultObservation, String> {
    block_on(async {
        let store = MemoryNotificationOutboxStore::bounded(1);
        let first = store
            .admit_notification(
                outbox_record(NotificationOutboxClass::BestEffort, false)?,
                100,
            )
            .await
            .map_err(port_error)?;
        if !first.admitted || first.shed_reason.is_some() {
            return Err("nm20_queue_full_first_not_admitted".to_owned());
        }

        let shed = store
            .admit_notification(
                outbox_record(NotificationOutboxClass::BestEffort, false)?,
                100,
            )
            .await
            .map_err(port_error)?;
        if shed.admitted || shed.shed_reason != Some(NotificationOutboxShedReason::QueueFull) {
            return Err("nm20_queue_full_shed_not_recorded".to_owned());
        }

        // A protected (actionable) record is admitted even at capacity and never shed.
        let protected = store
            .admit_notification(
                outbox_record(NotificationOutboxClass::Actionable, true)?,
                100,
            )
            .await
            .map_err(port_error)?;
        if !protected.admitted || !protected.protected {
            return Err("nm20_queue_full_protected_shed".to_owned());
        }

        // An overflow enqueue for a best-effort record is refused, never silently dropped.
        let refused = match store
            .enqueue_notification(outbox_record(NotificationOutboxClass::BestEffort, false)?)
            .await
        {
            Ok(_) => return Err("nm20_queue_full_best_effort_stored".to_owned()),
            Err(error) => port_error_code(&error),
        };
        if refused != "notification_outbox_capacity_exceeded" {
            return Err(format!("nm20_queue_full_unexpected:{refused}"));
        }

        let pressure = store.pressure().await;
        if pressure.admitted != 2
            || pressure.shed != 1
            || pressure.best_effort_shed != 1
            || pressure.protected_shed != 0
        {
            return Err("nm20_queue_full_pressure_invalid".to_owned());
        }
        let records = store.records().await;
        if records.len() != 2
            || !records
                .iter()
                .any(|record| record.effective_class().protected())
        {
            return Err("nm20_queue_full_protected_record_lost".to_owned());
        }

        Nm20FaultObservation::new(
            Nm20FaultCase::BoundedQueueShed,
            SHED_SEAM,
            true,
            "notification_outbox_queue_full_shed_counted",
            Some("critical/actionable notification shed by queue pressure"),
            false,
            true,
            false,
            false,
            true,
            true,
        )
    })
}
