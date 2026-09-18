//! Committed-only notification projection with a rebuildable source checkpoint.

use crate::ReplayProjection;
use kiana_domain::{
    validate_notification_runtime_event, Notification, NotificationEventSource,
    ProjectionCheckpoint, RuntimeEvent,
};
use serde_json::{json, Value};

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
