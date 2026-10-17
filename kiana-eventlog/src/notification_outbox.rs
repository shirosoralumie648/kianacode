//! Bounded, replayable notification outbox adapter.
//!
//! # Responsibility
//!
//! This module owns the *storage* half of NM-08: a deterministic in-process store plus an
//! append-only JSONL journal that a fresh instance can replay. It never dispatches anything: a
//! `NotificationDispatchIntent` is produced by the domain record, and a separate channel adapter
//! must return a validated `DeliveryReceipt`.
//!
//! # Guarantees
//!
//! - **One live claim per record.** A claim holds a lease token; a stale holder cannot submit,
//!   acknowledge, fail, release or abandon after another worker reclaimed the record
//!   (`notification_outbox_lease_fence_mismatch` / `..._lease_expired`).
//! - **Crash after claim.** A claim whose lease expired is reclaimed as a *new delivery attempt*,
//!   so the crashed attempt can never be submitted twice.
//! - **Submitted in doubt.** A submitted claim whose lease expired reconciles to `Unknown`
//!   (`result_unknown`) instead of being retried automatically.
//! - **Bounded queue.** A store built with `bounded`/`open_jsonl_bounded` keeps a cap on
//!   *best-effort* records. `Critical`/`Actionable` records are never shed; a shed is always
//!   reported with a reason in [`kiana_domain::NotificationOutboxPressure`].
//! - **Replay = rebuild.** Every mutation appends the resulting record, and every record's
//!   `Pending -> Claimed -> Submitted -> Acknowledged/Failed/Unknown` chain is re-derived and
//!   validated (`notification_outbox_rebuild_*`) on replay and on demand.
//!
//! # Limits
//!
//! The JSONL journal is a single-writer, in-process lock. It is not a cross-process transaction
//! boundary, not an fsync-per-frame WAL and not a production SQLite/EventLog store; it exists so a
//! restarted instance processes the same records with the same fences. The default `new()` store is
//! memory-only and makes no durability claim.

use async_trait::async_trait;
use kiana_domain::{
    json_digest, rebuild_notification_outbox_state, DeliveryAttemptId, DeliveryReceipt,
    NotificationOutboxAdmission, NotificationOutboxLease, NotificationOutboxPressure,
    NotificationOutboxRebuild, NotificationOutboxRecord, NotificationOutboxShedReason,
    NotificationOutboxState, NotificationOutboxTerminalReason, NotificationOutboxTransition,
};
use kiana_ports::{NotificationOutboxStore, PortError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// Hard bound on the JSONL journal. A bounded log is part of the contract: an unbounded append-only
/// file would turn a stuck queue into a disk exhaustion event.
pub const NOTIFICATION_OUTBOX_MAX_JOURNAL_BYTES: u64 = 32 * 1024 * 1024;
/// Hard bound on one journal frame, so a corrupt or truncated tail cannot be read into memory.
pub const NOTIFICATION_OUTBOX_MAX_JOURNAL_LINE_BYTES: usize = 4 * 1024 * 1024;
/// Default cap for best-effort records when a caller asks for a bounded store without a number.
pub const NOTIFICATION_OUTBOX_DEFAULT_MAX_BEST_EFFORT: usize = 256;

pub const NOTIFICATION_OUTBOX_REBUILD_SCHEMA: &str = "kiana.notification-outbox-rebuild.v1";

/// One journal frame: either a committed record snapshot or an admission outcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationOutboxJournalFrame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record: Option<NotificationOutboxRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    admission: Option<NotificationOutboxAdmission>,
}

/// Append-only JSONL journal for the CI/restart adapter.
#[derive(Debug)]
struct NotificationOutboxJournal {
    path: PathBuf,
    file: fs::File,
    bytes: u64,
}

