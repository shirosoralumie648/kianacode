//! Bounded in-process notification query and projection state.
//!
//! `NotificationStore` is the in-process/in-app channel surface of the notification system. It
//! consumes the existing committed-only `NotificationMaterializer` and exposes bounded,
//! server-scoped `list` pages plus projection-only `mark_read`/`acknowledge`/`snooze` mutations.
//!
//! The store owns presentation read/ACK state only. It never copies HumanTask/Approval status,
//! approves an action, appends a committed fact or receipt, drops source history, invokes Broker
//! or treats an empty projection as unavailable. Queries must be bound to the authenticated
//! recipient before a page or mutation is answered, so an entrypoint cannot widen the recipient
//! scope.
//!
//! Ordering and due state are server-derived: `list` orders by the `notification_priority`
//! projection and classifies due state against the server clock bound by the composition root, so
//! no client timestamp takes part. Read/ACK/snooze are replayable presentation facts
//! ([`NotificationReadStateFact`]) that a fresh process can fold back, and none of them is an
//! approval, a task status or a committed fact.

use crate::{
    classify_notification, compare_notification_priority, notification_due_state,
    notification_priority_digest, NotificationDueState, NotificationMaterializer,
    NotificationPriority, NotificationReadStateFact, NotificationUrgency,
};
use kiana_domain::{json_digest, HumanInboxItem, RuntimeEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const NOTIFICATION_STORE_SCHEMA: &str = "kiana.notification-store.v1";
pub const NOTIFICATION_PAGE_SCHEMA: &str = "kiana.notification-page.v1";
pub const NOTIFICATION_MUTATION_SCHEMA: &str = "kiana.notification-projection-mutation.v1";
pub const NOTIFICATION_INBOX_SUMMARY_SCHEMA: &str = "kiana.notification-inbox-summary.v1";
pub const NOTIFICATION_DIGEST_FOLD_SCHEMA: &str = "kiana.notification-digest-fold.v1";
pub const NOTIFICATION_STORE_MAX_PAGE_SIZE: u16 = 30;
/// A snooze is bounded projection state: it cannot push an item beyond this horizon.
pub const NOTIFICATION_STORE_MAX_SNOOZE_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
pub const NOTIFICATION_STORE_MAX_DIGEST_FOLD_MEMBERS: usize = 128;
const NOTIFICATION_STORE_MAX_TEXT_BYTES: usize = 512;
const NOTIFICATION_STORE_MAX_DIGEST_GROUP_BYTES: usize = 128;
const NOTIFICATION_STORE_MAX_ACTIONS: usize = 16;

fn required(value: &str, field: &'static str, max: usize) -> Result<(), NotificationStoreError> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(NotificationStoreError::Invalid(field));
    }
    Ok(())
}

