//! Notification bridge over the daemon-owned RunStream feed.
//!
//! This is a cursor/disposition adapter, not a second bus. It consumes `UiFeedFrameV1` produced by
//! the existing `RunStreamBus`, requires snapshot hydration after gaps, and never executes or
//! retries a notification action.
//!
//! Five honesty rules:
//!
//! 1. Snapshot-first: before the first boundary has been adopted nothing is rendered as a delta;
//!    the frame is refused with `SnapshotRequired`.
//! 2. Merge window: once a baseline exists, frames that race a fresh snapshot are kept in a bounded
//!    window and merged at the new boundary, so a reconnect never silently loses the interval
//!    between "subscribed" and "hydrated".  An overflow clears the window and keeps the snapshot
//!    requirement: a partial merge is never reported as complete.
//! 3. Dedupe by event id: an event that was already delivered never becomes a second item, even
//!    when the feed re-publishes it at a new position.
//! 4. Terminal retention: an already-seen position is reported as `Replayed`, but a replayed
//!    terminal still reports the run's terminal fact, and a terminal merged at the boundary stays
//!    visible to a reconnecting surface.
//! 5. No silent completion: a frame the surface cannot interpret is reported as `Unknown` with a
//!    snapshot requirement, a gap is never reported as a terminal, and a feed that ends without a
//!    terminal observation raises an explicit query alarm instead of pretending the run finished.
//!
//! Deltas (and merged frames) are volatile display frames: the bridge hands them back for
//! rendering and writes nothing itself.  The authoritative `run.finished`, approval and incident
//! facts stay queryable in the committed projection; this module only tells a surface when it has
//! to re-read them.

