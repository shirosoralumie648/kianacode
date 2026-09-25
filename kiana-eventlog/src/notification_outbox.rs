//! Deterministic notification outbox adapter for source/CI semantics.
//!
//! This adapter supplies one atomic mutex boundary for the outbox contract. It intentionally does
//! not claim restart, file durability, cross-process locking or channel delivery; those require a
//! separately evidenced durable store and external receipt adapter.

use async_trait::async_trait;
use kiana_domain::{NotificationOutboxLease, NotificationOutboxRecord, NotificationOutboxState};
use kiana_ports::{NotificationOutboxStore, PortError};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Default)]
struct NotificationOutboxStateStore {
    records: BTreeMap<String, NotificationOutboxRecord>,
}

/// In-process outbox adapter used by deterministic CI fixtures.
#[derive(Clone, Debug, Default)]
pub struct MemoryNotificationOutboxStore {
    state: Arc<Mutex<NotificationOutboxStateStore>>,
}

impl MemoryNotificationOutboxStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn read(&self, outbox_id: &str) -> Result<NotificationOutboxRecord, PortError> {
        let state = self.state.lock().await;
        state
            .records
            .get(outbox_id)
            .cloned()
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))
    }

    fn key(record: &NotificationOutboxRecord) -> String {
        record.outbox_id.to_string()
    }
}

#[async_trait]
impl NotificationOutboxStore for MemoryNotificationOutboxStore {
    async fn enqueue_notification(
        &self,
        record: NotificationOutboxRecord,
    ) -> Result<NotificationOutboxRecord, PortError> {
        record
            .validate()
            .map_err(|error| PortError::Failed(format!("notification_outbox_record:{error}")))?;
        let key = Self::key(&record);
        let mut state = self.state.lock().await;
        if state.records.contains_key(&key) {
            return Err(PortError::Conflict(
                "notification_outbox_duplicate".to_owned(),
            ));
        }
        state.records.insert(key, record.clone());
        Ok(record)
    }

    async fn claim_next_notification(
        &self,
        worker_id: &str,
        authority_epoch: u64,
        now_unix_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<Option<(NotificationOutboxRecord, NotificationOutboxLease)>, PortError> {
        let mut state = self.state.lock().await;
        let keys = state.records.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            let Some(record) = state.records.get_mut(&key) else {
                continue;
            };
            let claimable = record.state == NotificationOutboxState::Pending
                || (record.state == NotificationOutboxState::Claimed
                    && record
                        .lease_expires_at_unix_ms
                        .is_some_and(|expires| expires <= now_unix_ms));
            if !claimable {
                continue;
            }
            match record.claim(worker_id, authority_epoch, now_unix_ms, lease_ttl_ms) {
                Ok(lease) => return Ok(Some((record.clone(), lease))),
                Err(error) if error == "notification_outbox_lease_active" => continue,
                Err(error) => return Err(PortError::Conflict(error)),
            }
        }
        Ok(None)
    }

    async fn mark_notification_submitted(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(&lease.outbox_id.to_string())
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        record
            .mark_submitted(lease, now_unix_ms)
            .map_err(PortError::Conflict)?;
        Ok(record.clone())
    }

    async fn acknowledge_notification(
        &self,
        lease: &NotificationOutboxLease,
        receipt: &kiana_domain::DeliveryReceipt,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(&lease.outbox_id.to_string())
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        record
            .acknowledge(lease, receipt, now_unix_ms)
            .map_err(PortError::Conflict)?;
        Ok(record.clone())
    }

    async fn fail_notification(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(&lease.outbox_id.to_string())
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        record
            .fail(lease, now_unix_ms)
            .map_err(PortError::Conflict)?;
        Ok(record.clone())
    }

    async fn reconcile_notification_unknown(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxRecord, PortError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(&lease.outbox_id.to_string())
            .ok_or_else(|| PortError::Conflict("notification_outbox_missing".to_owned()))?;
        record
            .reconcile_unknown(lease, now_unix_ms)
            .map_err(|error| {
                if error.contains("expired") {
                    PortError::Failed(format!("result_unknown:{error}"))
                } else {
                    PortError::Conflict(error)
                }
            })?;
        Ok(record.clone())
    }
}
