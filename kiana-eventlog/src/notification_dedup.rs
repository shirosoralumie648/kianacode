//! CI-semantics notification intent deduplication.
//!
//! The map is deliberately an in-process adapter. Its mutex demonstrates the semantic atomicity
//! required by NM-07, but it does not claim restart, durable or cross-process proof. The adapter
//! stops at the notification intent boundary; outbox insertion and channel delivery belong to a
//! later step.

use async_trait::async_trait;
use kiana_domain::{NotificationDedupRecord, NotificationDedupRequest};
use kiana_ports::{NotificationDedupOutcome, NotificationDedupStore, PortError};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Default)]
struct NotificationDedupState {
    records: BTreeMap<String, NotificationDedupRecord>,
}

/// In-process notification intent store for CI and deterministic source checks.
#[derive(Clone, Debug, Default)]
pub struct MemoryNotificationDedupStore {
    state: Arc<Mutex<NotificationDedupState>>,
}

impl MemoryNotificationDedupStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn validate_key(key: &str) -> Result<(), PortError> {
        if key.trim().is_empty() || key.len() > 512 || key.contains(['\0', '\n', '\r']) {
            return Err(PortError::Failed(
                "notification_dedup_key_invalid".to_owned(),
            ));
        }
        Ok(())
    }

    fn conflict(reason: &'static str) -> PortError {
        PortError::Conflict(reason.to_owned())
    }
}

#[async_trait]
impl NotificationDedupStore for MemoryNotificationDedupStore {
    async fn claim_notification(
        &self,
        request: NotificationDedupRequest,
    ) -> Result<NotificationDedupOutcome, PortError> {
        request
            .validate()
            .map_err(|error| PortError::Failed(format!("notification_dedup_request:{error}")))?;
        Self::validate_key(&request.dedup_key)?;

        let mut state = self.state.lock().await;
        if let Some(existing) = state.records.get(&request.dedup_key) {
            if existing.content_digest != request.content_digest {
                return Err(Self::conflict("notification_dedup_content_conflict"));
            }
            if existing.subscription_revision != request.subscription_revision {
                return Err(Self::conflict(
                    "notification_dedup_subscription_revision_conflict",
                ));
            }
            if request
                .expected_revision
                .is_some_and(|revision| revision != existing.revision)
            {
                return Err(Self::conflict("notification_dedup_stale_revision"));
            }
            return Ok(NotificationDedupOutcome::Replayed(existing.clone()));
        }

        if request.expected_revision.is_some() {
            return Err(Self::conflict("notification_dedup_expected_record_missing"));
        }

        let record = NotificationDedupRecord::from_request(&request, 1)
            .map_err(|error| PortError::Failed(format!("notification_dedup_record:{error}")))?;
        state.records.insert(request.dedup_key, record.clone());
        Ok(NotificationDedupOutcome::Claimed(record))
    }

    async fn compare_and_swap_notification(
        &self,
        dedup_key: &str,
        expected_revision: u64,
        next: NotificationDedupRecord,
    ) -> Result<NotificationDedupRecord, PortError> {
        Self::validate_key(dedup_key)?;
        if expected_revision == 0 {
            return Err(PortError::Failed(
                "notification_dedup_expected_revision_invalid".to_owned(),
            ));
        }
        next.validate()
            .map_err(|error| PortError::Failed(format!("notification_dedup_record:{error}")))?;
        if next.dedup_key != dedup_key {
            return Err(Self::conflict("notification_dedup_key_conflict"));
        }
        let next_revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| PortError::Failed("notification_dedup_revision_overflow".to_owned()))?;

        let mut state = self.state.lock().await;
        let current = state
            .records
            .get(dedup_key)
            .cloned()
            .ok_or_else(|| Self::conflict("notification_dedup_record_missing"))?;
        if current.revision != expected_revision {
            return Err(Self::conflict("notification_dedup_stale_revision"));
        }
        if next.revision != next_revision {
            return Err(Self::conflict("notification_dedup_next_revision_invalid"));
        }
        if next.content_digest != current.content_digest
            || next.subscription_revision != current.subscription_revision
        {
            return Err(Self::conflict("notification_dedup_content_conflict"));
        }
        if next.notification.notification_id != current.notification.notification_id {
            return Err(Self::conflict(
                "notification_dedup_notification_identity_conflict",
            ));
        }

        state.records.insert(dedup_key.to_owned(), next.clone());
        Ok(next)
    }
}
