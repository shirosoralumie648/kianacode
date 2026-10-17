//! Deterministic notification urgency, due ordering and digest grouping.
//!
//! Priority is a presentation projection derived from the server-owned Human Inbox item. It never
//! grants authority, hides critical/Unknown items or changes the source notification status.
//!
//! Every ordering key comes from the committed materialization: the urgency from the server-owned
//! `HumanInboxKind`, the due time and the source sequence from the committed detail. A client
//! clock, a presenter-supplied urgency or a pre-selected action cannot change the ordering, and a
//! materialization that tries to smuggle one of those keys into `detail` fails to classify.

use kiana_domain::{json_digest, HumanInboxItem, HumanInboxKind};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const NOTIFICATION_PRIORITY_SCHEMA: &str = "kiana.notification-priority.v1";
pub const NOTIFICATION_PRIORITY_MAX_ITEM_ID_BYTES: usize = 512;
pub const NOTIFICATION_PRIORITY_MAX_DIGEST_GROUP_BYTES: usize = 128;

/// Detail keys that would let a committer, model or presenter reprioritize or pre-answer an item.
///
/// A materialization that carries any of them is refused before a priority forms: urgency, due and
/// read/ACK state are server-derived projections, never client input.
pub const NOTIFICATION_PRIORITY_FORBIDDEN_DETAIL_HINTS: [&str; 11] = [
    "urgency",
    "priority",
    "priority_rank",
    "client_time_unix_ms",
    "server_time_unix_ms",
    "read",
    "acknowledged",
    "snoozed_until_unix_ms",
    "preselected_action",
    "approved",
    "decision",
];

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

    /// Critical items are never silenceable: snooze may not hide them.
    pub fn is_critical(self) -> bool {
        matches!(self, Self::Critical)
    }
}

/// Due state of one item relative to a server clock reading.
///
/// The state is a server projection: it is derived from the committed due time and the *server*
/// clock, never from a client-supplied timestamp.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDueState {
    Scheduled,
    Due,
    Overdue,
}

/// Classify a due time against a server clock reading. Zero is not a clock reading.
pub fn notification_due_state(
    due_at_unix_ms: u64,
    server_time_unix_ms: u64,
) -> Result<NotificationDueState, NotificationPriorityError> {
    if due_at_unix_ms == 0 || server_time_unix_ms == 0 {
        return Err(NotificationPriorityError::ServerTimeInvalid);
    }
    Ok(match server_time_unix_ms.cmp(&due_at_unix_ms) {
        std::cmp::Ordering::Less => NotificationDueState::Scheduled,
        std::cmp::Ordering::Equal => NotificationDueState::Due,
        std::cmp::Ordering::Greater => NotificationDueState::Overdue,
    })
}

/// Canonical digest of the server-derived ordering key of one item.
///
/// The digest binds exactly the fields `list` orders and displays (item identity, urgency, due,
/// source sequence, digest group). Read/ACK/snooze state and any client field are deliberately
/// outside it, so a forged row that reorders or re-labels an item cannot recompute the same key.
pub fn notification_priority_digest(
    item_id: &str,
    urgency: NotificationUrgency,
    due_at_unix_ms: u64,
    source_cursor: u64,
    digest_group: &str,
) -> String {
    json_digest(&serde_json::json!({
        "schema": NOTIFICATION_PRIORITY_SCHEMA,
        "item_id": item_id,
        "urgency": urgency,
        "due_at_unix_ms": due_at_unix_ms,
        "source_cursor": source_cursor,
        "digest_group": digest_group,
    }))
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

impl NotificationPriority {
    /// Validate one server-derived priority before it is used for ordering or display.
    pub fn validate(&self) -> Result<(), NotificationPriorityError> {
        if self.schema != NOTIFICATION_PRIORITY_SCHEMA {
            return Err(NotificationPriorityError::SchemaInvalid);
        }
        if self.item_id.trim().is_empty()
            || self.item_id.len() > NOTIFICATION_PRIORITY_MAX_ITEM_ID_BYTES
            || self.item_id.contains(['\0', '\n', '\r'])
        {
            return Err(NotificationPriorityError::ItemInvalid);
        }
        if self.due_at_unix_ms == 0 {
            return Err(NotificationPriorityError::DueInvalid);
        }
        if self.source_cursor == 0 {
            return Err(NotificationPriorityError::CursorInvalid);
        }
        if self.digest_group.trim().is_empty()
            || self.digest_group.len() > NOTIFICATION_PRIORITY_MAX_DIGEST_GROUP_BYTES
            || self.digest_group.contains(['\0', '\n', '\r'])
        {
            return Err(NotificationPriorityError::DigestGroupInvalid);
        }
        Ok(())
    }

    pub fn priority_digest(&self) -> String {
        notification_priority_digest(
            &self.item_id,
            self.urgency,
            self.due_at_unix_ms,
            self.source_cursor,
            &self.digest_group,
        )
    }

    /// Due state against a server clock reading.
    pub fn due_state(
        &self,
        server_time_unix_ms: u64,
    ) -> Result<NotificationDueState, NotificationPriorityError> {
        notification_due_state(self.due_at_unix_ms, server_time_unix_ms)
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationPriorityError {
    #[error("notification_priority_item_invalid")]
    ItemInvalid,
    #[error("notification_priority_schema_invalid")]
    SchemaInvalid,
    #[error("notification_priority_due_invalid")]
    DueInvalid,
    #[error("notification_priority_cursor_invalid")]
    CursorInvalid,
    #[error("notification_priority_digest_group_invalid")]
    DigestGroupInvalid,
    #[error("notification_priority_server_time_invalid")]
    ServerTimeInvalid,
    #[error("notification_priority_client_hint")]
    ClientHint,
}

pub fn classify_notification(
    item: &HumanInboxItem,
) -> Result<NotificationPriority, NotificationPriorityError> {
    if item.item_id.trim().is_empty() || item.source_ref.trim().is_empty() {
        return Err(NotificationPriorityError::ItemInvalid);
    }
    // A committed materialization may not carry a priority hint, a client clock, read/ACK state or
    // a pre-selected answer: those keys are the client's, and letting them through would make the
    // ordering (or the presentation state) client-controlled.
    if NOTIFICATION_PRIORITY_FORBIDDEN_DETAIL_HINTS
        .iter()
        .any(|hint| item.detail.get(hint).is_some())
    {
        return Err(NotificationPriorityError::ClientHint);
    }
    let due_at_unix_ms = item
        .detail
        .get("due_at_unix_ms")
        .and_then(serde_json::Value::as_u64)
        .ok_or(NotificationPriorityError::DueInvalid)?;
    if due_at_unix_ms == 0 {
        return Err(NotificationPriorityError::DueInvalid);
    }
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
    let priority = NotificationPriority {
        schema: NOTIFICATION_PRIORITY_SCHEMA.to_owned(),
        item_id: item.item_id.clone(),
        urgency,
        due_at_unix_ms,
        source_cursor,
        digest_group,
    };
    priority.validate()?;
    Ok(priority)
}

/// Total order of the inbox: urgency (critical first), then due time, then the committed source
/// sequence, then the item identity as the final deterministic tie-break.
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
