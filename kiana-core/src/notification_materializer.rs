//! Committed notification facts projected into a display-only human inbox.
//!
//! This module is deliberately an in-memory projection.  It accepts only committed EventLog
//! events, keeps the event/cursor binding needed for deterministic replay, and returns the
//! existing `HumanInboxItem` DTO.  It does not own a task store, delivery worker, Broker or
//! provider.  A HumanTask bridge carries identity and action references; task status remains in
//! the source authority that created the bridge.

use kiana_domain::{
    validate_notification_runtime_event, EventId, HumanInboxItem, HumanInboxKind,
    NotificationEventSource, NotificationMaterialization, RuntimeEvent,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub const NOTIFICATION_MATERIALIZER: &str = "notification_materializer";
const MAX_NOTIFICATION_MATERIALIZER_EVENTS: usize = 4_096;

/// A bounded, rebuildable materializer for committed notification facts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotificationMaterializer {
    source_cursor: u64,
    /// Every committed source event is retained so a replay with the same cursor can be checked
    /// for exact idempotency, including events that are not notification facts.
    source_event_digests: BTreeMap<EventId, String>,
    materializations: BTreeMap<EventId, NotificationMaterialization>,
}

impl NotificationMaterializer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a committed EventLog page.  `source_cursor` is the cursor of the complete committed
    /// page, not a client supplied notification cursor.  The receiver is unchanged on every
    /// validation or cursor failure.
    pub fn apply_committed(
        &mut self,
        events: &[RuntimeEvent],
        source_cursor: u64,
    ) -> Result<(), String> {
        if source_cursor == 0 {
            return Err("notification_materializer_source_cursor_invalid".to_owned());
        }
        if source_cursor < self.source_cursor {
            return Err("notification_materializer_source_cursor_regressed".to_owned());
        }
        if source_cursor > self.source_cursor && events.is_empty() {
            return Err("notification_materializer_cursor_gap".to_owned());
        }

        let mut next_event_digests = self.source_event_digests.clone();
        let mut next_materializations = self.materializations.clone();
        let mut new_event_count = 0usize;
        let mut new_materialization_cursors = Vec::new();
        let mut explicit_cursors = Vec::new();
        let mut all_events_have_explicit_cursor = true;
        let mut all_events_have_materialization = true;

        for event in events {
            if event.event_id.as_uuid().is_nil() {
                return Err("notification_materializer_source_event_invalid".to_owned());
            }
            let event_value = serde_json::to_value(event)
                .map_err(|_| "notification_materializer_event_encode_failed".to_owned())?;
            let event_digest = kiana_domain::json_digest(&event_value);
            if let Some(previous_digest) = next_event_digests.get(&event.event_id) {
                if previous_digest != &event_digest {
                    return Err("notification_materializer_event_digest_conflict".to_owned());
                }
                // An exact source-event replay is allowed, but cannot advance a cursor by
                // itself.  Continue validating the event so a malformed replay cannot be used
                // to bypass the committed source checks.
            } else {
                next_event_digests.insert(event.event_id, event_digest);
                new_event_count = new_event_count.saturating_add(1);
            }

            let is_new_event = !self.source_event_digests.contains_key(&event.event_id);
            match event.data.get("source_cursor") {
                Some(value) => {
                    let cursor = value.as_u64().ok_or_else(|| {
                        "notification_materializer_source_cursor_invalid".to_owned()
                    })?;
                    if cursor == 0 || cursor > source_cursor {
                        return Err("notification_materializer_source_cursor_invalid".to_owned());
                    }
                    if is_new_event {
                        explicit_cursors.push(cursor);
                    }
                }
                None => all_events_have_explicit_cursor = false,
            }

            // The event registry is the trust boundary.  Unknown notification families and
            // model/UI declarations fail closed before their payload is decoded.
            let registered =
                validate_notification_runtime_event(event, NotificationEventSource::EventLog)?;
            let payload = event
                .data
                .get("notification_materialization")
                .or_else(|| event.data.get("materialization"));
            if event.data.get("source").and_then(Value::as_str) != Some("eventlog") {
                if payload.is_some() || registered.is_some() {
                    return Err("notification_materializer_committed_source_required".to_owned());
                }
                continue;
            }
            if payload.is_none() {
                all_events_have_materialization = false;
                if registered.is_some() {
                    return Err("notification_materializer_payload_missing".to_owned());
                }
                continue;
            }
            if registered.is_none() {
                return Err("notification_materializer_event_not_registered".to_owned());
            }
            let materialization: NotificationMaterialization =
                serde_json::from_value(payload.cloned().unwrap_or(Value::Null))
                    .map_err(|_| "notification_materializer_payload_invalid".to_owned())?;
            materialization.validate()?;
            if materialization.source_event_id != event.event_id {
                return Err("notification_materializer_source_event_mismatch".to_owned());
            }
            if materialization.source_cursor == 0 || materialization.source_cursor > source_cursor {
                return Err("notification_materializer_source_cursor_invalid".to_owned());
            }
            if let Some(event_cursor) = event.data.get("source_cursor").and_then(Value::as_u64) {
                if event_cursor != materialization.source_cursor {
                    return Err("notification_materializer_source_cursor_mismatch".to_owned());
                }
            }
            if materialization.source_cursor <= self.source_cursor
                && !self
                    .materializations
                    .contains_key(&materialization.source_event_id)
            {
                return Err("notification_materializer_source_cursor_stale".to_owned());
            }
            if let Some(previous) = next_materializations.get(&materialization.source_event_id) {
                if previous.materialization_digest != materialization.materialization_digest {
                    return Err("notification_materializer_digest_conflict".to_owned());
                }
            } else {
                new_materialization_cursors.push(materialization.source_cursor);
                next_materializations.insert(materialization.source_event_id, materialization);
            }
        }

        if next_event_digests.len() > MAX_NOTIFICATION_MATERIALIZER_EVENTS
            || next_materializations.len() > MAX_NOTIFICATION_MATERIALIZER_EVENTS
        {
            return Err("notification_materializer_limit".to_owned());
        }

        if source_cursor == self.source_cursor {
            if new_event_count != 0 {
                return Err("notification_materializer_cursor_conflict".to_owned());
            }
        } else if new_event_count == 0 {
            return Err("notification_materializer_cursor_gap".to_owned());
        }

        // When the caller supplies cursor metadata for the complete page, require a contiguous
        // committed range.  Pages containing unrelated facts may omit that optional metadata;
        // the materialization's own source cursor is still checked above.
        if source_cursor > self.source_cursor
            && all_events_have_explicit_cursor
            && !explicit_cursors.is_empty()
        {
            explicit_cursors.sort_unstable();
            explicit_cursors.dedup();
            let first = self.source_cursor.saturating_add(1);
            let expected_len = source_cursor.saturating_sub(self.source_cursor) as usize;
            if explicit_cursors.first().copied() != Some(first)
                || explicit_cursors.last().copied() != Some(source_cursor)
                || explicit_cursors.len() != expected_len
                || explicit_cursors
                    .windows(2)
                    .any(|pair| pair[1] != pair[0].saturating_add(1))
            {
                return Err("notification_materializer_cursor_gap".to_owned());
            }
        }
        if source_cursor > self.source_cursor && !new_materialization_cursors.is_empty() {
            new_materialization_cursors.sort_unstable();
            new_materialization_cursors.dedup();
            if new_materialization_cursors
                .iter()
                .any(|cursor| *cursor > source_cursor)
            {
                return Err("notification_materializer_source_cursor_invalid".to_owned());
            }
            if all_events_have_materialization {
                let first = self.source_cursor.saturating_add(1);
                let expected_len = source_cursor.saturating_sub(self.source_cursor) as usize;
                if new_materialization_cursors.first().copied() != Some(first)
                    || new_materialization_cursors.last().copied() != Some(source_cursor)
                    || new_materialization_cursors.len() != expected_len
                    || new_materialization_cursors
                        .windows(2)
                        .any(|pair| pair[1] != pair[0].saturating_add(1))
                {
                    return Err("notification_materializer_cursor_gap".to_owned());
                }
            }
        }

        self.source_cursor = source_cursor;
        self.source_event_digests = next_event_digests;
        self.materializations = next_materializations;
        Ok(())
    }

    /// Apply a committed page and return the server-scoped display projection in one pure call.
    pub fn materialize_committed(
        &mut self,
        events: &[RuntimeEvent],
        source_cursor: u64,
        server_recipient_id: &str,
    ) -> Result<Vec<HumanInboxItem>, String> {
        self.apply_committed(events, source_cursor)?;
        self.human_inbox_items(server_recipient_id)
    }

    /// Return only items assigned to the authenticated server recipient.  A mismatched recipient
    /// or decider is omitted, never widened from a client supplied field.
    pub fn human_inbox_items(
        &self,
        server_recipient_id: &str,
    ) -> Result<Vec<HumanInboxItem>, String> {
        if server_recipient_id.trim().is_empty() {
            return Err("notification_materializer_recipient_required".to_owned());
        }
        let mut items = self
            .materializations
            .values()
            .filter(|materialization| {
                materialization.notification.recipient_id == server_recipient_id
            })
            .filter(|materialization| {
                materialization
                    .human_task
                    .as_ref()
                    .map(|task| task.decider_principal_id == server_recipient_id)
                    .unwrap_or_else(|| !is_actionable(&materialization.kind))
            })
            .map(NotificationMaterialization::to_human_inbox_item)
            .collect::<Result<Vec<_>, _>>()?;
        items.sort_by(|left, right| {
            left.item_id
                .cmp(&right.item_id)
                .then_with(|| left.source_ref.cmp(&right.source_ref))
        });
        Ok(items)
    }

    pub fn source_cursor(&self) -> u64 {
        self.source_cursor
    }

    pub fn materializations(&self) -> impl Iterator<Item = &NotificationMaterialization> {
        self.materializations.values()
    }

    pub fn source_event_ids(&self) -> impl Iterator<Item = &EventId> {
        self.source_event_digests.keys()
    }
}

fn is_actionable(kind: &HumanInboxKind) -> bool {
    matches!(
        kind,
        HumanInboxKind::Approval
            | HumanInboxKind::Review
            | HumanInboxKind::Acceptance
            | HumanInboxKind::Incident
            | HumanInboxKind::Reconciliation
    )
}
