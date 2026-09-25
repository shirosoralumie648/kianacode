//! Deterministic notification urgency, due ordering and digest grouping.
//!
//! Priority is a presentation projection derived from the server-owned Human Inbox item. It never
//! grants authority, hides critical/Unknown items or changes the source notification status.

use kiana_domain::{json_digest, HumanInboxItem, HumanInboxKind};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const NOTIFICATION_PRIORITY_SCHEMA: &str = "kiana.notification-priority.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationUrgency {
    Low,
    Medium,
    High,
    Critical,
}

impl NotificationUrgency {
    pub fn rank(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationPriority {
    pub schema: String,
    pub item_id: String,
    pub urgency: NotificationUrgency,
    pub due_at_unix_ms: u64,
    pub source_cursor: u64,
    pub digest_group: String,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationPriorityError {
    #[error("notification_priority_item_invalid")]
    ItemInvalid,
    #[error("notification_priority_due_invalid")]
    DueInvalid,
    #[error("notification_priority_cursor_invalid")]
    CursorInvalid,
}

pub fn classify_notification(
    item: &HumanInboxItem,
) -> Result<NotificationPriority, NotificationPriorityError> {
    if item.item_id.trim().is_empty() || item.source_ref.trim().is_empty() {
        return Err(NotificationPriorityError::ItemInvalid);
    }
    let due_at_unix_ms = item
        .detail
        .get("due_at_unix_ms")
        .and_then(serde_json::Value::as_u64)
        .ok_or(NotificationPriorityError::DueInvalid)?;
    let source_cursor = item
        .detail
        .get("source_cursor")
        .and_then(serde_json::Value::as_u64)
        .ok_or(NotificationPriorityError::CursorInvalid)?;
    if source_cursor == 0 {
        return Err(NotificationPriorityError::CursorInvalid);
    }
    let urgency = match &item.kind {
        HumanInboxKind::Incident | HumanInboxKind::Reconciliation => NotificationUrgency::Critical,
        HumanInboxKind::Approval | HumanInboxKind::Acceptance | HumanInboxKind::Review => {
            NotificationUrgency::High
        }
        HumanInboxKind::Question | HumanInboxKind::Feedback => NotificationUrgency::Medium,
    };
    let digest_group = json_digest(&serde_json::json!({
        "schema": NOTIFICATION_PRIORITY_SCHEMA,
        "kind": &item.kind,
        "title": item.title,
        "source": item.source_ref,
    }));
    Ok(NotificationPriority {
        schema: NOTIFICATION_PRIORITY_SCHEMA.to_owned(),
        item_id: item.item_id.clone(),
        urgency,
        due_at_unix_ms,
        source_cursor,
        digest_group,
    })
}

pub fn compare_notification_priority(
    left: &NotificationPriority,
    right: &NotificationPriority,
) -> std::cmp::Ordering {
    right
        .urgency
        .rank()
        .cmp(&left.urgency.rank())
        .then_with(|| left.due_at_unix_ms.cmp(&right.due_at_unix_ms))
        .then_with(|| left.source_cursor.cmp(&right.source_cursor))
        .then_with(|| left.item_id.cmp(&right.item_id))
}
