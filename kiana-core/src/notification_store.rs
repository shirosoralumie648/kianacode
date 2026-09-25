//! Bounded in-process notification query and projection state.
//!
//! `NotificationStore` consumes the existing committed-only `NotificationMaterializer`. It owns
//! only presentation read/ack state; it never copies HumanTask status, approves an action, writes
//! EventLog facts, invokes Broker or treats an empty projection as unavailable.

use crate::{classify_notification, compare_notification_priority, NotificationMaterializer};
use kiana_domain::{json_digest, HumanInboxItem, RuntimeEvent};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use thiserror::Error;

pub const NOTIFICATION_STORE_SCHEMA: &str = "kiana.notification-store.v1";
pub const NOTIFICATION_PAGE_SCHEMA: &str = "kiana.notification-page.v1";
pub const NOTIFICATION_MUTATION_SCHEMA: &str = "kiana.notification-projection-mutation.v1";
pub const NOTIFICATION_STORE_MAX_PAGE_SIZE: u16 = 30;

fn required(value: &str, field: &str, max: usize) -> Result<(), NotificationStoreError> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(NotificationStoreError::Invalid(field));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationListRequest {
    pub schema: String,
    pub recipient_id: String,
    pub limit: u16,
    #[serde(default)]
    pub after_item_id: Option<String>,
    #[serde(default)]
    pub expected_source_cursor: Option<u64>,
}