/// True when a committed fact says the item's outcome is unresolved.
///
/// `unknown` is a server-owned source status, never model or presenter prose. Unknown items are
/// always queryable: they can be read and shown, but never silenced by a snooze.
fn item_is_unknown(item: &HumanInboxItem) -> bool {
    item.detail
        .get("notification")
        .and_then(|notification| notification.get("status"))
        .and_then(Value::as_str)
        == Some("unknown")
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
        if self.schema != NOTIFICATION_STORE_SCHEMA {
            return Err(NotificationStoreError::Invalid("schema"));
        }
        if self.limit == 0 || self.limit > NOTIFICATION_STORE_MAX_PAGE_SIZE {
            return Err(NotificationStoreError::Invalid("limit"));
        }
        if self.expected_source_cursor == Some(0) {
            return Err(NotificationStoreError::Invalid("expected_source_cursor"));
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
    pub urgency: NotificationUrgency,
    pub due_at_unix_ms: u64,
    pub source_cursor: u64,
    pub digest_group: String,
    pub read: bool,
    pub acknowledged: bool,
    #[serde(default)]
    pub snoozed_until_unix_ms: Option<u64>,
    /// Whether the stored snooze still applies at the page's server time.
    pub snooze_active: bool,
    /// Whether the source outcome is unresolved (`unknown` notification status).
    pub unknown: bool,
    /// Due state against the page's server time; absent when no server clock is bound.
    #[serde(default)]
    pub due_state: Option<NotificationDueState>,
    /// Digest of the server-derived ordering key, so a reordered or re-labelled row is refused.
    pub priority_digest: String,
}

impl NotificationPageItem {
    /// Validate one page row. The row carries a display-only item plus the presentation state of
    /// this process; it deliberately carries no authority, decision or task-status field.
    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        required(
            &self.item.item_id,
            "item_id",
            NOTIFICATION_STORE_MAX_TEXT_BYTES,
        )?;
        required(&self.item.title, "title", NOTIFICATION_STORE_MAX_TEXT_BYTES)?;
        required(
            &self.item.source_ref,
            "source_ref",
            NOTIFICATION_STORE_MAX_TEXT_BYTES,
        )?;
        if self.item.actions.len() > NOTIFICATION_STORE_MAX_ACTIONS {
            return Err(NotificationStoreError::Invalid("actions"));
        }
        if self.due_at_unix_ms == 0 {
            return Err(NotificationStoreError::Invalid("due_at_unix_ms"));
        }
        if self.source_cursor == 0 {
            return Err(NotificationStoreError::Invalid("source_cursor"));
        }
        required(
            &self.digest_group,
            "digest_group",
            NOTIFICATION_STORE_MAX_DIGEST_GROUP_BYTES,
        )?;
        if self.snoozed_until_unix_ms == Some(0) {
            return Err(NotificationStoreError::Invalid("snooze"));
        }
        if self.acknowledged && !self.read {
            return Err(NotificationStoreError::Invalid("acknowledgement"));
        }
        if self.snooze_active && self.snoozed_until_unix_ms.is_none() {
            return Err(NotificationStoreError::Invalid("snooze_active"));
        }
        // The displayed ordering key must be the committed one: the row may not show a due time or
        // a source sequence that differs from the materialization it claims to present.
        if self
            .item
            .detail
            .get("due_at_unix_ms")
            .and_then(Value::as_u64)
            != Some(self.due_at_unix_ms)
            || self
                .item
                .detail
                .get("source_cursor")
                .and_then(Value::as_u64)
                != Some(self.source_cursor)
        {
            return Err(NotificationStoreError::Invalid("page_row_binding"));
        }
        if self.unknown != item_is_unknown(&self.item) {
            return Err(NotificationStoreError::Invalid("page_row_unknown"));
        }
        if self.priority_digest
            != notification_priority_digest(
                &self.item.item_id,
                self.urgency,
                self.due_at_unix_ms,
                self.source_cursor,
                &self.digest_group,
            )
        {
            return Err(NotificationStoreError::DigestMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationPage {
    pub schema: String,
    pub source_cursor: u64,
    /// Server clock reading bound by the composition root when this page was produced.
    #[serde(default)]
    pub server_time_unix_ms: Option<u64>,
    pub items: Vec<NotificationPageItem>,
    #[serde(default)]
    pub next_after_item_id: Option<String>,
    pub page_digest: String,
}

impl NotificationPage {
    fn new(
        source_cursor: u64,
        server_time_unix_ms: Option<u64>,
        items: Vec<NotificationPageItem>,
        next_after_item_id: Option<String>,
    ) -> Result<Self, NotificationStoreError> {
        if source_cursor == 0 || items.len() > usize::from(NOTIFICATION_STORE_MAX_PAGE_SIZE) {
            return Err(NotificationStoreError::Invalid("page"));
        }
        let mut page = Self {
            schema: NOTIFICATION_PAGE_SCHEMA.to_owned(),
            source_cursor,
            server_time_unix_ms,
            items,
            next_after_item_id,
            page_digest: String::new(),
        };
        page.page_digest = page.digest();
        page.validate()?;
        Ok(page)
    }

    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        if self.schema != NOTIFICATION_PAGE_SCHEMA
            || self.source_cursor == 0
            || self.items.len() > usize::from(NOTIFICATION_STORE_MAX_PAGE_SIZE)
        {
            return Err(NotificationStoreError::Invalid("page"));
        }
        if self.server_time_unix_ms == Some(0) {
            return Err(NotificationStoreError::Invalid("server_time_unix_ms"));
        }
        for item in &self.items {
            item.validate()?;
            // Due state and snooze activation are derived from the page's server time: a row may
            // not claim a state the bound clock contradicts.
            match self.server_time_unix_ms {
                Some(server_time) => {
                    let due_state = notification_due_state(item.due_at_unix_ms, server_time)?;
                    if item.due_state != Some(due_state)
                        || item.snooze_active
                            != item
                                .snoozed_until_unix_ms
                                .is_some_and(|until| server_time < until)
                    {
                        return Err(NotificationStoreError::Invalid("page_row_server_time"));
                    }
                }
                None => {
                    if item.due_state.is_some()
                        || item.snooze_active != item.snoozed_until_unix_ms.is_some()
                    {
                        return Err(NotificationStoreError::Invalid("page_row_server_time"));
                    }
                }
            }
        }
        if let Some(after) = &self.next_after_item_id {
            required(after, "next_after_item_id", 512)?;
            // A continuation cursor must point at the last row of this page.  A page that points
            // anywhere else can loop a client forever or skip rows, so it never validates.
            if self.items.last().map(|item| item.item.item_id.as_str()) != Some(after.as_str()) {
                return Err(NotificationStoreError::Invalid("page_next_cursor"));
            }
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
            "server_time_unix_ms": self.server_time_unix_ms,
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
        required(&item_id, "item_id", NOTIFICATION_STORE_MAX_TEXT_BYTES)?;
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
        mutation.validate()?;
        Ok(mutation)
    }

    /// Validate a projection-mutation receipt.
    ///
    /// The receipt is a presentation acknowledgement, never a decision: it has no approval,
    /// decision, task-status or authority field, and a forged receipt carrying one fails to decode
    /// at the `deny_unknown_fields` boundary before it can ever validate.
    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        if self.schema != NOTIFICATION_MUTATION_SCHEMA {
            return Err(NotificationStoreError::Invalid("mutation_schema"));
        }
        required(&self.item_id, "item_id", NOTIFICATION_STORE_MAX_TEXT_BYTES)?;
        if self.source_cursor == 0 || self.projection_revision == 0 {
            return Err(NotificationStoreError::Invalid("mutation"));
        }
        if self.snoozed_until_unix_ms == Some(0) {
            return Err(NotificationStoreError::Invalid("mutation_snooze"));
        }
        if self.acknowledged && !self.read {
            return Err(NotificationStoreError::Invalid("mutation_acknowledgement"));
        }
        if self.mutation_digest != self.digest() {
            return Err(NotificationStoreError::DigestMismatch);
        }
        Ok(())
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

/// Unread/read/ACK/snooze/urgency counts for the bound recipient, stamped with server time.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationInboxCounts {
    pub total: u32,
    pub unread: u32,
    pub read: u32,
    pub acknowledged: u32,
    pub snoozed: u32,
    pub critical_total: u32,
    pub critical_unread: u32,
    pub unknown_total: u32,
    pub overdue: u32,
    pub due_now: u32,
    pub scheduled: u32,
}

impl NotificationInboxCounts {
    /// The counts must be internally coherent: every item is read or unread, an acknowledgement is
    /// a read, critical/unknown items are subsets, and the due buckets partition the total.
    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        if self.read.saturating_add(self.unread) != self.total
            || self.acknowledged > self.read
            || self.snoozed > self.total
            || self.critical_total > self.total
            || self.critical_unread > self.critical_total
            || self.critical_unread > self.unread
            || self.unknown_total > self.total
            || self
                .overdue
                .saturating_add(self.due_now)
                .saturating_add(self.scheduled)
                != self.total
        {
            return Err(NotificationStoreError::Invalid("summary_counts"));
        }
        Ok(())
    }
}

/// Badge/summary projection of one server-bound recipient at one server clock reading.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationInboxSummary {
    pub schema: String,
    pub recipient_id: String,
    pub source_cursor: u64,
    pub server_time_unix_ms: u64,
    pub counts: NotificationInboxCounts,
    pub digest_group_count: u32,
    pub summary_digest: String,
}

impl NotificationInboxSummary {
    pub fn new(
        recipient_id: impl Into<String>,
        source_cursor: u64,
        server_time_unix_ms: u64,
        counts: NotificationInboxCounts,
        digest_group_count: u32,
    ) -> Result<Self, NotificationStoreError> {
        let mut summary = Self {
            schema: NOTIFICATION_INBOX_SUMMARY_SCHEMA.to_owned(),
            recipient_id: recipient_id.into(),
            source_cursor,
            server_time_unix_ms,
            counts,
            digest_group_count,
            summary_digest: String::new(),
        };
        summary.summary_digest = summary.digest();
        summary.validate()?;
        Ok(summary)
    }

    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        if self.schema != NOTIFICATION_INBOX_SUMMARY_SCHEMA
            || self.source_cursor == 0
            || self.server_time_unix_ms == 0
        {
            return Err(NotificationStoreError::Invalid("summary_header"));
        }
        required(&self.recipient_id, "recipient_id", 256)?;
        self.counts.validate()?;
        // Every item carries exactly one digest group, so an empty inbox has none and a non-empty
        // one has at least one and never more groups than items.
        let groups_valid = if self.counts.total == 0 {
            self.digest_group_count == 0
        } else {
            self.digest_group_count >= 1 && self.digest_group_count <= self.counts.total
        };
        if !groups_valid {
            return Err(NotificationStoreError::Invalid("summary_digest_groups"));
        }
        if self.summary_digest != self.digest() {
            return Err(NotificationStoreError::DigestMismatch);
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "recipient_id": self.recipient_id,
            "source_cursor": self.source_cursor,
            "server_time_unix_ms": self.server_time_unix_ms,
            "counts": self.counts,
            "digest_group_count": self.digest_group_count,
        }))
    }
}

