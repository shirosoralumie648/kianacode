//! Rebuildable notification lifecycle projector.
//!
//! The projector folds committed materializations plus append-only withdraw/supersede/expire
//! facts. It never deletes source history and never mutates HumanTask/Approval authority.

use crate::{NotificationMaterializer, ReplayProjection};
use kiana_domain::{
    validate_notification_runtime_event, HumanInboxItem, Notification, NotificationEventSource,
    NotificationId, NotificationLifecycleFact, NotificationLifecycleKind, ProjectionCheckpoint,
    RuntimeEvent,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use thiserror::Error;

pub const NOTIFICATION_PROJECTOR_SCHEMA: &str = "kiana.notification-projector.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationVisibility {
    Active,
    Withdrawn,
    Superseded,
    Expired,
}

impl NotificationVisibility {
    fn from_fact(fact: &NotificationLifecycleFact) -> Self {
        match fact.kind {
            NotificationLifecycleKind::Withdraw => Self::Withdrawn,
            NotificationLifecycleKind::Supersede => Self::Superseded,
            NotificationLifecycleKind::Expire => Self::Expired,
        }
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationProjectorError {
    #[error("notification_projector_invalid:{0}")]
    Invalid(String),
    #[error("notification_projector_cursor_conflict")]
    CursorConflict,
    #[error("notification_projector_epoch_regressed")]
    EpochRegressed,
    #[error("notification_projector_fact_conflict")]
    FactConflict,
    #[error("notification_projector_terminal_rewrite")]
    TerminalRewrite,
    #[error("notification_projector_materializer:{0}")]
    Materializer(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotificationProjector {
    materializer: NotificationMaterializer,
    lifecycle: BTreeMap<String, NotificationLifecycleFact>,
    source_cursor: u64,
    data_epoch: u64,
}

impl NotificationProjector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rebuild_from_committed(
        events: &[RuntimeEvent],
        source_cursor: u64,
        lifecycle_facts: &[NotificationLifecycleFact],
    ) -> Result<Self, NotificationProjectorError> {
        let mut projector = Self::new();
        projector.apply_committed(events, source_cursor, lifecycle_facts)?;
        Ok(projector)
    }

    pub fn apply_committed(
        &mut self,
        events: &[RuntimeEvent],
        source_cursor: u64,
        lifecycle_facts: &[NotificationLifecycleFact],
    ) -> Result<(), NotificationProjectorError> {
        if source_cursor == 0 || source_cursor < self.source_cursor {
            return Err(NotificationProjectorError::CursorConflict);
        }
        let mut next_materializer = self.materializer.clone();
        next_materializer
            .apply_committed(events, source_cursor)
            .map_err(NotificationProjectorError::Materializer)?;
        let mut next_lifecycle = self.lifecycle.clone();
        let mut next_epoch = self.data_epoch;
        for fact in lifecycle_facts {
            fact.validate()
                .map_err(NotificationProjectorError::Invalid)?;
            if fact.source_cursor > source_cursor {
                return Err(NotificationProjectorError::CursorConflict);
            }
            if fact.data_epoch < next_epoch {
                return Err(NotificationProjectorError::EpochRegressed);
            }
            next_epoch = next_epoch.max(fact.data_epoch);
            let key = fact.notification_id.to_string();
            if let Some(previous) = next_lifecycle.get(&key) {
                if previous.fact_digest == fact.fact_digest {
                    continue;
                }
                if previous.source_cursor >= fact.source_cursor {
                    return Err(NotificationProjectorError::FactConflict);
                }
                if NotificationVisibility::from_fact(previous) != NotificationVisibility::Active {
                    return Err(NotificationProjectorError::TerminalRewrite);
                }
            }
            next_lifecycle.insert(key, fact.clone());
        }
        self.materializer = next_materializer;
        self.lifecycle = next_lifecycle;
        self.source_cursor = source_cursor;
        self.data_epoch = next_epoch;
        Ok(())
    }

    pub fn visible_human_inbox_items(
        &self,
        recipient_id: &str,
    ) -> Result<Vec<HumanInboxItem>, NotificationProjectorError> {
        let items = self
            .materializer
            .human_inbox_items(recipient_id)
            .map_err(NotificationProjectorError::Materializer)?;
        Ok(items
            .into_iter()
            .filter(|item| {
                let Some(notification_id) = item.item_id.strip_prefix("notification:") else {
                    return false;
                };
                self.lifecycle
                    .get(notification_id)
                    .map(|fact| {
                        NotificationVisibility::from_fact(fact) == NotificationVisibility::Active
                    })
                    .unwrap_or(true)
            })
            .collect())
    }

    pub fn visibility(&self, notification_id: NotificationId) -> NotificationVisibility {
        self.lifecycle
            .get(&notification_id.to_string())
            .map(NotificationVisibility::from_fact)
            .unwrap_or(NotificationVisibility::Active)
    }

    pub fn source_cursor(&self) -> u64 {
        self.source_cursor
    }

    pub fn data_epoch(&self) -> u64 {
        self.data_epoch
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotificationProjection {
    checkpoint: Option<ProjectionCheckpoint>,
    notifications: Vec<Notification>,
}

impl Default for NotificationProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationProjection {
    pub fn new() -> Self {
        Self {
            checkpoint: None,
            notifications: Vec::new(),
        }
    }

    pub fn apply_committed(
        &mut self,
        events: &[RuntimeEvent],
        source_cursor: u64,
    ) -> Result<(), String> {
        let initial = json!({"notifications": self.notifications});
        if let Some(checkpoint) = self.checkpoint.as_ref() {
            if source_cursor == checkpoint.source_cursor {
                if events
                    .iter()
                    .all(|event| checkpoint.source_event_ids.contains(&event.event_id))
                {
                    return Ok(());
                }
                return Err("notification_projection_cursor_gap".to_owned());
            }
        }
        let replay = match self.checkpoint.as_ref() {
            Some(checkpoint) => ReplayProjection::from_checkpoint(
                "notifications",
                checkpoint,
                events,
                source_cursor,
                fold_notification,
            ),
            None => ReplayProjection::from_zero(
                "notifications",
                initial,
                events,
                source_cursor,
                fold_notification,
            ),
        }?;
        let notifications: Vec<Notification> = serde_json::from_value(
            replay
                .state()
                .get("notifications")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        )
        .map_err(|_| "notification_projection_state_invalid".to_owned())?;
        self.notifications = notifications;
        self.checkpoint = Some(replay.checkpoint()?);
        Ok(())
    }

    pub fn checkpoint(&self) -> Option<&ProjectionCheckpoint> {
        self.checkpoint.as_ref()
    }

    pub fn source_cursor(&self) -> u64 {
        self.checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.source_cursor)
            .unwrap_or(0)
    }

    pub fn notifications(&self) -> &[Notification] {
        &self.notifications
    }
}

fn fold_notification(mut state: Value, event: &RuntimeEvent) -> Result<Value, String> {
    let Some(_class) =
        validate_notification_runtime_event(event, NotificationEventSource::EventLog)?
    else {
        return Ok(state);
    };
    let value = event
        .data
        .get("notification")
        .cloned()
        .ok_or_else(|| "notification_projection_payload_missing".to_owned())?;
    let notification: Notification = serde_json::from_value(value)
        .map_err(|_| "notification_projection_payload_invalid".to_owned())?;
    notification.validate()?;
    let list = state
        .get_mut("notifications")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "notification_projection_state_invalid".to_owned())?;
    if !list
        .iter()
        .any(|item| item == &serde_json::to_value(&notification).unwrap())
    {
        if list.len() >= 4_096 {
            return Err("notification_projection_limit".to_owned());
        }
        list.push(
            serde_json::to_value(notification)
                .map_err(|_| "notification_projection_encode_failed".to_owned())?,
        );
    }
    Ok(state)
}