impl NotificationOutboxJournal {
    fn open(path: &Path) -> Result<Self, PortError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                return Err(PortError::Unavailable(
                    "notification_outbox_journal_parent_missing".to_owned(),
                ));
            }
        }
        let existing = path.exists();
        if existing {
            let metadata = fs::metadata(path).map_err(|error| {
                PortError::Unavailable(format!("notification_outbox_journal_unreadable:{error}"))
            })?;
            if metadata.len() > NOTIFICATION_OUTBOX_MAX_JOURNAL_BYTES {
                return Err(PortError::Failed(
                    "notification_outbox_journal_too_large".to_owned(),
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err(PortError::Failed(
                        "notification_outbox_journal_permissions_too_broad".to_owned(),
                    ));
                }
            }
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|error| {
            PortError::Unavailable(format!("notification_outbox_journal_unreadable:{error}"))
        })?;
        let bytes = file
            .metadata()
            .map_err(|error| {
                PortError::Unavailable(format!("notification_outbox_journal_unreadable:{error}"))
            })?
            .len();
        Ok(Self {
            path: path.to_path_buf(),
            file,
            bytes,
        })
    }

    fn read_backlog(&self) -> Result<String, PortError> {
        let metadata = fs::metadata(&self.path).map_err(|error| {
            PortError::Unavailable(format!("notification_outbox_journal_unreadable:{error}"))
        })?;
        if metadata.len() > NOTIFICATION_OUTBOX_MAX_JOURNAL_BYTES {
            return Err(PortError::Failed(
                "notification_outbox_journal_too_large".to_owned(),
            ));
        }
        fs::read_to_string(&self.path).map_err(|error| {
            PortError::Unavailable(format!("notification_outbox_journal_unreadable:{error}"))
        })
    }

    fn append(&mut self, frame: &NotificationOutboxJournalFrame) -> Result<(), PortError> {
        let line = serde_json::to_string(frame).map_err(|error| {
            PortError::Failed(format!("notification_outbox_journal_encode:{error}"))
        })?;
        if line.len() > NOTIFICATION_OUTBOX_MAX_JOURNAL_LINE_BYTES {
            return Err(PortError::Failed(
                "notification_outbox_journal_frame_too_large".to_owned(),
            ));
        }
        let bytes = line.len() as u64 + 1;
        if self.bytes.saturating_add(bytes) > NOTIFICATION_OUTBOX_MAX_JOURNAL_BYTES {
            return Err(PortError::Failed(
                "notification_outbox_journal_full".to_owned(),
            ));
        }
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.write_all(b"\n"))
            .and_then(|()| self.file.flush())
            .map_err(|error| {
                PortError::Unavailable(format!("notification_outbox_journal_append_failed:{error}"))
            })?;
        self.bytes += bytes;
        Ok(())
    }
}

/// Per-record rebuild evidence for one store instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationOutboxRebuildReport {
    pub schema: String,
    pub records: Vec<NotificationOutboxRebuild>,
    pub report_digest: String,
}

impl NotificationOutboxRebuildReport {
    fn new(records: Vec<NotificationOutboxRebuild>) -> Result<Self, String> {
        let mut report = Self {
            schema: NOTIFICATION_OUTBOX_REBUILD_SCHEMA.to_owned(),
            records,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_OUTBOX_REBUILD_SCHEMA {
            return Err("notification_outbox_rebuild_header_invalid".to_owned());
        }
        let mut previous: Option<DeliveryAttemptId> = None;
        for record in &self.records {
            if record.outbox_id.as_uuid().is_nil() {
                return Err("notification_outbox_rebuild_identity_missing".to_owned());
            }
            if previous.is_some_and(|id| id >= record.outbox_id) {
                return Err("notification_outbox_rebuild_report_noncanonical".to_owned());
            }
            previous = Some(record.outbox_id);
        }
        validate_digest(&self.report_digest, "notification_outbox_rebuild_digest")?;
        if self.report_digest != self.digest() {
            return Err("notification_outbox_rebuild_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "records": self.records,
        }))
    }
}

/// Records, transition history, pressure ledger and optional journal.
#[derive(Debug)]
struct NotificationOutboxStateStore {
    records: BTreeMap<String, NotificationOutboxRecord>,
    history: BTreeMap<String, Vec<NotificationOutboxTransition>>,
    pressure: NotificationOutboxPressure,
    max_best_effort: Option<usize>,
    journal: Option<NotificationOutboxJournal>,
}