/// One item's contribution to a digest fold.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationDigestMember {
    pub item_id: String,
    pub urgency: NotificationUrgency,
    pub due_at_unix_ms: u64,
    pub source_cursor: u64,
    pub unknown: bool,
    pub read: bool,
    pub acknowledged: bool,
}

impl NotificationDigestMember {
    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        required(
            &self.item_id,
            "digest_member_item_id",
            NOTIFICATION_STORE_MAX_TEXT_BYTES,
        )?;
        if self.due_at_unix_ms == 0 || self.source_cursor == 0 {
            return Err(NotificationStoreError::Invalid("digest_member"));
        }
        if self.acknowledged && !self.read {
            return Err(NotificationStoreError::Invalid(
                "digest_member_acknowledgement",
            ));
        }
        Ok(())
    }
}

/// A digest fold: the server-side roll-up of one digest group inside one recipient's scope.
///
/// The fold is self-verifying: `validate` recomputes every aggregate (highest urgency, earliest
/// due, latest source sequence, critical/unknown/unread/ACK counts) from its members, so a fold
/// cannot claim a lower urgency than a member it contains -- a critical or Unknown item is never
/// swallowed by a digest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationDigestFold {
    pub schema: String,
    pub recipient_id: String,
    pub digest_group: String,
    pub urgency: NotificationUrgency,
    pub due_at_unix_ms: u64,
    pub source_cursor: u64,
    pub critical_count: u32,
    pub unknown_count: u32,
    pub unread_count: u32,
    pub acknowledged_count: u32,
    pub members: Vec<NotificationDigestMember>,
    pub fold_digest: String,
}

