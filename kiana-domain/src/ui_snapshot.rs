//! Deterministic, owner-scoped UI snapshot projection and pagination.
//!
//! The projector consumes one EventStore read and emits a snapshot at that exact source cursor.
//! It never reads a transcript, filesystem projection or stream cache.  A lagging or unknown
//! read-model is an explicit error, and a page cursor cannot be reused with another epoch or
//! source cursor.

use crate::{json_digest, RuntimeEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const UI_SNAPSHOT_PROJECTOR_SCHEMA: &str = "kiana.ui-snapshot-projector.v1";
pub const UI_SNAPSHOT_PAGE_SCHEMA: &str = "kiana.ui-snapshot-page.v1";
pub const UI_SNAPSHOT_CURSOR_SCHEMA: &str = "kiana.ui-snapshot-cursor.v1";
pub const UI_SNAPSHOT_MAX_PAGE_SIZE: usize = 256;
pub const UI_SNAPSHOT_MAX_ENTRIES: usize = 4_096;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSnapshotEntryKind {
    Session,
    Run,
    Action,
    Artifact,
    Receipt,
}

impl UiSnapshotEntryKind {
    const fn rank(self) -> u8 {
        match self {
            Self::Session => 0,
            Self::Run => 1,
            Self::Action => 2,
            Self::Artifact => 3,
            Self::Receipt => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotEntry {
    pub kind: UiSnapshotEntryKind,
    pub id: String,
    pub owner_id: String,
    pub revision: u64,
    pub source_cursor: u64,
    pub payload: Value,
}

impl UiSnapshotEntry {
    fn validate(&self) -> Result<(), String> {
        required(&self.id, "ui_snapshot_entry_id", 512)?;
        required(&self.owner_id, "ui_snapshot_entry_owner", 256)?;
        if self.revision == 0 || self.source_cursor == 0 {
            return Err("ui_snapshot_entry_revision_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotPageCursor {
    pub schema: String,
    pub epoch: String,
    pub source_cursor: u64,
    pub offset: usize,
    pub revision: u64,
    pub cursor_digest: String,
}

impl UiSnapshotPageCursor {
    pub fn new(
        epoch: impl Into<String>,
        source_cursor: u64,
        offset: usize,
        revision: u64,
    ) -> Result<Self, String> {
        let mut cursor = Self {
            schema: UI_SNAPSHOT_CURSOR_SCHEMA.to_owned(),
            epoch: epoch.into(),
            source_cursor,
            offset,
            revision,
            cursor_digest: String::new(),
        };
        cursor.cursor_digest = cursor.digest();
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| "ui_snapshot_cursor_encode_failed".to_owned())
    }

    pub fn decode(value: &str) -> Result<Self, String> {
        if value.len() > 2_048 {
            return Err("ui_snapshot_cursor_too_large".to_owned());
        }
        let cursor: Self = serde_json::from_str(value)
            .map_err(|_| "ui_snapshot_cursor_decode_failed".to_owned())?;
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_CURSOR_SCHEMA
            || self.revision == 0
            || self.offset > UI_SNAPSHOT_MAX_ENTRIES
        {
            return Err("ui_snapshot_cursor_invalid".to_owned());
        }
        required(&self.epoch, "ui_snapshot_cursor_epoch", 256)?;
        digest(&self.cursor_digest, "ui_snapshot_cursor_digest")?;
        if self.cursor_digest != self.digest() {
            return Err("ui_snapshot_cursor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "epoch": self.epoch,
            "source_cursor": self.source_cursor,
            "offset": self.offset,
            "revision": self.revision,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotQuery {
    pub schema: String,
    pub owner_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub epoch: String,
    pub source_cursor: u64,
    #[serde(default)]
    pub projection_cursor: Option<u64>,
    pub projection_generation: u64,
    pub data_epoch: u64,
    pub retention_floor: u64,
    pub page_size: usize,
    #[serde(default)]
    pub after: Option<String>,
    pub generated_at_unix_ms: u64,
}

impl UiSnapshotQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner_id: impl Into<String>,
        session_id: Option<String>,
        epoch: impl Into<String>,
        source_cursor: u64,
        projection_cursor: Option<u64>,
        projection_generation: u64,
        data_epoch: u64,
        retention_floor: u64,
        page_size: usize,
        after: Option<String>,
        generated_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let query = Self {
            schema: UI_SNAPSHOT_PROJECTOR_SCHEMA.to_owned(),
            owner_id: owner_id.into(),
            session_id,
            epoch: epoch.into(),
            source_cursor,
            projection_cursor,
            projection_generation,
            data_epoch,
            retention_floor,
            page_size,
            after,
            generated_at_unix_ms,
        };
        query.validate()?;
        Ok(query)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_PROJECTOR_SCHEMA
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.retention_floor > self.source_cursor
            || self.page_size == 0
            || self.page_size > UI_SNAPSHOT_MAX_PAGE_SIZE
            || self.generated_at_unix_ms == 0
        {
            return Err("ui_snapshot_query_header_invalid".to_owned());
        }
        required(&self.owner_id, "ui_snapshot_query_owner", 256)?;
        required(&self.epoch, "ui_snapshot_query_epoch", 256)?;
        if let Some(session_id) = &self.session_id {
            required(session_id, "ui_snapshot_query_session", 256)?;
        }
        if self
            .projection_cursor
            .is_some_and(|cursor| cursor > self.source_cursor)
        {
            return Err("ui_snapshot_projection_cursor_ahead".to_owned());
        }
        if self.after.as_deref().is_some_and(str::is_empty) {
            return Err("ui_snapshot_after_cursor_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotPage {
    pub schema: String,
    pub epoch: String,
    pub snapshot_cursor: u64,
    pub owner_id: String,
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub projection_generation: u64,
    pub data_epoch: u64,
    pub retention_floor: u64,
    pub entries: Vec<UiSnapshotEntry>,
    #[serde(default)]
    pub next_page: Option<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub snapshot_digest: String,
}

impl UiSnapshotPage {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_PAGE_SCHEMA
            || self.snapshot_cursor != self.source_cursor
            || self.projection_cursor != self.source_cursor
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.retention_floor > self.source_cursor
            || self.entries.len() > UI_SNAPSHOT_MAX_PAGE_SIZE
            || self.limitations.len() > 32
        {
            return Err("ui_snapshot_page_header_invalid".to_owned());
        }
        required(&self.epoch, "ui_snapshot_page_epoch", 256)?;
        required(&self.owner_id, "ui_snapshot_page_owner", 256)?;
        for entry in &self.entries {
            entry.validate()?;
            if entry.owner_id != self.owner_id {
                return Err("ui_snapshot_page_owner_leak".to_owned());
            }
        }
        for limitation in &self.limitations {
            required(limitation, "ui_snapshot_limitation", 512)?;
        }
        digest(&self.snapshot_digest, "ui_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("ui_snapshot_digest_mismatch".to_owned());
        }
        if let Some(next) = &self.next_page {
            let cursor = UiSnapshotPageCursor::decode(next)?;
            if cursor.epoch != self.epoch
                || cursor.source_cursor != self.source_cursor
                || cursor.revision != self.projection_generation
            {
                return Err("ui_snapshot_next_cursor_mismatch".to_owned());
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "epoch": self.epoch,
            "snapshot_cursor": self.snapshot_cursor,
            "owner_id": self.owner_id,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "retention_floor": self.retention_floor,
            "entries": self.entries,
            "next_page": self.next_page,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectedItem {
    kind: UiSnapshotEntryKind,
    id: String,
    owner_id: String,
    revision: u64,
    source_cursor: u64,
    payload: Value,
}

fn event_owner(event: &RuntimeEvent) -> Option<&str> {
    event
        .data
        .get("owner_id")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("actor_id").and_then(Value::as_str))
        .or_else(|| {
            event
                .data
                .get("action")
                .and_then(|action| action.get("owner_id"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            event
                .data
                .get("record")
                .and_then(|record| record.get("owner_id"))
                .and_then(Value::as_str)
        })
}

fn event_session(event: &RuntimeEvent) -> Option<&str> {
    event
        .data
        .get("session_id")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("session").and_then(Value::as_str))
        .or_else(|| {
            event
                .data
                .get("action")
                .and_then(|action| action.get("session_id"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            event
                .data
                .get("record")
                .and_then(|record| record.get("session_id"))
                .and_then(Value::as_str)
        })
}

fn event_item(event: &RuntimeEvent, source_cursor: u64) -> Option<ProjectedItem> {
    let owner_id = event_owner(event)?.to_owned();
    let (kind, id) = if event.kind.starts_with("ui.action.") {
        let id = event
            .aggregate_id
            .clone()
            .or_else(|| event.data.get("action_id").and_then(Value::as_str).map(str::to_owned))?;
        (UiSnapshotEntryKind::Action, id)
    } else if let Some(id) = event
        .data
        .get("artifact_id")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("artifact_ref").and_then(Value::as_str))
    {
        (UiSnapshotEntryKind::Artifact, id.to_owned())
    } else if let Some(id) = event
        .data
        .get("receipt_id")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("receipt_digest").and_then(Value::as_str))
    {
        (UiSnapshotEntryKind::Receipt, id.to_owned())
    } else if let Some(id) = event.data.get("run_id").and_then(Value::as_str) {
        (UiSnapshotEntryKind::Run, id.to_owned())
    } else if let Some(id) = event.data.get("session_id").and_then(Value::as_str) {
        (UiSnapshotEntryKind::Session, id.to_owned())
    } else {
        return None;
    };
    Some(ProjectedItem {
        kind,
        id,
        owner_id,
        revision: event
            .stream_version
            .or(Some(event.sequence))
            .unwrap_or(source_cursor),
        source_cursor,
        payload: event.data.clone(),
    })
}

fn state_string(event: &RuntimeEvent) -> Option<&str> {
    event
        .data
        .get("state")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("record").and_then(|record| record.get("state")))
        .and_then(Value::as_str)
}

/// Build one atomic, owner-scoped snapshot from the supplied EventStore read.
pub fn project_ui_snapshot(
    events: &[RuntimeEvent],
    query: &UiSnapshotQuery,
) -> Result<UiSnapshotPage, String> {
    query.validate()?;
    if query.source_cursor != events.len() as u64 {
        return Err("ui_snapshot_source_cursor_mismatch".to_owned());
    }
    let projection_cursor = query
        .projection_cursor
        .ok_or_else(|| "ui_snapshot_projection_lag_unknown".to_owned())?;
    if projection_cursor < query.source_cursor {
        return Err("ui_snapshot_projection_lag".to_owned());
    }
    if projection_cursor > query.source_cursor {
        return Err("ui_snapshot_projection_cursor_ahead".to_owned());
    }
    let after = query
        .after
        .as_deref()
        .map(UiSnapshotPageCursor::decode)
        .transpose()?;
    let offset = if let Some(cursor) = &after {
        if cursor.epoch != query.epoch || cursor.source_cursor != query.source_cursor {
            return Err("ui_snapshot_cursor_source_mismatch".to_owned());
        }
        if cursor.revision != query.projection_generation {
            return Err("ui_snapshot_cursor_generation_mismatch".to_owned());
        }
        cursor.offset
    } else {
        0
    };

    let requested_session = query.session_id.as_deref();
    let mut session_owner_mismatch = false;
    let mut pending_before_retention = BTreeSet::new();
    let mut actions = BTreeMap::<String, ProjectedItem>::new();
    let mut items = BTreeMap::<(u8, String), ProjectedItem>::new();
    for (index, event) in events.iter().enumerate() {
        let source_cursor = index as u64 + 1;
        if let Some(session_id) = requested_session {
            if event_session(event) == Some(session_id)
                && event_owner(event).is_some_and(|owner| owner != query.owner_id)
            {
                session_owner_mismatch = true;
            }
        }
        if event.kind.starts_with("ui.action.") {
            let Some(action_id) = event
                .aggregate_id
                .clone()
                .or_else(|| event.data.get("action_id").and_then(Value::as_str).map(str::to_owned))
            else {
                continue;
            };
            if let Some(item) = event_item(event, source_cursor) {
                if item.owner_id == query.owner_id
                    && requested_session.is_none_or(|session| event_session(event) == Some(session))
                {
                    if state_string(event) == Some("accepted")
                        && source_cursor <= query.retention_floor
                    {
                        pending_before_retention.insert(action_id.clone());
                    }
                    if matches!(state_string(event), Some("applied" | "rejected" | "unknown")) {
                        pending_before_retention.remove(&action_id);
                    }
                    actions.insert(action_id, item);
                }
            }
            continue;
        }
        let Some(item) = event_item(event, source_cursor) else {
            continue;
        };
        if item.owner_id != query.owner_id
            || requested_session.is_some_and(|session| event_session(event) != Some(session))
        {
            continue;
        }
        items.insert((item.kind.rank(), item.id.clone()), item);
    }
    if session_owner_mismatch {
        return Err("ui_snapshot_session_owner_mismatch".to_owned());
    }
    if !pending_before_retention.is_empty() {
        return Err("ui_snapshot_retention_protected_pending".to_owned());
    }
    for action in actions.into_values() {
        items.insert((action.kind.rank(), action.id.clone()), action);
    }
    let mut all = items.into_values().collect::<Vec<_>>();
    if all.len() > UI_SNAPSHOT_MAX_ENTRIES {
        return Err("ui_snapshot_entry_limit".to_owned());
    }
    all.sort_by(|left, right| {
        (left.kind.rank(), &left.id, left.revision, left.source_cursor).cmp(&(
            right.kind.rank(),
            &right.id,
            right.revision,
            right.source_cursor,
        ))
    });
    if offset > all.len() {
        return Err("ui_snapshot_cursor_offset_invalid".to_owned());
    }
    let end = (offset + query.page_size).min(all.len());
    let entries = all[offset..end]
        .iter()
        .map(|item| UiSnapshotEntry {
            kind: item.kind,
            id: item.id.clone(),
            owner_id: item.owner_id.clone(),
            revision: item.revision,
            source_cursor: item.source_cursor,
            payload: item.payload.clone(),
        })
        .collect::<Vec<_>>();
    let next_page = if end < all.len() {
        Some(UiSnapshotPageCursor::new(
            query.epoch.clone(),
            query.source_cursor,
            end,
            query.projection_generation,
        )?
        .encode()?)
    } else {
        None
    };
    let mut page = UiSnapshotPage {
        schema: UI_SNAPSHOT_PAGE_SCHEMA.to_owned(),
        epoch: query.epoch.clone(),
        snapshot_cursor: query.source_cursor,
        owner_id: query.owner_id.clone(),
        source_cursor: query.source_cursor,
        projection_cursor,
        projection_generation: query.projection_generation,
        data_epoch: query.data_epoch,
        retention_floor: query.retention_floor,
        entries,
        next_page,
        limitations: Vec::new(),
        snapshot_digest: String::new(),
    };
    page.snapshot_digest = page.digest();
    page.validate()?;
    Ok(page)
}
