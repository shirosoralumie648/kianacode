//! Pure UI entity reducer and bounded store.
//!
//! `UiEntityStore` is a disposable projection for presenters.  It never authorizes an action,
//! executes a capability, or treats optimistic state as an EventLog fact.  The reducer consumes
//! typed snapshot/feed/action inputs and returns a new store, which makes replay and hydrate
//! deterministic and keeps every surface on one state path.

use kiana_domain::json_digest;
use kiana_protocol::{
    UiActionDisposition, UiActionResult, UiFeedFrameKind, UiFeedFrameV1, UiSnapshotV1,
    UI_FEED_FRAME_SCHEMA, UI_SNAPSHOT_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const UI_ENTITY_STORE_SCHEMA: &str = "kiana.ui-entity-store.v1";
pub const UI_ENTITY_STORE_SNAPSHOT_SCHEMA: &str = "kiana.ui-entity-store-snapshot.v1";
pub const UI_ENTITY_STORE_MAX_ENTITIES: usize = 4_096;
pub const UI_ENTITY_STORE_MAX_EVENTS: usize = 8_192;
pub const UI_ENTITY_STORE_MAX_UNKNOWN: usize = 512;

fn required(value: &str, field: &str, max: usize) -> Result<(), UiStoreError> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(UiStoreError::Invalid(format!("{field}_invalid")));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), UiStoreError> {
    let valid = value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if valid {
        Ok(())
    } else {
        Err(UiStoreError::Invalid(format!("{field}_invalid")))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiEntityKind {
    Run,
    Connection,
    Submission,
    Draft,
    Inbox,
    Artifact,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEntityKey {
    pub kind: UiEntityKind,
    pub id: String,
}

impl UiEntityKey {
    pub fn new(kind: UiEntityKind, id: impl Into<String>) -> Result<Self, UiStoreError> {
        let key = Self {
            kind,
            id: id.into(),
        };
        required(&key.id, "ui_entity_id", 512)?;
        Ok(key)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiEntityLifecycle {
    Authoritative,
    OptimisticPending,
    Unknown,
}

impl UiEntityLifecycle {
    fn protected(self) -> bool {
        matches!(self, Self::OptimisticPending | Self::Unknown)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEntity {
    pub key: UiEntityKey,
    pub workspace: String,
    pub session_id: String,
    pub tab_id: String,
    pub epoch: String,
    pub revision: u64,
    pub lifecycle: UiEntityLifecycle,
    pub value: Value,
    #[serde(default)]
    pub source_event_id: Option<String>,
}

impl UiEntity {
    pub fn validate(&self) -> Result<(), UiStoreError> {
        required(&self.workspace, "ui_entity_workspace", 512)?;
        required(&self.session_id, "ui_entity_session", 256)?;
        required(&self.tab_id, "ui_entity_tab", 256)?;
        required(&self.epoch, "ui_entity_epoch", 256)?;
        if self.revision == 0 {
            return Err(UiStoreError::Invalid(
                "ui_entity_revision_invalid".to_owned(),
            ));
        }
        self.key.clone().validate()?;
        if self
            .source_event_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 512)
        {
            return Err(UiStoreError::Invalid(
                "ui_entity_event_id_invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

impl UiEntityKey {
    fn validate(&self) -> Result<(), UiStoreError> {
        required(&self.id, "ui_entity_id", 512)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiStoreScope {
    pub workspace: String,
    pub session_id: String,
    pub tab_id: String,
}

impl UiStoreScope {
    pub fn new(
        workspace: impl Into<String>,
        session_id: impl Into<String>,
        tab_id: impl Into<String>,
    ) -> Result<Self, UiStoreError> {
        let scope = Self {
            workspace: workspace.into(),
            session_id: session_id.into(),
            tab_id: tab_id.into(),
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), UiStoreError> {
        required(&self.workspace, "ui_store_workspace", 512)?;
        required(&self.session_id, "ui_store_session", 256)?;
        required(&self.tab_id, "ui_store_tab", 256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiStoreGapReason {
    SequenceGap,
    FeedGap,
    EpochReset,
    SnapshotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiStoreGap {
    pub reason: UiStoreGapReason,
    pub expected_sequence: u64,
    pub observed_sequence: u64,
    pub epoch: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiOptimisticUpdate {
    pub idempotency_key: String,
    pub command_id: kiana_protocol::RequestId,
    pub key: UiEntityKey,
    pub revision: u64,
    pub value: Value,
    pub expires_at_unix_ms: u64,
    #[serde(default)]
    pub previous: Option<UiEntity>,
}

impl UiOptimisticUpdate {
    pub fn validate(&self) -> Result<(), UiStoreError> {
        required(&self.idempotency_key, "ui_optimistic_idempotency_key", 256)?;
        if self.command_id.as_uuid().is_nil() || self.revision == 0 || self.expires_at_unix_ms == 0
        {
            return Err(UiStoreError::Invalid(
                "ui_optimistic_header_invalid".to_owned(),
            ));
        }
        self.key.validate()?;
        if let Some(previous) = &self.previous {
            previous.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum UiStoreEvent {
    Snapshot {
        scope: UiStoreScope,
        snapshot: UiSnapshotV1,
    },
    Feed {
        scope: UiStoreScope,
        frame: UiFeedFrameV1,
    },
    Optimistic(UiOptimisticUpdate),
    ActionResult {
        scope: UiStoreScope,
        idempotency_key: String,
        result: UiActionResult,
    },
    ExpireOptimistic {
        scope: UiStoreScope,
        now_unix_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiStoreChange {
    SnapshotHydrated,
    EntityInserted,
    EntityUpdated,
    DuplicateEvent,
    DuplicateFrame,
    OptimisticPending,
    OptimisticRolledBack,
    ActionAccepted,
    ActionApplied,
    ActionRejected,
    ActionUnknown,
    ExpiredOptimistic,
    SnapshotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum UiStoreError {
    #[error("ui_store_invalid:{0}")]
    Invalid(String),
    #[error("ui_store_scope_mismatch")]
    ScopeMismatch,
    #[error("ui_store_stale_revision:{0}")]
    StaleRevision(String),
    #[error("ui_store_duplicate_event:{0}")]
    DuplicateEvent(String),
    #[error("ui_store_duplicate_frame:{0}")]
    DuplicateFrame(String),
    #[error("ui_store_epoch_mismatch")]
    EpochMismatch,
    #[error("ui_store_gap_required")]
    GapRequired,
    #[error("ui_store_sequence_gap")]
    SequenceGap,
    #[error("ui_store_cache_full_protected")]
    CacheFullProtected,
    #[error("ui_store_unknown_optimistic:{0}")]
    UnknownOptimistic(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEntityStoreSnapshot {
    pub schema: String,
    pub scope: UiStoreScope,
    pub epoch: String,
    pub feed_sequence: u64,
    pub entities: Vec<UiEntity>,
    #[serde(default)]
    pub optimistic: Vec<UiOptimisticUpdate>,
    pub seen_event_ids: Vec<String>,
    pub unknown_commands: Vec<String>,
    pub needs_snapshot: bool,
    pub max_entities: usize,
    pub snapshot_digest: String,
}

impl UiEntityStoreSnapshot {
    pub fn validate(&self) -> Result<(), UiStoreError> {
        if self.schema != UI_ENTITY_STORE_SNAPSHOT_SCHEMA
            || self.max_entities == 0
            || self.max_entities > UI_ENTITY_STORE_MAX_ENTITIES
            || self.entities.len() > self.max_entities
            || self.optimistic.len() > self.max_entities
            || self.seen_event_ids.len() > UI_ENTITY_STORE_MAX_EVENTS
            || self.unknown_commands.len() > UI_ENTITY_STORE_MAX_UNKNOWN
        {
            return Err(UiStoreError::Invalid(
                "ui_store_snapshot_header_invalid".to_owned(),
            ));
        }
        self.scope.validate()?;
        required(&self.epoch, "ui_store_snapshot_epoch", 256)?;
        let mut keys = BTreeSet::new();
        for entity in &self.entities {
            entity.validate()?;
            if entity.workspace != self.scope.workspace
                || entity.session_id != self.scope.session_id
                || entity.tab_id != self.scope.tab_id
                || !keys.insert(entity.key.clone())
            {
                return Err(UiStoreError::ScopeMismatch);
            }
        }
        let mut optimistic_keys = BTreeSet::new();
        for update in &self.optimistic {
            update.validate()?;
            if !optimistic_keys.insert(update.idempotency_key.clone()) {
                return Err(UiStoreError::Invalid(
                    "ui_store_optimistic_duplicate".to_owned(),
                ));
            }
            let entity = self
                .entities
                .iter()
                .find(|entity| entity.key == update.key)
                .ok_or_else(|| {
                    UiStoreError::Invalid("ui_store_optimistic_entity_missing".to_owned())
                })?;
            if entity.lifecycle != UiEntityLifecycle::OptimisticPending
                || entity.revision != update.revision
                || entity.workspace != self.scope.workspace
                || entity.session_id != self.scope.session_id
                || entity.tab_id != self.scope.tab_id
                || entity.epoch != self.epoch
            {
                return Err(UiStoreError::Invalid(
                    "ui_store_optimistic_entity_mismatch".to_owned(),
                ));
            }
            if let Some(previous) = &update.previous {
                if previous.key != update.key
                    || previous.workspace != self.scope.workspace
                    || previous.session_id != self.scope.session_id
                    || previous.tab_id != self.scope.tab_id
                    || previous.epoch != self.epoch
                {
                    return Err(UiStoreError::ScopeMismatch);
                }
            }
        }
        for event_id in &self.seen_event_ids {
            required(event_id, "ui_store_event_id", 512)?;
        }
        for command in &self.unknown_commands {
            required(command, "ui_store_unknown_command", 256)?;
        }
        digest(&self.snapshot_digest, "ui_store_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err(UiStoreError::Invalid(
                "ui_store_snapshot_digest_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "scope": self.scope,
            "epoch": self.epoch,
            "feed_sequence": self.feed_sequence,
            "entities": self.entities,
            "optimistic": self.optimistic,
            "seen_event_ids": self.seen_event_ids,
            "unknown_commands": self.unknown_commands,
            "needs_snapshot": self.needs_snapshot,
            "max_entities": self.max_entities,
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiStoreTransition {
    pub store: UiEntityStore,
    pub change: UiStoreChange,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiEntityStore {
    scope: UiStoreScope,
    epoch: String,
    feed_sequence: u64,
    entities: BTreeMap<UiEntityKey, UiEntity>,
    seen_event_ids: BTreeSet<String>,
    optimistic: BTreeMap<String, UiOptimisticUpdate>,
    unknown_commands: BTreeSet<String>,
    needs_snapshot: bool,
    gap: Option<UiStoreGap>,
    max_entities: usize,
}

impl UiEntityStore {
    pub fn new(scope: UiStoreScope, max_entities: usize) -> Result<Self, UiStoreError> {
        scope.validate()?;
        if max_entities == 0 || max_entities > UI_ENTITY_STORE_MAX_ENTITIES {
            return Err(UiStoreError::Invalid(
                "ui_store_capacity_invalid".to_owned(),
            ));
        }
        Ok(Self {
            scope,
            epoch: String::new(),
            feed_sequence: 0,
            entities: BTreeMap::new(),
            seen_event_ids: BTreeSet::new(),
            optimistic: BTreeMap::new(),
            unknown_commands: BTreeSet::new(),
            needs_snapshot: true,
            gap: None,
            max_entities,
        })
    }

    pub fn scope(&self) -> &UiStoreScope {
        &self.scope
    }

    pub fn epoch(&self) -> &str {
        &self.epoch
    }

    pub fn feed_sequence(&self) -> u64 {
        self.feed_sequence
    }

    pub fn needs_snapshot(&self) -> bool {
        self.needs_snapshot
    }

    pub fn gap(&self) -> Option<&UiStoreGap> {
        self.gap.as_ref()
    }

    pub fn entity(&self, key: &UiEntityKey) -> Option<&UiEntity> {
        self.entities.get(key)
    }

    pub fn entities(&self) -> impl Iterator<Item = &UiEntity> {
        self.entities.values()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn pending_count(&self) -> usize {
        self.entities
            .values()
            .filter(|entity| entity.lifecycle == UiEntityLifecycle::OptimisticPending)
            .count()
    }

    pub fn unknown_count(&self) -> usize {
        self.unknown_commands.len()
    }

    /// Apply an event by replacing the state with the deterministic reducer output.
    pub fn apply(&mut self, event: UiStoreEvent) -> Result<UiStoreChange, UiStoreError> {
        let transition = self.reduce(event)?;
        *self = transition.store;
        Ok(transition.change)
    }

    /// Pure reducer: the input store is never mutated.
    pub fn reduce(&self, event: UiStoreEvent) -> Result<UiStoreTransition, UiStoreError> {
        match event {
            UiStoreEvent::Snapshot { scope, snapshot } => self.reduce_snapshot(scope, snapshot),
            UiStoreEvent::Feed { scope, frame } => self.reduce_feed(scope, frame),
            UiStoreEvent::Optimistic(update) => self.reduce_optimistic(update),
            UiStoreEvent::ActionResult {
                scope,
                idempotency_key,
                result,
            } => self.reduce_action_result(scope, idempotency_key, result),
            UiStoreEvent::ExpireOptimistic { scope, now_unix_ms } => {
                self.reduce_expiry(scope, now_unix_ms)
            }
        }
    }

    pub fn dehydrate(&self) -> Result<UiEntityStoreSnapshot, UiStoreError> {
        if self.epoch.is_empty() {
            return Err(UiStoreError::Invalid("ui_store_epoch_missing".to_owned()));
        }
        let mut snapshot = UiEntityStoreSnapshot {
            schema: UI_ENTITY_STORE_SNAPSHOT_SCHEMA.to_owned(),
            scope: self.scope.clone(),
            epoch: self.epoch.clone(),
            feed_sequence: self.feed_sequence,
            entities: self.entities.values().cloned().collect(),
            optimistic: self.optimistic.values().cloned().collect(),
            seen_event_ids: self.seen_event_ids.iter().cloned().collect(),
            unknown_commands: self.unknown_commands.iter().cloned().collect(),
            needs_snapshot: self.needs_snapshot,
            max_entities: self.max_entities,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn hydrate(snapshot: UiEntityStoreSnapshot) -> Result<Self, UiStoreError> {
        snapshot.validate()?;
        let UiEntityStoreSnapshot {
            scope,
            epoch,
            feed_sequence,
            entities: snapshot_entities,
            optimistic: snapshot_optimistic,
            seen_event_ids,
            unknown_commands,
            needs_snapshot,
            max_entities,
            ..
        } = snapshot;
        let entities = snapshot_entities
            .into_iter()
            .map(|entity| (entity.key.clone(), entity))
            .collect();
        Ok(Self {
            scope,
            epoch,
            feed_sequence,
            entities,
            seen_event_ids: seen_event_ids.into_iter().collect(),
            optimistic: snapshot_optimistic
                .into_iter()
                .map(|update| (update.idempotency_key.clone(), update))
                .collect(),
            unknown_commands: unknown_commands.into_iter().collect(),
            needs_snapshot,
            gap: None,
            max_entities,
        })
    }

    fn check_scope(&self, scope: &UiStoreScope) -> Result<(), UiStoreError> {
        scope.validate()?;
        if scope != &self.scope {
            return Err(UiStoreError::ScopeMismatch);
        }
        Ok(())
    }

    fn reduce_snapshot(
        &self,
        scope: UiStoreScope,
        snapshot: UiSnapshotV1,
    ) -> Result<UiStoreTransition, UiStoreError> {
        self.check_scope(&scope)?;
        if snapshot.schema != UI_SNAPSHOT_SCHEMA {
            return Err(UiStoreError::Invalid(
                "ui_snapshot_schema_invalid".to_owned(),
            ));
        }
        snapshot.validate().map_err(UiStoreError::Invalid)?;
        if !snapshot.sessions.is_empty()
            && !snapshot
                .sessions
                .iter()
                .any(|session| session.session_id.to_string() == self.scope.session_id)
        {
            return Err(UiStoreError::ScopeMismatch);
        }
        let next_epoch = snapshot.snapshot_cursor.epoch.clone();
        let epoch_reset = !self.epoch.is_empty() && self.epoch != next_epoch;
        let mut next = if epoch_reset {
            self.preserve_local_state(next_epoch.clone())?
        } else {
            self.clone()
        };
        next.epoch = next_epoch.clone();
        next.feed_sequence = 0;
        next.needs_snapshot = false;
        next.gap = None;
        let mut revision = snapshot.snapshot_cursor.sequence.max(1);
        for session in &snapshot.sessions {
            let key = UiEntityKey::new(UiEntityKind::Connection, session.session_id.to_string())?;
            let value = serde_json::to_value(session)
                .map_err(|error| UiStoreError::Invalid(format!("ui_session_encode:{error}")))?;
            next.merge_snapshot_entity(key, revision, value)?;
            revision = revision.saturating_add(1);
        }
        for run in &snapshot.runs {
            let key = UiEntityKey::new(UiEntityKind::Run, run.run_id.to_string())?;
            let value = serde_json::to_value(run)
                .map_err(|error| UiStoreError::Invalid(format!("ui_run_encode:{error}")))?;
            next.merge_snapshot_entity(key, run.revision, value)?;
        }
        for action in &snapshot.pending_actions {
            let key = UiEntityKey::new(UiEntityKind::Inbox, action.action_id.clone())?;
            if next
                .entities
                .get(&key)
                .is_some_and(|entity| entity.lifecycle.protected())
            {
                continue;
            }
            let value = serde_json::to_value(action)
                .map_err(|error| UiStoreError::Invalid(format!("ui_action_encode:{error}")))?;
            let entity = UiEntity {
                key: key.clone(),
                workspace: next.scope.workspace.clone(),
                session_id: next.scope.session_id.clone(),
                tab_id: next.scope.tab_id.clone(),
                epoch: next.epoch.clone(),
                revision: action.expected_revision.unwrap_or(revision).max(1),
                lifecycle: UiEntityLifecycle::OptimisticPending,
                value,
                source_event_id: None,
            };
            next.insert_entity(entity)?;
            revision = revision.saturating_add(1);
        }
        for artifact in &snapshot.artifacts {
            let key = UiEntityKey::new(UiEntityKind::Artifact, artifact.artifact_id.to_string())?;
            let value = serde_json::to_value(artifact)
                .map_err(|error| UiStoreError::Invalid(format!("ui_artifact_encode:{error}")))?;
            next.merge_snapshot_entity(key, revision, value)?;
            revision = revision.saturating_add(1);
        }
        next.compact_seen_events();
        Ok(UiStoreTransition {
            store: next,
            change: if epoch_reset {
                UiStoreChange::SnapshotRequired
            } else {
                UiStoreChange::SnapshotHydrated
            },
        })
    }

    fn preserve_local_state(&self, epoch: String) -> Result<Self, UiStoreError> {
        let mut optimistic = BTreeMap::new();
        for (idempotency_key, update) in &self.optimistic {
            let mut update = update.clone();
            if let Some(previous) = update.previous.as_mut() {
                previous.epoch = epoch.clone();
            }
            optimistic.insert(idempotency_key.clone(), update);
        }
        let mut preserved = Self {
            scope: self.scope.clone(),
            epoch,
            feed_sequence: 0,
            entities: BTreeMap::new(),
            seen_event_ids: BTreeSet::new(),
            optimistic,
            unknown_commands: self.unknown_commands.clone(),
            needs_snapshot: true,
            gap: Some(UiStoreGap {
                reason: UiStoreGapReason::EpochReset,
                expected_sequence: self.feed_sequence.saturating_add(1),
                observed_sequence: 0,
                epoch: self.epoch.clone(),
            }),
            max_entities: self.max_entities,
        };
        for (key, entity) in &self.entities {
            if entity.lifecycle.protected() {
                let mut retained = entity.clone();
                retained.epoch = preserved.epoch.clone();
                preserved.entities.insert(key.clone(), retained);
            }
        }
        preserved.ensure_capacity()?;
        Ok(preserved)
    }

    fn merge_snapshot_entity(
        &mut self,
        key: UiEntityKey,
        revision: u64,
        value: Value,
    ) -> Result<(), UiStoreError> {
        if let Some(current) = self.entities.get(&key) {
            if current.lifecycle.protected() {
                return Ok(());
            }
            if revision < current.revision {
                return Err(UiStoreError::StaleRevision(key.id));
            }
        }
        let entity = UiEntity {
            key,
            workspace: self.scope.workspace.clone(),
            session_id: self.scope.session_id.clone(),
            tab_id: self.scope.tab_id.clone(),
            epoch: self.epoch.clone(),
            revision: revision.max(1),
            lifecycle: UiEntityLifecycle::Authoritative,
            value,
            source_event_id: None,
        };
        self.insert_entity(entity)
    }

    fn reduce_feed(
        &self,
        scope: UiStoreScope,
        frame: UiFeedFrameV1,
    ) -> Result<UiStoreTransition, UiStoreError> {
        self.check_scope(&scope)?;
        if frame.schema != UI_FEED_FRAME_SCHEMA {
            return Err(UiStoreError::Invalid(
                "ui_feed_frame_schema_invalid".to_owned(),
            ));
        }
        frame.validate().map_err(UiStoreError::Invalid)?;
        if frame.kind == UiFeedFrameKind::Gap {
            let mut next = self.clone();
            next.needs_snapshot = true;
            next.gap = Some(UiStoreGap {
                reason: UiStoreGapReason::FeedGap,
                expected_sequence: self.feed_sequence.saturating_add(1),
                observed_sequence: frame.cursor.feed_sequence,
                epoch: frame.cursor.authority_epoch.clone(),
            });
            return Ok(UiStoreTransition {
                store: next,
                change: UiStoreChange::SnapshotRequired,
            });
        }
        if !self.epoch.is_empty() && frame.cursor.authority_epoch != self.epoch {
            return Err(UiStoreError::EpochMismatch);
        }
        if self.needs_snapshot && frame.kind != UiFeedFrameKind::SnapshotBoundary {
            return Err(UiStoreError::GapRequired);
        }
        if frame.cursor.feed_sequence <= self.feed_sequence {
            if self.seen_event_ids.contains(&frame.event_id) {
                return Err(UiStoreError::DuplicateEvent(frame.event_id));
            }
            return Err(UiStoreError::DuplicateFrame(frame.event_id));
        }
        let expected = self.feed_sequence.saturating_add(1);
        if self.feed_sequence > 0 && frame.cursor.feed_sequence != expected {
            let mut next = self.clone();
            next.needs_snapshot = true;
            next.gap = Some(UiStoreGap {
                reason: UiStoreGapReason::SequenceGap,
                expected_sequence: expected,
                observed_sequence: frame.cursor.feed_sequence,
                epoch: frame.cursor.authority_epoch.clone(),
            });
            return Ok(UiStoreTransition {
                store: next,
                change: UiStoreChange::SnapshotRequired,
            });
        }
        let mut next = self.clone();
        next.epoch = frame.cursor.authority_epoch.clone();
        next.feed_sequence = frame.cursor.feed_sequence;
        next.seen_event_ids.insert(frame.event_id.clone());
        next.compact_seen_events();
        if frame.kind == UiFeedFrameKind::SnapshotBoundary {
            next.needs_snapshot = false;
            next.gap = None;
            return Ok(UiStoreTransition {
                store: next,
                change: UiStoreChange::SnapshotHydrated,
            });
        }
        if matches!(
            frame.kind,
            UiFeedFrameKind::Heartbeat | UiFeedFrameKind::Unknown
        ) {
            return Ok(UiStoreTransition {
                store: next,
                change: UiStoreChange::EntityUpdated,
            });
        }
        let event = frame
            .event
            .ok_or_else(|| UiStoreError::Invalid("ui_feed_entity_missing".to_owned()))?;
        let object = event
            .as_object()
            .ok_or_else(|| UiStoreError::Invalid("ui_feed_entity_not_object".to_owned()))?;
        let event_session = object.get("session_id").and_then(Value::as_str);
        if event_session.is_some_and(|value| value != self.scope.session_id) {
            return Err(UiStoreError::ScopeMismatch);
        }
        let id = object
            .get("entity_id")
            .or_else(|| object.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| UiStoreError::Invalid("ui_feed_entity_id_missing".to_owned()))?;
        let kind = parse_entity_kind(object.get("entity_kind").and_then(Value::as_str))?;
        let revision = object
            .get("revision")
            .and_then(Value::as_u64)
            .unwrap_or(frame.cursor.feed_sequence)
            .max(1);
        let key = UiEntityKey::new(kind, id.to_owned())?;
        if let Some(current) = next.entities.get(&key) {
            if current.lifecycle.protected() {
                return Err(UiStoreError::StaleRevision(key.id));
            }
            if revision <= current.revision {
                return Err(UiStoreError::StaleRevision(key.id));
            }
        }
        let entity = UiEntity {
            key,
            workspace: next.scope.workspace.clone(),
            session_id: next.scope.session_id.clone(),
            tab_id: next.scope.tab_id.clone(),
            epoch: next.epoch.clone(),
            revision,
            lifecycle: UiEntityLifecycle::Authoritative,
            value: event,
            source_event_id: Some(frame.event_id),
        };
        let change = if next.entities.contains_key(&entity.key) {
            UiStoreChange::EntityUpdated
        } else {
            UiStoreChange::EntityInserted
        };
        next.insert_entity(entity)?;
        Ok(UiStoreTransition {
            store: next,
            change,
        })
    }

    fn reduce_optimistic(
        &self,
        mut update: UiOptimisticUpdate,
    ) -> Result<UiStoreTransition, UiStoreError> {
        update.validate()?;
        if self.optimistic.contains_key(&update.idempotency_key) {
            return Err(UiStoreError::DuplicateEvent(format!(
                "optimistic:{}",
                update.idempotency_key
            )));
        }
        if self
            .entities
            .get(&update.key)
            .is_some_and(|entity| entity.lifecycle == UiEntityLifecycle::OptimisticPending)
        {
            return Err(UiStoreError::StaleRevision(update.key.id.clone()));
        }
        if update.previous.is_none() {
            update.previous = self.entities.get(&update.key).cloned();
        }
        let mut next = self.clone();
        let entity = UiEntity {
            key: update.key.clone(),
            workspace: next.scope.workspace.clone(),
            session_id: next.scope.session_id.clone(),
            tab_id: next.scope.tab_id.clone(),
            epoch: next.epoch.clone(),
            revision: update.revision,
            lifecycle: UiEntityLifecycle::OptimisticPending,
            value: update.value.clone(),
            source_event_id: None,
        };
        next.entities.insert(update.key.clone(), entity);
        next.optimistic
            .insert(update.idempotency_key.clone(), update);
        next.ensure_capacity()?;
        Ok(UiStoreTransition {
            store: next,
            change: UiStoreChange::OptimisticPending,
        })
    }

    fn reduce_action_result(
        &self,
        scope: UiStoreScope,
        idempotency_key: String,
        result: UiActionResult,
    ) -> Result<UiStoreTransition, UiStoreError> {
        self.check_scope(&scope)?;
        result.validate().map_err(UiStoreError::Invalid)?;
        required(&idempotency_key, "ui_action_idempotency_key", 256)?;
        let mut next = self.clone();
        // An accepted command remains pending until a later applied, rejected, or
        // unknown result settles it. Keep the optimistic record so rollback and
        // expiry still have the original value available.
        let update = next.optimistic.get(&idempotency_key).cloned();
        if update
            .as_ref()
            .is_some_and(|update| update.command_id != result.command_id)
        {
            return Err(UiStoreError::Invalid(
                "ui_action_command_id_mismatch".to_owned(),
            ));
        }
        let key = update
            .as_ref()
            .map(|update| update.key.clone())
            .unwrap_or_else(|| UiEntityKey {
                kind: UiEntityKind::Submission,
                id: idempotency_key.clone(),
            });
        match result.disposition {
            UiActionDisposition::Accepted => {
                if let Some(entity) = next.entities.get_mut(&key) {
                    entity.lifecycle = UiEntityLifecycle::OptimisticPending;
                }
                if update.is_none() && !next.entities.contains_key(&key) {
                    next.insert_entity(UiEntity {
                        key,
                        workspace: next.scope.workspace.clone(),
                        session_id: next.scope.session_id.clone(),
                        tab_id: next.scope.tab_id.clone(),
                        epoch: next.epoch.clone(),
                        revision: result.resulting_revision.unwrap_or(1).max(1),
                        lifecycle: UiEntityLifecycle::OptimisticPending,
                        value: json!({"idempotency_key": idempotency_key}),
                        source_event_id: None,
                    })?;
                }
                Ok(UiStoreTransition {
                    store: next,
                    change: UiStoreChange::ActionAccepted,
                })
            }
            UiActionDisposition::Applied => {
                next.optimistic.remove(&idempotency_key);
                if let Some(entity) = next.entities.get_mut(&key) {
                    entity.lifecycle = UiEntityLifecycle::Authoritative;
                    if let Some(revision) = result.resulting_revision {
                        entity.revision = revision.max(entity.revision);
                    }
                }
                next.unknown_commands.remove(&idempotency_key);
                next.ensure_capacity()?;
                Ok(UiStoreTransition {
                    store: next,
                    change: UiStoreChange::ActionApplied,
                })
            }
            UiActionDisposition::Rejected => {
                next.optimistic.remove(&idempotency_key);
                if let Some(update) = update {
                    if let Some(previous) = update.previous {
                        next.entities.insert(previous.key.clone(), previous);
                    } else {
                        next.entities.remove(&update.key);
                    }
                } else {
                    next.entities.remove(&key);
                }
                next.unknown_commands.remove(&idempotency_key);
                Ok(UiStoreTransition {
                    store: next,
                    change: UiStoreChange::ActionRejected,
                })
            }
            UiActionDisposition::Unknown => {
                next.optimistic.remove(&idempotency_key);
                if let Some(entity) = next.entities.get_mut(&key) {
                    entity.lifecycle = UiEntityLifecycle::Unknown;
                } else {
                    next.insert_entity(UiEntity {
                        key,
                        workspace: next.scope.workspace.clone(),
                        session_id: next.scope.session_id.clone(),
                        tab_id: next.scope.tab_id.clone(),
                        epoch: next.epoch.clone(),
                        revision: result.resulting_revision.unwrap_or(1).max(1),
                        lifecycle: UiEntityLifecycle::Unknown,
                        value: json!({"idempotency_key": idempotency_key, "status": "unknown"}),
                        source_event_id: None,
                    })?;
                }
                next.unknown_commands.insert(idempotency_key);
                if next.unknown_commands.len() > UI_ENTITY_STORE_MAX_UNKNOWN {
                    return Err(UiStoreError::CacheFullProtected);
                }
                Ok(UiStoreTransition {
                    store: next,
                    change: UiStoreChange::ActionUnknown,
                })
            }
        }
    }

    fn reduce_expiry(
        &self,
        scope: UiStoreScope,
        now_unix_ms: u64,
    ) -> Result<UiStoreTransition, UiStoreError> {
        self.check_scope(&scope)?;
        if now_unix_ms == 0 {
            return Err(UiStoreError::Invalid("ui_store_clock_invalid".to_owned()));
        }
        let mut next = self.clone();
        let expired: Vec<String> = next
            .optimistic
            .iter()
            .filter(|(_, update)| update.expires_at_unix_ms <= now_unix_ms)
            .map(|(key, _)| key.clone())
            .collect();
        for idempotency_key in expired {
            if let Some(update) = next.optimistic.remove(&idempotency_key) {
                if let Some(previous) = update.previous {
                    next.entities.insert(previous.key.clone(), previous);
                } else {
                    next.entities.remove(&update.key);
                }
            }
        }
        Ok(UiStoreTransition {
            store: next,
            change: if expired.is_empty() {
                UiStoreChange::EntityUpdated
            } else {
                UiStoreChange::ExpiredOptimistic
            },
        })
    }

    fn insert_entity(&mut self, entity: UiEntity) -> Result<(), UiStoreError> {
        entity.validate()?;
        self.entities.insert(entity.key.clone(), entity);
        self.ensure_capacity()
    }

    fn ensure_capacity(&mut self) -> Result<(), UiStoreError> {
        while self.entities.len() > self.max_entities {
            let candidate = self
                .entities
                .iter()
                .find(|(_, entity)| !entity.lifecycle.protected())
                .map(|(key, _)| key.clone());
            let Some(candidate) = candidate else {
                return Err(UiStoreError::CacheFullProtected);
            };
            self.entities.remove(&candidate);
        }
        Ok(())
    }

    fn compact_seen_events(&mut self) {
        while self.seen_event_ids.len() > UI_ENTITY_STORE_MAX_EVENTS {
            if let Some(first) = self.seen_event_ids.iter().next().cloned() {
                self.seen_event_ids.remove(&first);
            } else {
                break;
            }
        }
    }
}

fn parse_entity_kind(value: Option<&str>) -> Result<UiEntityKind, UiStoreError> {
    match value.unwrap_or("run") {
        "run" => Ok(UiEntityKind::Run),
        "connection" | "session" => Ok(UiEntityKind::Connection),
        "submission" | "command" => Ok(UiEntityKind::Submission),
        "draft" => Ok(UiEntityKind::Draft),
        "inbox" | "action" => Ok(UiEntityKind::Inbox),
        "artifact" => Ok(UiEntityKind::Artifact),
        other => Err(UiStoreError::Invalid(format!(
            "ui_entity_kind_unknown:{other}"
        ))),
    }
}

impl UiStoreEvent {
    pub fn snapshot(scope: UiStoreScope, snapshot: UiSnapshotV1) -> Self {
        Self::Snapshot { scope, snapshot }
    }

    pub fn feed(scope: UiStoreScope, frame: UiFeedFrameV1) -> Self {
        Self::Feed { scope, frame }
    }

    pub fn action_result(
        scope: UiStoreScope,
        idempotency_key: impl Into<String>,
        result: UiActionResult,
    ) -> Self {
        Self::ActionResult {
            scope,
            idempotency_key: idempotency_key.into(),
            result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_reduce_does_not_mutate_input() {
        let scope = UiStoreScope::new("/repo", "session-1", "tab-1").unwrap();
        let store = UiEntityStore::new(scope.clone(), 2).unwrap();
        let update = UiOptimisticUpdate {
            idempotency_key: "command-1".to_owned(),
            command_id: kiana_protocol::RequestId::new(),
            key: UiEntityKey::new(UiEntityKind::Submission, "command-1").unwrap(),
            revision: 1,
            value: json!({"status":"sending"}),
            expires_at_unix_ms: u64::MAX,
            previous: None,
        };
        let transition = store.reduce(UiStoreEvent::Optimistic(update)).unwrap();
        assert!(store.is_empty());
        assert_eq!(transition.store.pending_count(), 1);
    }
}