impl NotificationDigestFold {
    pub fn new(
        recipient_id: impl Into<String>,
        digest_group: impl Into<String>,
        mut members: Vec<NotificationDigestMember>,
    ) -> Result<Self, NotificationStoreError> {
        members.sort_by(|left, right| left.item_id.cmp(&right.item_id));
        let mut fold = Self {
            schema: NOTIFICATION_DIGEST_FOLD_SCHEMA.to_owned(),
            recipient_id: recipient_id.into(),
            digest_group: digest_group.into(),
            urgency: NotificationUrgency::Low,
            due_at_unix_ms: 0,
            source_cursor: 0,
            critical_count: 0,
            unknown_count: 0,
            unread_count: 0,
            acknowledged_count: 0,
            members,
            fold_digest: String::new(),
        };
        fold.urgency = fold
            .members
            .iter()
            .map(|member| member.urgency)
            .max()
            .unwrap_or(NotificationUrgency::Low);
        fold.due_at_unix_ms = fold
            .members
            .iter()
            .map(|member| member.due_at_unix_ms)
            .min()
            .unwrap_or(0);
        fold.source_cursor = fold
            .members
            .iter()
            .map(|member| member.source_cursor)
            .max()
            .unwrap_or(0);
        fold.critical_count = fold
            .members
            .iter()
            .filter(|member| member.urgency.is_critical())
            .count() as u32;
        fold.unknown_count = fold.members.iter().filter(|member| member.unknown).count() as u32;
        fold.unread_count = fold.members.iter().filter(|member| !member.read).count() as u32;
        fold.acknowledged_count = fold
            .members
            .iter()
            .filter(|member| member.acknowledged)
            .count() as u32;
        fold.fold_digest = fold.digest();
        fold.validate()?;
        Ok(fold)
    }

    pub fn validate(&self) -> Result<(), NotificationStoreError> {
        if self.schema != NOTIFICATION_DIGEST_FOLD_SCHEMA {
            return Err(NotificationStoreError::Invalid("digest_fold_schema"));
        }
        required(&self.recipient_id, "recipient_id", 256)?;
        required(
            &self.digest_group,
            "digest_group",
            NOTIFICATION_STORE_MAX_DIGEST_GROUP_BYTES,
        )?;
        if self.members.is_empty()
            || self.members.len() > NOTIFICATION_STORE_MAX_DIGEST_FOLD_MEMBERS
        {
            return Err(NotificationStoreError::Invalid("digest_fold_members"));
        }
        let mut ids = BTreeSet::new();
        let mut previous: Option<&str> = None;
        for member in &self.members {
            member.validate()?;
            if !ids.insert(member.item_id.as_str()) {
                return Err(NotificationStoreError::Invalid(
                    "digest_fold_member_duplicate",
                ));
            }
            if previous.is_some_and(|value| value >= member.item_id.as_str()) {
                return Err(NotificationStoreError::Invalid("digest_fold_member_order"));
            }
            previous = Some(member.item_id.as_str());
        }
        let urgency = self
            .members
            .iter()
            .map(|member| member.urgency)
            .max()
            .ok_or(NotificationStoreError::Invalid("digest_fold_members"))?;
        if self.urgency != urgency {
            return Err(NotificationStoreError::Invalid("digest_fold_urgency"));
        }
        if self.due_at_unix_ms
            != self
                .members
                .iter()
                .map(|member| member.due_at_unix_ms)
                .min()
                .unwrap_or(0)
        {
            return Err(NotificationStoreError::Invalid("digest_fold_due"));
        }
        if self.source_cursor
            != self
                .members
                .iter()
                .map(|member| member.source_cursor)
                .max()
                .unwrap_or(0)
        {
            return Err(NotificationStoreError::Invalid("digest_fold_cursor"));
        }
        if self.critical_count
            != self
                .members
                .iter()
                .filter(|member| member.urgency.is_critical())
                .count() as u32
        {
            return Err(NotificationStoreError::Invalid(
                "digest_fold_critical_count",
            ));
        }
        if self.unknown_count != self.members.iter().filter(|member| member.unknown).count() as u32
        {
            return Err(NotificationStoreError::Invalid("digest_fold_unknown_count"));
        }
        if self.unread_count != self.members.iter().filter(|member| !member.read).count() as u32 {
            return Err(NotificationStoreError::Invalid("digest_fold_unread_count"));
        }
        if self.acknowledged_count
            != self
                .members
                .iter()
                .filter(|member| member.acknowledged)
                .count() as u32
        {
            return Err(NotificationStoreError::Invalid(
                "digest_fold_acknowledged_count",
            ));
        }
        if self.fold_digest != self.digest() {
            return Err(NotificationStoreError::DigestMismatch);
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "recipient_id": self.recipient_id,
            "digest_group": self.digest_group,
            "urgency": self.urgency,
            "due_at_unix_ms": self.due_at_unix_ms,
            "source_cursor": self.source_cursor,
            "critical_count": self.critical_count,
            "unknown_count": self.unknown_count,
            "unread_count": self.unread_count,
            "acknowledged_count": self.acknowledged_count,
            "members": self.members,
        }))
    }
}