impl NotificationListRequest {
    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        if self.schema != NOTIFICATION_STORE_SCHEMA
            || self.limit == 0
            || self.limit > NOTIFICATION_STORE_MAX_PAGE_SIZE
            || self.expected_source_cursor == Some(0)
        {
            return Err(NotificationStoreError::Invalid("request"));
        }
        required(&self.recipient_id, "recipient_id", 256)?;
        if let Some(after) = &self.after_item_id {
            required(after, "after_item_id", 512)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationPageItem {
    pub item: HumanInboxItem,
    pub urgency: crate::NotificationUrgency,
    pub due_at_unix_ms: u64,
    pub digest_group: String,
    pub read: bool,
    pub acknowledged: bool,
    #[serde(default)]
    pub snoozed_until_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationPage {
    pub schema: String,
    pub source_cursor: u64,
    pub items: Vec<NotificationPageItem>,
    #[serde(default)]
    pub next_after_item_id: Option<String>,
    pub page_digest: String,
}

impl NotificationPage {
    fn new(
        source_cursor: u64,
        items: Vec<NotificationPageItem>,
        next_after_item_id: Option<String>,
    ) -> Result<Self, NotificationStoreError> {
        if source_cursor == 0 || items.len() > usize::from(NOTIFICATION_STORE_MAX_PAGE_SIZE) {
            return Err(NotificationStoreError::Invalid("page"));
        }
        let mut page = Self {
            schema: NOTIFICATION_PAGE_SCHEMA.to_owned(),
            source_cursor,
            items,
            next_after_item_id,
            page_digest: String::new(),
        };
        page.page_digest = page.digest();
        Ok(page)
    }

    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        if self.schema != NOTIFICATION_PAGE_SCHEMA
            || self.source_cursor == 0
            || self.items.len() > usize::from(NOTIFICATION_STORE_MAX_PAGE_SIZE)
        {
            return Err(NotificationStoreError::Invalid("page"));
        }
        if let Some(after) = &self.next_after_item_id {
            required(after, "next_after_item_id", 512)?;
        }
        if self.page_digest != self.digest() {
            return Err(NotificationStoreError::DigestMismatch);
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source_cursor": self.source_cursor,
            "items": self.items,
            "next_after_item_id": self.next_after_item_id,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationProjectionMutation {
    pub schema: String,
    pub item_id: String,
    pub source_cursor: u64,
    pub projection_revision: u64,
    pub read: bool,
    pub acknowledged: bool,
    pub changed: bool,
    #[serde(default)]
    pub snoozed_until_unix_ms: Option<u64>,
    pub mutation_digest: String,
}

impl NotificationProjectionMutation {
    fn new(
        item_id: String,
        source_cursor: u64,
        projection_revision: u64,
        read: bool,
        acknowledged: bool,
        changed: bool,
        snoozed_until_unix_ms: Option<u64>,
    ) -> Result<Self, NotificationStoreError> {
        required(&item_id, "item_id", 512)?;
        if source_cursor == 0 || projection_revision == 0 {
            return Err(NotificationStoreError::Invalid("mutation"));
        }
        let mut mutation = Self {
            schema: NOTIFICATION_MUTATION_SCHEMA.to_owned(),
            item_id,
            source_cursor,
            projection_revision,
            read,
            acknowledged,
            changed,
            snoozed_until_unix_ms,
            mutation_digest: String::new(),
        };
        mutation.mutation_digest = mutation.digest();
        Ok(mutation)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "item_id": self.item_id,
            "source_cursor": self.source_cursor,
            "projection_revision": self.projection_revision,
            "read": self.read,
            "acknowledged": self.acknowledged,
            "changed": self.changed,
            "snoozed_until_unix_ms": self.snoozed_until_unix_ms,
        }))
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationStoreError {
    #[error("notification_store_invalid:{0}")]
    Invalid(&'static str),
    #[error("notification_store_projection_unavailable")]
    ProjectionUnavailable,
    #[error("notification_store_cursor_stale")]
    CursorStale,
    #[error("notification_store_cursor_invalid")]
    CursorInvalid,
    #[error("notification_store_item_not_found")]
    ItemNotFound,
    #[error("notification_store_recipient_mismatch")]
    RecipientMismatch,
    #[error("notification_store_clock_regression")]
    ClockRegression,
    #[error("notification_store_digest_mismatch")]
    DigestMismatch,
    #[error("notification_store_materializer:{0}")]
    Materializer(String),
    #[error("notification_store_priority:{0}")]
    Priority(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ProjectionState {
    read_at_unix_ms: Option<u64>,
    acknowledged_at_unix_ms: Option<u64>,
    snoozed_until_unix_ms: Option<u64>,
    revision: u64,
}

/// In-process/in-app query projection. It is rebuildable from committed materializations and
/// intentionally does not pretend to be a durable NotificationStore.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotificationStore {
    materializer: NotificationMaterializer,
    projection: BTreeMap<String, ProjectionState>,
}

impl NotificationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_committed(
        &mut self,
        events: &[RuntimeEvent],
        source_cursor: u64,
    ) -> Result<(), NotificationStoreError> {
        self.materializer
            .apply_committed(events, source_cursor)
            .map_err(NotificationStoreError::Materializer)
    }

    pub fn source_cursor(&self) -> u64 {
        self.materializer.source_cursor()
    }

    pub fn list(
        &self,
        request: NotificationListRequest,
    ) -> Result<NotificationPage, NotificationStoreError> {
        request.validate()?;
        let source_cursor = self.source_cursor();
        if source_cursor == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        if let Some(expected) = request.expected_source_cursor {
            if expected < source_cursor {
                return Err(NotificationStoreError::CursorStale);
            }
            if expected > source_cursor {
                return Err(NotificationStoreError::ProjectionUnavailable);
            }
        }
        let items = self
            .materializer
            .human_inbox_items(&request.recipient_id)
            .map_err(NotificationStoreError::Materializer)?;
        let mut prioritized = items
            .into_iter()
            .map(|item| {
                let priority = classify_notification(&item)
                    .map_err(|error| NotificationStoreError::Priority(error.to_string()))?;
                Ok((item, priority))
            })
            .collect::<Result<Vec<_>, NotificationStoreError>>()?;
        prioritized.sort_by(|(_, left), (_, right)| compare_notification_priority(left, right));
        let start = match request.after_item_id.as_deref() {
            None => 0,
            Some(after) => prioritized
                .iter()
                .position(|(item, _)| item.item_id == after)
                .map(|index| index + 1)
                .ok_or(NotificationStoreError::CursorInvalid)?,
        };
        let page_items = prioritized
            .into_iter()
            .skip(start)
            .take(usize::from(request.limit))
            .map(|(item, priority)| {
                let state = self.projection.get(&item.item_id);
                NotificationPageItem {
                    urgency: priority.urgency,
                    due_at_unix_ms: priority.due_at_unix_ms,
                    digest_group: priority.digest_group,
                    read: state.and_then(|value| value.read_at_unix_ms).is_some(),
                    acknowledged: state
                        .and_then(|value| value.acknowledged_at_unix_ms)
                        .is_some(),
                    snoozed_until_unix_ms: state.and_then(|value| value.snoozed_until_unix_ms),
                    item,
                }
            })
            .collect::<Vec<_>>();
        let next_after_item_id = if page_items.len() == usize::from(request.limit) {
            page_items.last().map(|item| item.item.item_id.clone())
        } else {
            None
        };
        NotificationPage::new(source_cursor, page_items, next_after_item_id)
    }

    pub fn mark_read(
        &mut self,
        recipient_id: &str,
        item_id: &str,
        now_unix_ms: u64,
    ) -> Result<NotificationProjectionMutation, NotificationStoreError> {
        self.mutate(recipient_id, item_id, now_unix_ms, false)
    }

    pub fn acknowledge(
        &mut self,
        recipient_id: &str,
        item_id: &str,
        now_unix_ms: u64,
    ) -> Result<NotificationProjectionMutation, NotificationStoreError> {
        self.mutate(recipient_id, item_id, now_unix_ms, true)
    }

    pub fn snooze(
        &mut self,
        recipient_id: &str,
        item_id: &str,
        now_unix_ms: u64,
        snoozed_until_unix_ms: u64,
    ) -> Result<NotificationProjectionMutation, NotificationStoreError> {
        required(recipient_id, "recipient_id", 256)?;
        required(item_id, "item_id", 512)?;
        if now_unix_ms == 0 || snoozed_until_unix_ms <= now_unix_ms {
            return Err(NotificationStoreError::Invalid("snooze_window"));
        }
        if self.source_cursor() == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        let item = self
            .materializer
            .human_inbox_items(recipient_id)
            .map_err(NotificationStoreError::Materializer)?
            .into_iter()
            .find(|item| item.item_id == item_id)
            .ok_or(NotificationStoreError::ItemNotFound)?;
        let source_cursor = self.source_cursor();
        let (revision, read, acknowledged, changed) = {
            let state = self.projection.entry(item.item_id.clone()).or_default();
            if state
                .snoozed_until_unix_ms
                .is_some_and(|previous| snoozed_until_unix_ms < previous)
            {
                return Err(NotificationStoreError::ClockRegression);
            }
            let changed = state.snoozed_until_unix_ms != Some(snoozed_until_unix_ms);
            state.snoozed_until_unix_ms = Some(snoozed_until_unix_ms);
            if changed {
                state.revision = state.revision.saturating_add(1);
            }
            (
                state.revision,
                state.read_at_unix_ms.is_some(),
                state.acknowledged_at_unix_ms.is_some(),
                changed,
            )
        };
        NotificationProjectionMutation::new(
            item.item_id,
            source_cursor,
            revision,
            read,
            acknowledged,
            changed,
            Some(snoozed_until_unix_ms),
        )
    }

    fn mutate(
        &mut self,
        recipient_id: &str,
        item_id: &str,
        now_unix_ms: u64,
        acknowledge: bool,
    ) -> Result<NotificationProjectionMutation, NotificationStoreError> {
        required(recipient_id, "recipient_id", 256)?;
        required(item_id, "item_id", 512)?;
        if now_unix_ms == 0 {
            return Err(NotificationStoreError::Invalid("now_unix_ms"));
        }
        if self.source_cursor() == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        let item = self
            .materializer
            .human_inbox_items(recipient_id)
            .map_err(NotificationStoreError::Materializer)?
            .into_iter()
            .find(|item| item.item_id == item_id)
            .ok_or(NotificationStoreError::ItemNotFound)?;
        let source_cursor = self.source_cursor();
        let (revision, read, acknowledged, changed, snoozed_until) = {
            let state = self.projection.entry(item.item_id.clone()).or_default();
            if state
                .read_at_unix_ms
                .is_some_and(|previous| now_unix_ms < previous)
                || state
                    .acknowledged_at_unix_ms
                    .is_some_and(|previous| now_unix_ms < previous)
            {
                return Err(NotificationStoreError::ClockRegression);
            }
            let changed = if acknowledge {
                let changed = state.acknowledged_at_unix_ms.is_none();
                state.acknowledged_at_unix_ms = Some(now_unix_ms);
                changed
            } else {
                let changed = state.read_at_unix_ms.is_none();
                state.read_at_unix_ms = Some(now_unix_ms);
                changed
            };
            if changed {
                state.revision = state.revision.saturating_add(1);
            }
            (
                state.revision,
                state.read_at_unix_ms.is_some(),
                state.acknowledged_at_unix_ms.is_some(),
                changed,
                state.snoozed_until_unix_ms,
            )
        };
        NotificationProjectionMutation::new(
            item.item_id,
            source_cursor,
            revision,
            read,
            acknowledged,
            changed,
            snoozed_until,
        )
    }
}
