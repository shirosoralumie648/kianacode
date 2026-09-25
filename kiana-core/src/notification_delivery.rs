//! Bounded notification delivery worker planning.
//!
//! The worker is a state/lease planner only. It never spawns a task, calls a channel, consumes a
//! Broker permit or changes HumanTask authority. A composition root may execute the resulting
//! typed plan through the existing controlled ports and must commit a separate receipt.

use kiana_domain::{
    NotificationDispatchIntent, NotificationOutboxRecord, NotificationOutboxState,
    NOTIFICATION_OUTBOX_MAX_LEASE_MS,
};

pub const NOTIFICATION_DELIVERY_WORKER_SCHEMA: &str = "kiana.notification-delivery-worker.v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationDeliveryWorker {
    pub schema: String,
    pub worker_id: String,
    pub authority_epoch: u64,
    pub lease_ttl_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotificationDeliveryPlan {
    ClaimRequired,
    ReclaimExpiredClaim,
    Dispatch(NotificationDispatchIntent),
    AwaitReceipt,
    ReconcileUnknown,
    Terminal,
}

impl NotificationDeliveryWorker {
    pub fn new(
        worker_id: impl Into<String>,
        authority_epoch: u64,
        lease_ttl_ms: u64,
    ) -> Result<Self, String> {
        let worker_id = worker_id.into();
        if worker_id.trim().is_empty() || worker_id.len() > 128 || worker_id.contains(['\0', '\n'])
        {
            return Err("notification_delivery_worker_id_invalid".to_owned());
        }
        if authority_epoch == 0
            || lease_ttl_ms == 0
            || lease_ttl_ms > NOTIFICATION_OUTBOX_MAX_LEASE_MS
        {
            return Err("notification_delivery_worker_config_invalid".to_owned());
        }
        Ok(Self {
            schema: NOTIFICATION_DELIVERY_WORKER_SCHEMA.to_owned(),
            worker_id,
            authority_epoch,
            lease_ttl_ms,
        })
    }

    pub fn plan(
        &self,
        record: &NotificationOutboxRecord,
        now_unix_ms: u64,
    ) -> Result<NotificationDeliveryPlan, String> {
        record.validate()?;
        if record.authority_epoch != self.authority_epoch {
            return Ok(NotificationDeliveryPlan::ReconcileUnknown);
        }
        match record.state {
            NotificationOutboxState::Pending => Ok(NotificationDeliveryPlan::ClaimRequired),
            NotificationOutboxState::Claimed => {
                let expires = record
                    .lease_expires_at_unix_ms
                    .ok_or_else(|| "notification_delivery_lease_missing".to_owned())?;
                if expires <= now_unix_ms {
                    Ok(NotificationDeliveryPlan::ReclaimExpiredClaim)
                } else {
                    let lease = record.current_lease()?;
                    Ok(NotificationDeliveryPlan::Dispatch(
                        record.dispatch_intent(&lease)?,
                    ))
                }
            }
            NotificationOutboxState::Submitted => {
                let expires = record
                    .lease_expires_at_unix_ms
                    .ok_or_else(|| "notification_delivery_lease_missing".to_owned())?;
                if expires <= now_unix_ms {
                    Ok(NotificationDeliveryPlan::ReconcileUnknown)
                } else {
                    Ok(NotificationDeliveryPlan::AwaitReceipt)
                }
            }
            NotificationOutboxState::Acknowledged
            | NotificationOutboxState::Failed
            | NotificationOutboxState::Unknown => Ok(NotificationDeliveryPlan::Terminal),
        }
    }
}