impl Default for NotificationOutboxStateStore {
    fn default() -> Self {
        Self {
            records: BTreeMap::new(),
            history: BTreeMap::new(),
            pressure: NotificationOutboxPressure::new(),
            max_best_effort: None,
            journal: None,
        }
    }
}

impl NotificationOutboxStateStore {
    fn key(record: &NotificationOutboxRecord) -> String {
        record.outbox_id.to_string()
    }

    fn best_effort_len(&self) -> usize {
        self.records
            .values()
            .filter(|record| !record.effective_class().protected())
            .count()
    }

    fn at_capacity(&self) -> bool {
        self.max_best_effort
            .is_some_and(|capacity| self.best_effort_len() >= capacity)
    }

    /// Derive the committed transition between two consecutive record snapshots.
    fn derive_transition(
        previous: &NotificationOutboxRecord,
        next: &NotificationOutboxRecord,
    ) -> Result<NotificationOutboxTransition, String> {
        if previous.outbox_id != next.outbox_id {
            return Err("notification_outbox_rebuild_identity_mismatch".to_owned());
        }
        let expected = previous
            .revision
            .checked_add(1)
            .ok_or_else(|| "notification_outbox_rebuild_revision_overflow".to_owned())?;
        if next.revision != expected {
            return Err("notification_outbox_rebuild_revision_gap".to_owned());
        }
        if next.updated_at_unix_ms < previous.updated_at_unix_ms {
            return Err("notification_outbox_rebuild_clock_regression".to_owned());
        }
        NotificationOutboxTransition::new(
            next.outbox_id,
            next.revision,
            previous.state,
            next.state,
            next.attempt.delivery_attempt_id,
            next.attempt.attempt_number,
            next.lease_token,
            next.updated_at_unix_ms,
        )
    }

    fn journal_record(&mut self, record: &NotificationOutboxRecord) -> Result<(), PortError> {
        let Some(journal) = self.journal.as_mut() else {
            return Ok(());
        };
        journal.append(&NotificationOutboxJournalFrame {
            record: Some(record.clone()),
            admission: None,
        })
    }

    fn journal_admission(
        &mut self,
        admission: &NotificationOutboxAdmission,
    ) -> Result<(), PortError> {
        let Some(journal) = self.journal.as_mut() else {
            return Ok(());
        };
        journal.append(&NotificationOutboxJournalFrame {
            record: None,
            admission: Some(admission.clone()),
        })
    }

    /// Persist the snapshot first, then apply it in memory, so a failed append never leaves the
    /// in-memory record ahead of the journal.
    fn commit(
        &mut self,
        next: NotificationOutboxRecord,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let key = Self::key(&next);
        let previous = self.records.get(&key).cloned();
        let transition = match previous.as_ref() {
            Some(previous) => Some(Self::derive_transition(previous, &next).map_err(|error| {
                PortError::Failed(format!("notification_outbox_transition:{error}"))
            })?),
            None => None,
        };
        self.journal_record(&next)?;
        let history = self.history.entry(key.clone()).or_default();
        if let Some(transition) = transition {
            history.push(transition);
        }
        self.records.insert(key, next.clone());
        Ok(next)
    }

    fn record_pressure(
        &mut self,
        admission: &NotificationOutboxAdmission,
    ) -> Result<(), PortError> {
        self.pressure
            .record(admission)
            .map_err(|error| PortError::Failed(format!("notification_outbox_pressure:{error}")))
    }

    /// Shared mutation path: load, mutate a clone, persist the committed snapshot.
    fn mutate(
        &mut self,
        outbox_id: &DeliveryAttemptId,
        mutate: impl FnOnce(&mut NotificationOutboxRecord) -> Result<(), String>,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let key = outbox_id.to_string();
        let mut next = self
            .records
            .get(&key)
            .cloned()
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        mutate(&mut next).map_err(PortError::Conflict)?;
        self.commit(next)
    }

