//! Notification bridge over the daemon-owned RunStream feed.
//!
//! This is a cursor/disposition adapter, not a second bus. It consumes `UiFeedFrameV1` produced by
//! the existing `RunStreamBus`, requires snapshot hydration after gaps, and never executes or
//! retries a notification action.

use kiana_domain::RunId;
use kiana_protocol::{UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const NOTIFICATION_STREAM_BRIDGE_SCHEMA: &str = "kiana.notification-stream-bridge.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationStreamCursor {
    pub schema: String,
    pub run_id: RunId,
    pub instance_id: String,
    pub authority_epoch: String,
    pub feed_sequence: u64,
    pub source_cursor: u64,
    pub terminal: bool,
    pub disposed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotificationStreamDisposition {
    SnapshotBoundary,
    SnapshotRequired(UiFeedGapReason),
    Accepted {
        feed_sequence: u64,
        source_cursor: u64,
    },
    Replayed,
    Heartbeat,
    Terminal,
    Disposed,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationStreamError {
    #[error("notification_stream_invalid:{0}")]
    Invalid(String),
    #[error("notification_stream_terminal_update")]
    TerminalUpdate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationStreamBridge {
    cursor: NotificationStreamCursor,
    snapshot_required: bool,
}

impl NotificationStreamBridge {
    pub fn new(
        run_id: RunId,
        instance_id: impl Into<String>,
        authority_epoch: impl Into<String>,
    ) -> Result<Self, NotificationStreamError> {
        let instance_id = instance_id.into();
        let authority_epoch = authority_epoch.into();
        if instance_id.trim().is_empty() || authority_epoch.trim().is_empty() {
            return Err(NotificationStreamError::Invalid("identity".to_owned()));
        }
        Ok(Self {
            cursor: NotificationStreamCursor {
                schema: NOTIFICATION_STREAM_BRIDGE_SCHEMA.to_owned(),
                run_id,
                instance_id,
                authority_epoch,
                feed_sequence: 0,
                source_cursor: 0,
                terminal: false,
                disposed: false,
            },
            snapshot_required: true,
        })
    }

    pub fn cursor(&self) -> &NotificationStreamCursor {
        &self.cursor
    }

    pub fn snapshot_required(&self) -> bool {
        self.snapshot_required
    }

    pub fn dispose(&mut self) {
        self.cursor.disposed = true;
    }

    pub fn ingest(
        &mut self,
        frame: &UiFeedFrameV1,
    ) -> Result<NotificationStreamDisposition, NotificationStreamError> {
        if self.cursor.disposed {
            return Ok(NotificationStreamDisposition::Disposed);
        }
        frame.validate().map_err(NotificationStreamError::Invalid)?;
        if frame.cursor.instance_id != self.cursor.instance_id {
            self.snapshot_required = true;
            return Ok(NotificationStreamDisposition::SnapshotRequired(
                UiFeedGapReason::InstanceChanged,
            ));
        }
        if frame.cursor.authority_epoch != self.cursor.authority_epoch {
            self.snapshot_required = true;
            return Ok(NotificationStreamDisposition::SnapshotRequired(
                UiFeedGapReason::OldEpoch,
            ));
        }
        if frame.kind == UiFeedFrameKind::Gap {
            self.snapshot_required = true;
            return Ok(NotificationStreamDisposition::SnapshotRequired(
                frame
                    .gap
                    .as_ref()
                    .ok_or_else(|| NotificationStreamError::Invalid("gap_missing".to_owned()))?
                    .reason
                    .clone(),
            ));
        }
        if frame.kind == UiFeedFrameKind::Heartbeat {
            self.cursor.source_cursor = self
                .cursor
                .source_cursor
                .max(frame.cursor.snapshot_cursor.sequence);
            return Ok(NotificationStreamDisposition::Heartbeat);
        }
        if frame.kind == UiFeedFrameKind::SnapshotBoundary {
            self.cursor.feed_sequence = frame.cursor.feed_sequence;
            self.cursor.source_cursor = frame.cursor.snapshot_cursor.sequence;
            self.snapshot_required = false;
            return Ok(NotificationStreamDisposition::SnapshotBoundary);
        }
        if self.snapshot_required {
            return Ok(NotificationStreamDisposition::SnapshotRequired(
                UiFeedGapReason::ReplayExpired,
            ));
        }
        if frame.cursor.feed_sequence <= self.cursor.feed_sequence {
            return Ok(NotificationStreamDisposition::Replayed);
        }
        if self.cursor.feed_sequence != 0
            && frame.cursor.feed_sequence != self.cursor.feed_sequence.saturating_add(1)
        {
            self.snapshot_required = true;
            return Ok(NotificationStreamDisposition::SnapshotRequired(
                UiFeedGapReason::SequenceGap,
            ));
        }
        if self.cursor.terminal {
            return Err(NotificationStreamError::TerminalUpdate);
        }
        self.cursor.feed_sequence = frame.cursor.feed_sequence;
        self.cursor.source_cursor = self
            .cursor
            .source_cursor
            .max(frame.cursor.snapshot_cursor.sequence);
        if frame.terminal {
            self.cursor.terminal = true;
            return Ok(NotificationStreamDisposition::Terminal);
        }
        self.snapshot_required = false;
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: self.cursor.feed_sequence,
            source_cursor: self.cursor.source_cursor,
        })
    }
}