/// Order digest folds the same way the page orders items: urgency, then due, then source sequence,
/// then the group identity.
pub fn compare_notification_digest_folds(
    left: &NotificationDigestFold,
    right: &NotificationDigestFold,
) -> std::cmp::Ordering {
    right
        .urgency
        .rank()
        .cmp(&left.urgency.rank())
        .then_with(|| left.due_at_unix_ms.cmp(&right.due_at_unix_ms))
        .then_with(|| left.source_cursor.cmp(&right.source_cursor))
        .then_with(|| left.digest_group.cmp(&right.digest_group))
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationStoreError {
    #[error("notification_store_invalid:{0}")]
    Invalid(&'static str),
    #[error("notification_store_projection_unavailable")]
    ProjectionUnavailable,
    #[error("notification_store_scope_required")]
    ScopeRequired,
    #[error("notification_store_scope_conflict")]
    ScopeConflict,
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
    #[error("notification_store_server_time_required")]
    ServerTimeRequired,
    #[error("notification_store_snooze_not_hideable")]
    SnoozeNotHideable,
    #[error("notification_store_read_state:{0}")]
    ReadStateInvalid(String),
    #[error("notification_store_read_state_cursor_ahead")]
    ReadStateCursorAhead,
    #[error("notification_store_read_state_conflict")]
    ReadStateConflict,
    #[error("notification_store_read_state_regression")]
    ReadStateRegression,
    #[error("notification_store_materializer:{0}")]
    Materializer(String),
    #[error("notification_store_priority:{0}")]
    Priority(String),
}

impl From<crate::NotificationPriorityError> for NotificationStoreError {
    fn from(error: crate::NotificationPriorityError) -> Self {
        NotificationStoreError::Priority(error.to_string())
    }
}

/// Presentation state of one item.
///
/// `read`/`acknowledged` are booleans (not timestamps) so an append-only read-state fact can be
/// folded back exactly; `fenced_at_unix_ms` keeps the strict mutation clock fence of this process.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ProjectionState {
    read: bool,
    acknowledged: bool,
    snoozed_until_unix_ms: Option<u64>,
    revision: u64,
    fenced_at_unix_ms: Option<u64>,
}

/// In-process/in-app query projection. It is rebuildable from committed materializations and
/// intentionally does not pretend to be a durable NotificationStore.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotificationStore {
    materializer: NotificationMaterializer,
    /// Server-authenticated recipient bound by the composition root.  `None` means the store has
    /// not been scoped yet and must not answer a page or mutation.
    recipient_scope: Option<String>,
    /// Server clock bound by the composition root.  `None` means no due state can be classified
    /// and no summary can be produced; it is never taken from a client request.
    server_clock: Option<u64>,
    projection: BTreeMap<String, ProjectionState>,
}

