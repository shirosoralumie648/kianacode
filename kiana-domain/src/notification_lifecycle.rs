//! Append-only notification lifecycle facts for rebuild/retention projections.
//!
//! Withdraw, supersede and expiry are source facts. They never delete the original notification,
//! rewrite a HumanTask, or grant an action. Projectors may hide an item from a current inbox view
//! only after validating the fact's cursor, epoch and digest.

use crate::{json_digest, redact_text, EventId, NotificationId};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NOTIFICATION_LIFECYCLE_SCHEMA: &str = "kiana.notification-lifecycle.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLifecycleKind {
    Withdraw,
    Supersede,
    Expire,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationLifecycleFact {
    pub schema: String,
    pub notification_id: NotificationId,
    pub source_event_id: EventId,
    pub source_cursor: u64,
    pub data_epoch: u64,
    pub kind: NotificationLifecycleKind,
    #[serde(default)]
    pub replacement_notification_id: Option<NotificationId>,
    pub reason: String,
    pub fact_digest: String,
}

impl NotificationLifecycleFact {
    pub fn new(
        notification_id: NotificationId,
        source_event_id: EventId,
        source_cursor: u64,
        data_epoch: u64,
        kind: NotificationLifecycleKind,
        replacement_notification_id: Option<NotificationId>,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: NOTIFICATION_LIFECYCLE_SCHEMA.to_owned(),
            notification_id,
            source_event_id,
            source_cursor,
            data_epoch,
            kind,
            replacement_notification_id,
            reason: reason.into(),
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_LIFECYCLE_SCHEMA
            || self.notification_id.as_uuid().is_nil()
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.data_epoch == 0
            || self.reason.trim().is_empty()
            || self.reason.len() > 512
            || self.reason.contains(['\0', '\n', '\r'])
            || redact_text(&self.reason) != self.reason
        {
            return Err("notification_lifecycle_header_invalid".to_owned());
        }
        match self.kind {
            NotificationLifecycleKind::Supersede => {
                let replacement = self
                    .replacement_notification_id
                    .ok_or_else(|| "notification_lifecycle_replacement_required".to_owned())?;
                if replacement.as_uuid().is_nil() || replacement == self.notification_id {
                    return Err("notification_lifecycle_replacement_invalid".to_owned());
                }
            }
            NotificationLifecycleKind::Withdraw | NotificationLifecycleKind::Expire => {
                if self.replacement_notification_id.is_some() {
                    return Err("notification_lifecycle_replacement_unexpected".to_owned());
                }
            }
        }
        if self.fact_digest != self.digest() {
            return Err("notification_lifecycle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification_id": self.notification_id,
            "source_event_id": self.source_event_id,
            "source_cursor": self.source_cursor,
            "data_epoch": self.data_epoch,
            "kind": self.kind,
            "replacement_notification_id": self.replacement_notification_id,
            "reason": self.reason,
        }))
    }
}