    fn enqueue(
        &mut self,
        record: NotificationOutboxRecord,
    ) -> Result<NotificationOutboxRecord, PortError> {
        record
            .validate()
            .map_err(|error| PortError::Failed(format!("notification_outbox_record:{error}")))?;
        let key = Self::key(&record);
        if self.records.contains_key(&key) {
            return Err(PortError::Conflict(
                "notification_outbox_duplicate".to_owned(),
            ));
        }
        if !record.effective_class().protected() && self.at_capacity() {
            return Err(PortError::Conflict(
                "notification_outbox_capacity_exceeded".to_owned(),
            ));
        }
        let admission = NotificationOutboxAdmission::admitted(&record)
            .map_err(|error| PortError::Failed(format!("notification_outbox_admission:{error}")))?;
        let stored = self.commit(record)?;
        self.journal_admission(&admission)?;
        self.record_pressure(&admission)?;
        Ok(stored)
    }

    fn admit(
        &mut self,
        record: NotificationOutboxRecord,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxAdmission, PortError> {
        record
            .validate()
            .map_err(|error| PortError::Failed(format!("notification_outbox_record:{error}")))?;
        let key = Self::key(&record);
        if self.records.contains_key(&key) {
            return Err(PortError::Conflict(
                "notification_outbox_duplicate".to_owned(),
            ));
        }
        // A best-effort record that already left its window is shed with a reason instead of
        // entering the queue. Protected records are never shed: they stay visible and are later
        // terminated explicitly (a reason and a record) rather than disappearing.
        let shed_reason = if record.effective_class().protected() {
            None
        } else if record.notification.expires_at_unix_ms <= now_unix_ms {
            Some(NotificationOutboxShedReason::NotificationExpired)
        } else if self.at_capacity() {
            Some(NotificationOutboxShedReason::QueueFull)
        } else {
            None
        };
        let admission = match shed_reason {
            Some(reason) => {
                NotificationOutboxAdmission::shed(&record, reason).map_err(|error| {
                    PortError::Failed(format!("notification_outbox_admission:{error}"))
                })?
            }
            None => NotificationOutboxAdmission::admitted(&record).map_err(|error| {
                PortError::Failed(format!("notification_outbox_admission:{error}"))
            })?,
        };
        if admission.admitted {
            self.commit(record)?;
        }
        self.journal_admission(&admission)?;
        self.record_pressure(&admission)?;
        Ok(admission)
    }

    fn claim_next(
        &mut self,
        worker_id: &str,
        authority_epoch: u64,
        now_unix_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<Option<(NotificationOutboxRecord, NotificationOutboxLease)>, PortError> {
        let keys = self.records.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            let Some(current) = self.records.get(&key).cloned() else {
                continue;
            };
            let claimable = current.state == NotificationOutboxState::Pending
                || (current.state == NotificationOutboxState::Claimed
                    && current
                        .lease_expires_at_unix_ms
                        .is_some_and(|expires| expires <= now_unix_ms));
            if !claimable {
                continue;
            }
            let mut next = current;
            match next.claim(worker_id, authority_epoch, now_unix_ms, lease_ttl_ms) {
                Ok(lease) => {
                    let stored = self.commit(next)?;
                    return Ok(Some((stored, lease)));
                }
                Err(error) if error == "notification_outbox_lease_active" => continue,
                Err(error) => return Err(PortError::Conflict(error)),
            }
        }
        Ok(None)
    }

    fn rebuild_report(&self) -> Result<NotificationOutboxRebuildReport, PortError> {
        let mut records = Vec::with_capacity(self.records.len());
        for (key, record) in &self.records {
            let history = self.history.get(key).map(Vec::as_slice).unwrap_or(&[]);
            let rebuild = rebuild_notification_outbox_state(
                record.outbox_id,
                NotificationOutboxState::Pending,
                history,
            )
            .map_err(|error| PortError::Failed(format!("notification_outbox_rebuild:{error}")))?;
            if rebuild.state != record.state || rebuild.revision != record.revision {
                return Err(PortError::Failed(
                    "notification_outbox_rebuild_diverged".to_owned(),
                ));
            }
            records.push(rebuild);
        }
        NotificationOutboxRebuildReport::new(records)
            .map_err(|error| PortError::Failed(format!("notification_outbox_rebuild:{error}")))
    }

    fn replay(&mut self) -> Result<(), PortError> {
        let backlog = match self.journal.as_ref() {
            Some(journal) => journal.read_backlog()?,
            None => return Ok(()),
        };
        for line in backlog.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line.len() > NOTIFICATION_OUTBOX_MAX_JOURNAL_LINE_BYTES {
                return Err(PortError::Failed(
                    "notification_outbox_journal_frame_too_large".to_owned(),
                ));
            }
            let frame: NotificationOutboxJournalFrame =
                serde_json::from_str(line).map_err(|error| {
                    PortError::Failed(format!("notification_outbox_journal_corrupt:{error}"))
                })?;
            match (frame.record, frame.admission) {
                (Some(record), None) => {
                    record.validate().map_err(|error| {
                        PortError::Failed(format!("notification_outbox_journal_corrupt:{error}"))
                    })?;
                    let key = Self::key(&record);
                    match self.records.get(&key).cloned() {
                        Some(previous) => {
                            let transition =
                                Self::derive_transition(&previous, &record).map_err(|error| {
                                    PortError::Failed(format!(
                                        "notification_outbox_journal_corrupt:{error}"
                                    ))
                                })?;
                            self.history
                                .entry(key.clone())
                                .or_default()
                                .push(transition);
                            self.records.insert(key, record);
                        }
                        None => {
                            if record.state != NotificationOutboxState::Pending
                                || record.revision != 1
                            {
                                return Err(PortError::Failed(
                                    "notification_outbox_journal_corrupt:truncated_or_unbound_record"
                                        .to_owned(),
                                ));
                            }
                            self.history.entry(key.clone()).or_default();
                            self.records.insert(key, record);
                        }
                    }
                }
                (None, Some(admission)) => {
                    admission.validate().map_err(|error| {
                        PortError::Failed(format!("notification_outbox_journal_corrupt:{error}"))
                    })?;
                    self.record_pressure(&admission)?;
                }
                _ => {
                    return Err(PortError::Failed(
                        "notification_outbox_journal_frame_invalid".to_owned(),
                    ));
                }
            }
        }
        // Replay must reproduce the same state chain it stores, otherwise the journal is corrupt
        // (tampered, reverted or truncated) and the store refuses to open.
        self.rebuild_report()?;
        Ok(())
    }
}