impl NotificationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind the single server-authenticated recipient this store answers for.
    ///
    /// The binding is derived by the caller from the authenticated principal, never from a client
    /// parameter. Rebinding to the same recipient is idempotent; rebinding to a different
    /// recipient fails closed instead of silently switching or widening the scope.
    pub fn bind_server_recipient(
        &mut self,
        recipient_id: &str,
    ) -> Result<(), NotificationStoreError> {
        required(recipient_id, "recipient_id", 256)?;
        match self.recipient_scope.as_deref() {
            Some(bound) if bound != recipient_id => Err(NotificationStoreError::ScopeConflict),
            _ => {
                self.recipient_scope = Some(recipient_id.to_owned());
                Ok(())
            }
        }
    }

    pub fn server_recipient(&self) -> Option<&str> {
        self.recipient_scope.as_deref()
    }

    /// Bind the server clock this store classifies due state against.
    ///
    /// The reading comes from the composition root's clock, never from a request. It is
    /// monotonic: rebinding the same reading is idempotent and moving it backwards fails closed.
    pub fn bind_server_clock(&mut self, now_unix_ms: u64) -> Result<(), NotificationStoreError> {
        if now_unix_ms == 0 {
            return Err(NotificationStoreError::Invalid("server_time_unix_ms"));
        }
        match self.server_clock {
            Some(bound) if now_unix_ms < bound => Err(NotificationStoreError::ClockRegression),
            _ => {
                self.server_clock = Some(now_unix_ms);
                Ok(())
            }
        }
    }

    pub fn server_time_unix_ms(&self) -> Option<u64> {
        self.server_clock
    }

    /// A mutation timestamp may not precede the bound server clock: a client cannot retro-date a
    /// read/ACK/snooze and pull it out of the fence.
    fn fence_mutation_time(&self, now_unix_ms: u64) -> Result<(), NotificationStoreError> {
        if now_unix_ms == 0 {
            return Err(NotificationStoreError::Invalid("now_unix_ms"));
        }
        if self.server_clock.is_some_and(|bound| now_unix_ms < bound) {
            return Err(NotificationStoreError::ClockRegression);
        }
        Ok(())
    }

    /// A mutation may not move an item's own presentation clock backwards either, whatever the
    /// bound server clock says.
    fn fence_item_mutation_time(
        &self,
        item_id: &str,
        now_unix_ms: u64,
    ) -> Result<(), NotificationStoreError> {
        if self
            .projection
            .get(item_id)
            .and_then(|state| state.fenced_at_unix_ms)
            .is_some_and(|previous| now_unix_ms < previous)
        {
            return Err(NotificationStoreError::ClockRegression);
        }
        Ok(())
    }

    fn require_scope(&self, recipient_id: &str) -> Result<(), NotificationStoreError> {
        match self.recipient_scope.as_deref() {
            None => Err(NotificationStoreError::ScopeRequired),
            Some(bound) if bound != recipient_id => Err(NotificationStoreError::RecipientMismatch),
            Some(_) => Ok(()),
        }
    }

    fn require_recipient(&self) -> Result<String, NotificationStoreError> {
        self.recipient_scope
            .clone()
            .ok_or(NotificationStoreError::ScopeRequired)
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

    /// Number of committed source events retained by the projection.
    ///
    /// Applying a committed page can only append to this projection, and read/ACK/snooze mutations
    /// never remove a retained source event: the count never shrinks.
    pub fn retained_source_event_count(&self) -> usize {
        self.materializer.source_event_ids().count()
    }

    /// The bound recipient's items with their server-derived priority, in the deterministic inbox
    /// order. Read/ACK/snooze state never takes part in this order.
    fn prioritized_items(
        &self,
        recipient_id: &str,
    ) -> Result<Vec<(HumanInboxItem, NotificationPriority)>, NotificationStoreError> {
        let items = self
            .materializer
            .human_inbox_items(recipient_id)
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
        Ok(prioritized)
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
        self.require_scope(&request.recipient_id)?;
        if let Some(expected) = request.expected_source_cursor {
            if expected < source_cursor {
                return Err(NotificationStoreError::CursorStale);
            }
            if expected > source_cursor {
                return Err(NotificationStoreError::ProjectionUnavailable);
            }
        }
        let prioritized = self.prioritized_items(&request.recipient_id)?;
        let start = match request.after_item_id.as_deref() {
            None => 0,
            Some(after) => prioritized
                .iter()
                .position(|(item, _)| item.item_id == after)
                .map(|index| index + 1)
                .ok_or(NotificationStoreError::CursorInvalid)?,
        };
        let server_time = self.server_clock;
        let page_items = prioritized
            .into_iter()
            .skip(start)
            .take(usize::from(request.limit))
            .map(|(item, priority)| {
                let state = self.projection.get(&item.item_id);
                let snoozed_until_unix_ms = state.and_then(|value| value.snoozed_until_unix_ms);
                let snooze_active = match (server_time, snoozed_until_unix_ms) {
                    (Some(server_time), Some(until)) => server_time < until,
                    (None, Some(_)) => true,
                    (_, None) => false,
                };
                let due_state = server_time
                    .map(|server_time| notification_due_state(priority.due_at_unix_ms, server_time))
                    .transpose()?;
                let priority_digest = priority.priority_digest();
                Ok(NotificationPageItem {
                    urgency: priority.urgency,
                    due_at_unix_ms: priority.due_at_unix_ms,
                    source_cursor: priority.source_cursor,
                    digest_group: priority.digest_group,
                    read: state.is_some_and(|value| value.read),
                    acknowledged: state.is_some_and(|value| value.acknowledged),
                    snoozed_until_unix_ms,
                    snooze_active,
                    unknown: item_is_unknown(&item),
                    due_state,
                    priority_digest,
                    item,
                })
            })
            .collect::<Result<Vec<_>, NotificationStoreError>>()?;
        let next_after_item_id = if page_items.len() == usize::from(request.limit) {
            page_items.last().map(|item| item.item.item_id.clone())
        } else {
            None
        };
        NotificationPage::new(source_cursor, server_time, page_items, next_after_item_id)
    }

    /// Unread/read/ACK/snooze/digest counts for the bound recipient at the bound server time.
    pub fn inbox_summary(&self) -> Result<NotificationInboxSummary, NotificationStoreError> {
        let recipient_id = self.require_recipient()?;
        let server_time = self
            .server_clock
            .ok_or(NotificationStoreError::ServerTimeRequired)?;
        let source_cursor = self.source_cursor();
        if source_cursor == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        let mut counts = NotificationInboxCounts::default();
        let mut groups = BTreeSet::new();
        for (item, priority) in self.prioritized_items(&recipient_id)? {
            let state = self.projection.get(&item.item_id);
            let read = state.is_some_and(|value| value.read);
            let acknowledged = state.is_some_and(|value| value.acknowledged);
            let snoozed = state
                .and_then(|value| value.snoozed_until_unix_ms)
                .is_some_and(|until| server_time < until);
            let unknown = item_is_unknown(&item);
            counts.total = counts.total.saturating_add(1);
            if read {
                counts.read = counts.read.saturating_add(1);
            } else {
                counts.unread = counts.unread.saturating_add(1);
            }
            if acknowledged {
                counts.acknowledged = counts.acknowledged.saturating_add(1);
            }
            if snoozed {
                counts.snoozed = counts.snoozed.saturating_add(1);
            }
            if priority.urgency.is_critical() {
                counts.critical_total = counts.critical_total.saturating_add(1);
                if !read {
                    counts.critical_unread = counts.critical_unread.saturating_add(1);
                }
            }
            if unknown {
                counts.unknown_total = counts.unknown_total.saturating_add(1);
            }
            match notification_due_state(priority.due_at_unix_ms, server_time)? {
                NotificationDueState::Overdue => {
                    counts.overdue = counts.overdue.saturating_add(1);
                }
                NotificationDueState::Due => {
                    counts.due_now = counts.due_now.saturating_add(1);
                }
                NotificationDueState::Scheduled => {
                    counts.scheduled = counts.scheduled.saturating_add(1);
                }
            }
            groups.insert(priority.digest_group);
        }
        let digest_group_count = u32::try_from(groups.len())
            .map_err(|_| NotificationStoreError::Invalid("summary_digest_groups"))?;
        NotificationInboxSummary::new(
            recipient_id,
            source_cursor,
            server_time,
            counts,
            digest_group_count,
        )
    }

    /// Digest folds of the bound recipient in deterministic fold order.
    ///
    /// Folds only ever contain this recipient's items: a fold is never assembled across scopes,
    /// and it repeats the digest group identity plus the source sequence of every member.
    pub fn digest_folds(&self) -> Result<Vec<NotificationDigestFold>, NotificationStoreError> {
        let recipient_id = self.require_recipient()?;
        if self.source_cursor() == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        let mut by_group: BTreeMap<String, Vec<NotificationDigestMember>> = BTreeMap::new();
        for (item, priority) in self.prioritized_items(&recipient_id)? {
            let state = self.projection.get(&item.item_id);
            by_group
                .entry(priority.digest_group)
                .or_default()
                .push(NotificationDigestMember {
                    item_id: item.item_id.clone(),
                    urgency: priority.urgency,
                    due_at_unix_ms: priority.due_at_unix_ms,
                    source_cursor: priority.source_cursor,
                    unknown: item_is_unknown(&item),
                    read: state.is_some_and(|value| value.read),
                    acknowledged: state.is_some_and(|value| value.acknowledged),
                });
        }
        let mut folds = by_group
            .into_iter()
            .map(|(digest_group, members)| {
                NotificationDigestFold::new(recipient_id.clone(), digest_group, members)
            })
            .collect::<Result<Vec<_>, NotificationStoreError>>()?;
        folds.sort_by(compare_notification_digest_folds);
        Ok(folds)
    }

    /// Export the presentation state as append-only facts bound to the committed source event of
    /// each item, so a fresh process can fold the same read/ACK/snooze state back.
    ///
    /// The committed event id always comes from the materialization, never from the receipt: a
    /// receipt cannot name the fact that commits it.
    pub fn read_state_facts(
        &self,
    ) -> Result<Vec<NotificationReadStateFact>, NotificationStoreError> {
        self.require_recipient()?;
        if self.source_cursor() == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        let mut bindings = BTreeMap::new();
        for materialization in self.materializer.materializations() {
            bindings.insert(
                format!(
                    "notification:{}",
                    materialization.notification.notification_id
                ),
                (
                    materialization.source_event_id,
                    materialization.source_cursor,
                ),
            );
        }
        let mut facts = Vec::with_capacity(self.projection.len());
        for (item_id, state) in &self.projection {
            let (source_event_id, source_cursor) = bindings
                .get(item_id)
                .copied()
                .ok_or(NotificationStoreError::Invalid("read_state_binding"))?;
            facts.push(
                NotificationReadStateFact::new(
                    item_id.clone(),
                    source_event_id,
                    source_cursor,
                    state.revision,
                    state.read,
                    state.acknowledged,
                    state.snoozed_until_unix_ms,
                )
                .map_err(NotificationStoreError::ReadStateInvalid)?,
            );
        }
        Ok(facts)
    }

    /// Fold append-only read-state facts back into the projection.
    ///
    /// A fact may not run ahead of the committed cursor, may not name an item this store does not
    /// hold for the bound recipient, and may never un-read, un-acknowledge or move a snooze
    /// backwards. An exact replay of the current state is idempotent.
    pub fn apply_read_state_facts(
        &mut self,
        facts: &[NotificationReadStateFact],
    ) -> Result<(), NotificationStoreError> {
        let recipient_id = self.require_recipient()?;
        let source_cursor = self.source_cursor();
        if source_cursor == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        let known = self
            .materializer
            .human_inbox_items(&recipient_id)
            .map_err(NotificationStoreError::Materializer)?
            .into_iter()
            .map(|item| item.item_id)
            .collect::<BTreeSet<_>>();
        let mut next = self.projection.clone();
        for fact in facts {
            fact.validate()
                .map_err(NotificationStoreError::ReadStateInvalid)?;
            if fact.source_cursor > source_cursor {
                return Err(NotificationStoreError::ReadStateCursorAhead);
            }
            if !known.contains(&fact.item_id) {
                return Err(NotificationStoreError::ItemNotFound);
            }
            let previous = next.get(&fact.item_id).cloned();
            if let Some(previous) = &previous {
                let unchanged = previous.read == fact.read
                    && previous.acknowledged == fact.acknowledged
                    && previous.snoozed_until_unix_ms == fact.snoozed_until_unix_ms;
                if unchanged && previous.revision == fact.projection_revision {
                    continue;
                }
                if fact.projection_revision <= previous.revision {
                    return Err(NotificationStoreError::ReadStateConflict);
                }
                if (previous.read && !fact.read)
                    || (previous.acknowledged && !fact.acknowledged)
                    || match (previous.snoozed_until_unix_ms, fact.snoozed_until_unix_ms) {
                        (Some(previous_snooze), Some(next_snooze)) => next_snooze < previous_snooze,
                        (Some(_), None) => true,
                        _ => false,
                    }
                {
                    return Err(NotificationStoreError::ReadStateRegression);
                }
            }
            next.insert(
                fact.item_id.clone(),
                ProjectionState {
                    read: fact.read,
                    acknowledged: fact.acknowledged,
                    snoozed_until_unix_ms: fact.snoozed_until_unix_ms,
                    revision: fact.projection_revision,
                    fenced_at_unix_ms: previous.and_then(|state| state.fenced_at_unix_ms),
                },
            );
        }
        self.projection = next;
        Ok(())
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
        required(item_id, "item_id", NOTIFICATION_STORE_MAX_TEXT_BYTES)?;
        if now_unix_ms == 0 || snoozed_until_unix_ms <= now_unix_ms {
            return Err(NotificationStoreError::Invalid("snooze_window"));
        }
        if self.source_cursor() == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        self.require_scope(recipient_id)?;
        self.fence_mutation_time(now_unix_ms)?;
        self.fence_item_mutation_time(item_id, now_unix_ms)?;
        if self
            .server_clock
            .is_some_and(|bound| snoozed_until_unix_ms <= bound)
        {
            // A snooze that ends at or before the server clock would silence an item that is due
            // now; the client clock cannot shorten it.
            return Err(NotificationStoreError::Invalid("snooze_window"));
        }
        if snoozed_until_unix_ms.saturating_sub(now_unix_ms) > NOTIFICATION_STORE_MAX_SNOOZE_MS {
            return Err(NotificationStoreError::Invalid("snooze_horizon"));
        }
        let item = self
            .materializer
            .human_inbox_items(recipient_id)
            .map_err(NotificationStoreError::Materializer)?
            .into_iter()
            .find(|item| item.item_id == item_id)
            .ok_or(NotificationStoreError::ItemNotFound)?;
        let priority = classify_notification(&item)
            .map_err(|error| NotificationStoreError::Priority(error.to_string()))?;
        // Silence may not hide an item whose outcome is critical or still unknown. Both stay
        // queryable and un-snoozed; the refusal is the same for a client that only wanted to
        // "remind later".
        if priority.urgency.is_critical() || item_is_unknown(&item) {
            return Err(NotificationStoreError::SnoozeNotHideable);
        }
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
            state.fenced_at_unix_ms = Some(
                state
                    .fenced_at_unix_ms
                    .map_or(now_unix_ms, |previous| previous.max(now_unix_ms)),
            );
            if changed {
                state.revision = state.revision.saturating_add(1);
            }
            (state.revision, state.read, state.acknowledged, changed)
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
        required(item_id, "item_id", NOTIFICATION_STORE_MAX_TEXT_BYTES)?;
        if self.source_cursor() == 0 {
            return Err(NotificationStoreError::ProjectionUnavailable);
        }
        self.require_scope(recipient_id)?;
        self.fence_mutation_time(now_unix_ms)?;
        self.fence_item_mutation_time(item_id, now_unix_ms)?;
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
            let changed = if acknowledge {
                let mut changed = !state.acknowledged;
                state.acknowledged = true;
                // An acknowledgement is also a read of the presentation row; it must never be
                // confused with an approval, which only the original task owner can make.
                if !state.read {
                    state.read = true;
                    changed = true;
                }
                changed
            } else {
                let changed = !state.read;
                state.read = true;
                changed
            };
            state.fenced_at_unix_ms = Some(
                state
                    .fenced_at_unix_ms
                    .map_or(now_unix_ms, |previous| previous.max(now_unix_ms)),
            );
            if changed {
                state.revision = state.revision.saturating_add(1);
            }
            (
                state.revision,
                state.read,
                state.acknowledged,
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