use kiana_domain::RunId;
use kiana_protocol::{UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use thiserror::Error;

pub const NOTIFICATION_STREAM_BRIDGE_SCHEMA: &str = "kiana.notification-stream-bridge.v1";

/// Upper bound on frames kept while a fresh snapshot boundary is pending.  The merge window exists
/// for the reconnect race (surface subscribed before the snapshot hydrated); beyond it the bridge
/// fails closed instead of holding unbounded display state.
pub const NOTIFICATION_STREAM_MERGE_WINDOW: usize = 64;

/// Upper bound on the event ids remembered for dedupe.  Delivering one event twice is worse than
/// forgetting an old id, so the oldest id is dropped first.
pub const NOTIFICATION_STREAM_SEEN_EVENT_IDS: usize = 256;

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
    /// A frame the surface cannot interpret.  It keeps its feed position (the sequence is still
    /// the server's) but must never be rendered as content, and the caller has to re-hydrate the
    /// authoritative snapshot before it observes more deltas.
    Unknown,
    Disposed,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationStreamError {
    #[error("notification_stream_invalid:{0}")]
    Invalid(String),
    /// The frame payload names another run.  Nothing about it may be rendered or merged into this
    /// bridge's cursor: the feed belongs to exactly one run.
    #[error("notification_stream_foreign_run")]
    ForeignRun,
    #[error("notification_stream_terminal_update")]
    TerminalUpdate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotificationStreamBridge {
    cursor: NotificationStreamCursor,
    snapshot_required: bool,
    /// True once a boundary was adopted.  The merge window only opens for a surface that already
    /// had a baseline to reconcile against; the very first attach must hydrate before it renders.
    boundary_adopted: bool,
    /// Frames buffered while a fresh boundary is pending (reconnect race, bounded).
    pending_merge: VecDeque<UiFeedFrameV1>,
    /// Frames delivered by the last boundary merge and not yet drained by the surface.
    merged: VecDeque<UiFeedFrameV1>,
    /// Bounded memory of delivered event ids, used to refuse a second delivery of one event.
    seen_event_ids: VecDeque<String>,
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
            boundary_adopted: false,
            pending_merge: VecDeque::new(),
            merged: VecDeque::new(),
            seen_event_ids: VecDeque::new(),
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

    /// True once the feed reported this run's terminal fact.
    pub fn terminal_observed(&self) -> bool {
        self.cursor.terminal
    }

    /// The surface must re-read the authoritative run facts instead of assuming completion: the
    /// feed cannot name the terminal now (it was disposed) or it never anchored one (a snapshot is
    /// still required).  `run.finished`, approval and incident facts remain queryable there.
    pub fn terminal_query_required(&self) -> bool {
        !self.cursor.terminal && (self.cursor.disposed || self.snapshot_required)
    }

    /// Buffered frames that still await the fresh boundary.
    pub fn merge_window_len(&self) -> usize {
        self.pending_merge.len()
    }

    /// Frames delivered by the last boundary merge and not yet drained.
    pub fn merged_frames_len(&self) -> usize {
        self.merged.len()
    }

    /// Drain the frames that were merged at the boundary.  They are volatile display frames: the
    /// caller renders them in order and must not treat them as committed facts.
    pub fn take_merged_frames(&mut self) -> Vec<UiFeedFrameV1> {
        self.merged.drain(..).collect()
    }

    pub fn ingest(
        &mut self,
        frame: &UiFeedFrameV1,
    ) -> Result<NotificationStreamDisposition, NotificationStreamError> {
        if self.cursor.disposed {
            return Ok(NotificationStreamDisposition::Disposed);
        }
        frame.validate().map_err(NotificationStreamError::Invalid)?;
        if let Some(run_id) = frame_payload_run_id(frame)? {
            if run_id != self.cursor.run_id {
                return Err(NotificationStreamError::ForeignRun);
            }
        }
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
            self.remember_event_id(&frame.event_id);
            self.snapshot_required = false;
            self.boundary_adopted = true;
            self.merge_pending_frames();
            return Ok(NotificationStreamDisposition::SnapshotBoundary);
        }
        if self.snapshot_required {
            // The surface has to hydrate before it observes deltas.  A recovery subscription also
            // keeps the frames that raced the hydration in a bounded window so the post-boundary
            // merge can tell superseded content from content the snapshot does not contain.
            if self.boundary_adopted {
                self.buffer_merge_frame(frame);
            }
            return Ok(NotificationStreamDisposition::SnapshotRequired(
                UiFeedGapReason::ReplayExpired,
            ));
        }
        // A replayed terminal is still this run's terminal fact: terminal retention has to stay
        // visible to a reconnecting client even when the frame sits at a position it already saw.
        // Degrading it to `Replayed` would hide that the run is finished.
        if frame.terminal {
            self.cursor.feed_sequence = self.cursor.feed_sequence.max(frame.cursor.feed_sequence);
            self.cursor.source_cursor = self
                .cursor
                .source_cursor
                .max(frame.cursor.snapshot_cursor.sequence);
            self.cursor.terminal = true;
            self.remember_event_id(&frame.event_id);
            return Ok(NotificationStreamDisposition::Terminal);
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
        if self.seen_event_ids.contains(&frame.event_id) {
            // The feed re-published an event the surface already rendered.  The position is not
            // advanced either: a later genuine frame at that position must still line up.
            return Ok(NotificationStreamDisposition::Replayed);
        }
        self.cursor.feed_sequence = frame.cursor.feed_sequence;
        self.cursor.source_cursor = self
            .cursor
            .source_cursor
            .max(frame.cursor.snapshot_cursor.sequence);
        self.remember_event_id(&frame.event_id);
        if frame.kind == UiFeedFrameKind::Unknown {
            // The position is real, but the payload is not renderable.  Ask for the snapshot
            // instead of letting the surface present an uninterpreted frame as a delta.
            self.snapshot_required = true;
            return Ok(NotificationStreamDisposition::Unknown);
        }
        self.snapshot_required = false;
        Ok(NotificationStreamDisposition::Accepted {
            feed_sequence: self.cursor.feed_sequence,
            source_cursor: self.cursor.source_cursor,
        })
    }

    fn buffer_merge_frame(&mut self, frame: &UiFeedFrameV1) {
        if self.pending_merge.len() >= NOTIFICATION_STREAM_MERGE_WINDOW {
            // A window this large cannot be merged honestly; fail closed and let the surface
            // hydrate again instead of reporting a partial merge.
            self.pending_merge.clear();
            self.snapshot_required = true;
            return;
        }
        self.pending_merge.push_back(frame.clone());
    }

    /// Merge the reconnect race at the freshly adopted boundary: frames at or below the boundary
    /// are already covered by the snapshot, the frames ahead of it must be contiguous or the
    /// window is unusable, and an event that was delivered before the gap is never delivered twice.
    fn merge_pending_frames(&mut self) {
        if self.pending_merge.is_empty() {
            return;
        }
        let boundary = self.cursor.feed_sequence;
        let mut pending = std::mem::take(&mut self.pending_merge);
        let mut expected = boundary.saturating_add(1);
        let mut merged = Vec::new();
        let mut contiguous = true;
        while let Some(frame) = pending.pop_front() {
            if frame.cursor.feed_sequence <= boundary {
                continue;
            }
            if frame.cursor.feed_sequence != expected {
                contiguous = false;
                break;
            }
            expected = frame.cursor.feed_sequence.saturating_add(1);
            if self.seen_event_ids.contains(&frame.event_id) {
                continue;
            }
            merged.push(frame);
        }
        if !contiguous {
            self.snapshot_required = true;
            self.merged.clear();
            return;
        }
        for frame in merged {
            self.cursor.feed_sequence = self.cursor.feed_sequence.max(frame.cursor.feed_sequence);
            self.cursor.source_cursor = self
                .cursor
                .source_cursor
                .max(frame.cursor.snapshot_cursor.sequence);
            self.cursor.terminal |= frame.terminal;
            self.remember_event_id(&frame.event_id);
            self.merged.push_back(frame);
        }
    }

    fn remember_event_id(&mut self, event_id: &str) {
        if self.seen_event_ids.iter().any(|seen| seen == event_id) {
            return;
        }
        self.seen_event_ids.push_back(event_id.to_owned());
        while self.seen_event_ids.len() > NOTIFICATION_STREAM_SEEN_EVENT_IDS {
            self.seen_event_ids.pop_front();
        }
    }
}

/// The run named by a frame payload, when the payload carries one.  Every data-carrying
/// `RunStreamEvent` is tagged at the top level; a payload that names no run is simply not checked
/// here, and a payload that names an unreadable run is malformed rather than silently accepted.
fn frame_payload_run_id(frame: &UiFeedFrameV1) -> Result<Option<RunId>, NotificationStreamError> {
    let Some(event) = frame.event.as_ref() else {
        return Ok(None);
    };
    let Some(value) = event.get("run_id") else {
        return Ok(None);
    };
    let Some(text) = value.as_str() else {
        return Err(NotificationStreamError::Invalid("frame_run".to_owned()));
    };
    RunId::parse_str(text)
        .map(Some)
        .ok_or_else(|| NotificationStreamError::Invalid("frame_run".to_owned()))
}