/// Deterministic notification outbox adapter.
///
/// `new()` is memory-only. `open_jsonl`/`open_jsonl_bounded` additionally replay an append-only
/// JSONL journal so a restarted instance resumes with the same records, leases and pressure
/// counters. `bounded` caps best-effort records and refuses to shed protected ones.
#[derive(Clone, Debug)]
pub struct MemoryNotificationOutboxStore {
    state: Arc<Mutex<NotificationOutboxStateStore>>,
}

impl Default for MemoryNotificationOutboxStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryNotificationOutboxStore {
    /// Create an empty, unbounded, memory-only store.
    pub fn new() -> Self {
        Self::from_state(NotificationOutboxStateStore::default())
    }

    /// Create an empty memory-only store with a cap on best-effort records.
    pub fn bounded(max_best_effort: usize) -> Self {
        let mut state = NotificationOutboxStateStore::default();
        state.max_best_effort = Some(max_best_effort);
        Self::from_state(state)
    }

    /// Open a journal-backed store and replay every committed frame.
    pub fn open_jsonl(path: impl AsRef<Path>) -> Result<Self, PortError> {
        Self::open_jsonl_bounded(path, None)
    }

    /// Open a journal-backed store with an optional best-effort cap.
    pub fn open_jsonl_bounded(
        path: impl AsRef<Path>,
        max_best_effort: Option<usize>,
    ) -> Result<Self, PortError> {
        let mut state = NotificationOutboxStateStore {
            journal: Some(NotificationOutboxJournal::open(path.as_ref())?),
            ..NotificationOutboxStateStore::default()
        };
        state.max_best_effort = max_best_effort;
        state.replay()?;
        Ok(Self::from_state(state))
    }

    fn from_state(state: NotificationOutboxStateStore) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// Read one record by `outbox_id`.
    pub async fn read(&self, outbox_id: &str) -> Result<NotificationOutboxRecord, PortError> {
        let state = self.state.lock().await;
        state
            .records
            .get(outbox_id)
            .cloned()
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))
    }

    /// Snapshot every record, ordered by `outbox_id`.
    pub async fn records(&self) -> Vec<NotificationOutboxRecord> {
        let state = self.state.lock().await;
        state.records.values().cloned().collect()
    }

    /// Current admission/shed ledger.
    pub async fn pressure(&self) -> NotificationOutboxPressure {
        let state = self.state.lock().await;
        state.pressure.clone()
    }

    /// Rebuild every record from its committed transition chain and compare with the stored row.
    pub async fn rebuild_report(&self) -> Result<NotificationOutboxRebuildReport, PortError> {
        let state = self.state.lock().await;
        state.rebuild_report()
    }

    /// Best-effort cap currently configured for this store.
    pub async fn max_best_effort(&self) -> Option<usize> {
        let state = self.state.lock().await;
        state.max_best_effort
    }

    /// Classify the subscription fence for one stored record; the caller still performs the
    /// transition it decided on (`release`/`abandon`) through this port.
    pub async fn subscription_fence(
        &self,
        outbox_id: &str,
        subscription: &kiana_domain::Subscription,
        now_unix_ms: u64,
    ) -> Result<Result<(), kiana_domain::NotificationOutboxFenceRefusal>, PortError> {
        let record = self.read(outbox_id).await?;
        record
            .validate()
            .map_err(|error| PortError::Failed(format!("notification_outbox_record:{error}")))?;
        Ok(record.subscription_fence(subscription, now_unix_ms))
    }
}

#[async_trait]
impl NotificationOutboxStore for MemoryNotificationOutboxStore {
    async fn enqueue_notification(
        &self,
        record: NotificationOutboxRecord,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        state.enqueue(record)
    }

    async fn admit_notification(
        &self,
        record: NotificationOutboxRecord,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxAdmission, PortError> {
        let mut state = self.state.lock().await;
        state.admit(record, now_unix_ms)
    }

    async fn claim_next_notification(
        &self,
        worker_id: &str,
        authority_epoch: u64,
        now_unix_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<Option<(NotificationOutboxRecord, NotificationOutboxLease)>, PortError> {
        let mut state = self.state.lock().await;
        state.claim_next(worker_id, authority_epoch, now_unix_ms, lease_ttl_ms)
    }

    async fn mark_notification_submitted(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        state.mutate(&lease.outbox_id, |record| {
            record.mark_submitted(lease, now_unix_ms)
        })
    }

    async fn acknowledge_notification(
        &self,
        lease: &NotificationOutboxLease,
        receipt: &DeliveryReceipt,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        state.mutate(&lease.outbox_id, |record| {
            record.acknowledge(lease, receipt, now_unix_ms)
        })
    }

    async fn fail_notification(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        state.mutate(&lease.outbox_id, |record| record.fail(lease, now_unix_ms))
    }

    async fn reconcile_notification_unknown(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        state.mutate(&lease.outbox_id, |record| {
            record.reconcile_unknown(lease, now_unix_ms)
        })
    }

    async fn abandon_notification(
        &self,
        lease: &NotificationOutboxLease,
        reason: NotificationOutboxTerminalReason,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        state.mutate(&lease.outbox_id, |record| {
            record.abandon(lease, reason, now_unix_ms)
        })
    }

    async fn abandon_unclaimed_notification(
        &self,
        outbox_id: &DeliveryAttemptId,
        reason: NotificationOutboxTerminalReason,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        state.mutate(outbox_id, |record| {
            record.abandon_unclaimed(reason, now_unix_ms)
        })
    }

    async fn release_notification_claim(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        state.mutate(&lease.outbox_id, |record| {
            record.release(lease, now_unix_ms)
        })
    }
}
